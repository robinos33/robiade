//! Lectures GitLab gardées en mémoire, sur le même principe : ce qui ne change plus est gardé,
//! le reste est relu après quelques minutes.
//! - États des MR liées aux sessions : une MR mergée ou fermée ne change plus.
//! - Mes contributions, par tranches de 30 jours : une tranche lue après sa fin ne change plus.
//!
//! La MR d'un agent, elle, est relue après [`AGENT_TTL_MS`] : la frise de l'agent ouvert doit
//! suivre une MR qui vient d'être créée ou marquée prête.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use robiade_core::contributions::Contribution;
use robiade_core::forge::{Forge, ForgeError, MergeRequest, MrOutcomes, MrRef, MrState};
use robiade_core::Millis;

const OPEN_TTL_MS: Millis = 5 * 60 * 1000;
const AGENT_TTL_MS: Millis = 30 * 1000;
/// Lectures simultanées sur GitLab.
const PARALLEL_READS: usize = 8;
const DAY_MS: Millis = 86_400_000;
/// Tranche de lecture des événements : environ 15 pages GitLab pour un mois chargé.
const SLICE_MS: Millis = 30 * DAY_MS;

pub struct MrStates {
    forge: Arc<dyn Forge>,
    known: Mutex<HashMap<MrRef, (MrState, Millis)>>,
    /// MR illisibles (supprimées, sans droit, réseau), pas relues avant `OPEN_TTL_MS`.
    unreadable: Mutex<HashMap<MrRef, Millis>>,
}

impl MrStates {
    pub fn new(forge: Arc<dyn Forge>) -> Self {
        Self { forge, known: Mutex::default(), unreadable: Mutex::default() }
    }

    fn cached(&self, mr: &MrRef, now: Millis) -> Option<MrState> {
        let known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        let (state, read_at) = *known.get(mr)?;
        let settled = matches!(state, MrState::Merged | MrState::Closed);
        (settled || now - read_at < OPEN_TTL_MS).then_some(state)
    }

    /// Bilan des MR données. Une MR illisible compte comme inconnue.
    pub fn outcomes(&self, mrs: &[MrRef], now: Millis) -> Result<MrOutcomes, ForgeError> {
        let states = self.states(mrs, now)?;
        Ok(MrOutcomes::of(states.iter().map(Option::as_ref)))
    }

    /// État de chaque MR, `None` si elle est illisible ; un token absent ou refusé fait échouer
    /// le tout, pour que l'interface le dise au lieu d'afficher des MR toutes inconnues.
    pub fn states(&self, mrs: &[MrRef], now: Millis) -> Result<Vec<Option<MrState>>, ForgeError> {
        let recently_unreadable = |mr: &MrRef| {
            let unreadable = self.unreadable.lock().unwrap_or_else(|e| e.into_inner());
            unreadable.get(mr).is_some_and(|at| now - at < OPEN_TTL_MS)
        };
        let missing: Vec<&MrRef> =
            mrs.iter().filter(|mr| self.cached(mr, now).is_none() && !recently_unreadable(mr)).collect();
        for chunk in missing.chunks(PARALLEL_READS) {
            let read: Vec<_> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk.iter().map(|mr| s.spawn(|| self.forge.merge_request(mr))).collect();
                let panicked = || Err(ForgeError::Api("lecture interrompue".into()));
                handles.into_iter().map(|h| h.join().unwrap_or_else(|_| panicked())).collect()
            });
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for (mr, result) in chunk.iter().zip(read) {
                match result {
                    Ok(found) => {
                        known.insert((*mr).clone(), (found.state, now));
                    }
                    Err(e @ (ForgeError::NotConfigured | ForgeError::NoToken(_) | ForgeError::Unauthorized)) => return Err(e),
                    Err(_) => {
                        self.unreadable.lock().unwrap_or_else(|e| e.into_inner()).insert((*mr).clone(), now);
                    }
                }
            }
        }
        Ok(mrs.iter().map(|mr| self.cached(mr, now)).collect())
    }
}

/// Ce qui désigne la MR d'un agent : sa branche (projet en minuscules), ou le lien de sa session.
#[derive(PartialEq, Eq, Hash)]
enum Lookup {
    Branch(String, String),
    Link(MrRef),
}

/// MR des agents lues, `None` si la branche n'en a pas.
pub struct AgentMergeRequests {
    forge: Arc<dyn Forge>,
    /// MR et instant de lecture.
    known: Mutex<HashMap<Lookup, (Option<MergeRequest>, Millis)>>,
}

impl AgentMergeRequests {
    pub fn new(forge: Arc<dyn Forge>) -> Self {
        Self { forge, known: Mutex::default() }
    }

    fn cached(
        &self,
        key: Lookup,
        now: Millis,
        read: impl FnOnce() -> Result<Option<MergeRequest>, ForgeError>,
    ) -> Result<Option<MergeRequest>, ForgeError> {
        if let Some((mr, read_at)) = self.known.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            if now - read_at < AGENT_TTL_MS {
                return Ok(mr.clone());
            }
        }
        let mr = read()?;
        self.known.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (mr.clone(), now));
        Ok(mr)
    }

    pub fn of_branch(&self, project: &str, branch: &str, now: Millis) -> Result<Option<MergeRequest>, ForgeError> {
        let key = Lookup::Branch(project.to_ascii_lowercase(), branch.to_owned());
        self.cached(key, now, || self.forge.merge_request_of_branch(project, branch))
    }

    /// MR désignée par le `pr-link` d'une session, quel que soit son état, avec ses approbations
    /// si elle est ouverte.
    pub fn of_link(&self, mr: &MrRef, now: Millis) -> Result<Option<MergeRequest>, ForgeError> {
        self.cached(Lookup::Link(mr.clone()), now, || {
            let mut found = self.forge.merge_request(mr)?;
            if matches!(found.state, MrState::Draft | MrState::Opened) {
                found.approvals = self.forge.approvals(mr).ok();
            }
            Ok(Some(found))
        })
    }

    /// Après une action sur une MR : la prochaine lecture repart de la forge.
    pub fn forget(&self) {
        self.known.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Mes contributions GitLab, par tranches de [`SLICE_MS`] alignées sur l'epoch. Une tranche lue
/// plus d'un jour après sa fin est définitive : un événement ne change pas après coup.
pub struct Contributions {
    forge: Arc<dyn Forge>,
    /// Tranche → événements et instant de lecture.
    slices: Mutex<HashMap<i64, (Vec<Contribution>, Millis)>>,
}

impl Contributions {
    pub fn new(forge: Arc<dyn Forge>) -> Self {
        Self { forge, slices: Mutex::default() }
    }

    fn fresh(slice: i64, read_at: Millis, now: Millis) -> bool {
        let settled = read_at >= (slice + 1) * SLICE_MS + DAY_MS;
        settled || now - read_at < OPEN_TTL_MS
    }

    /// Mes contributions depuis `from`. Les tranches manquantes sont lues en parallèle ; une
    /// tranche illisible fait échouer le tout, pour ne jamais afficher une période à moitié vide.
    pub fn since(&self, from: Millis, now: Millis) -> Result<Vec<Contribution>, ForgeError> {
        let wanted = from.div_euclid(SLICE_MS)..=now.div_euclid(SLICE_MS);
        let missing: Vec<i64> = {
            let slices = self.slices.lock().unwrap_or_else(|e| e.into_inner());
            wanted.clone().filter(|k| !slices.get(k).is_some_and(|(_, at)| Self::fresh(*k, *at, now))).collect()
        };
        for chunk in missing.chunks(PARALLEL_READS) {
            let read: Vec<_> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|k| s.spawn(move || self.forge.my_contributions(k * SLICE_MS, (k + 1) * SLICE_MS)))
                    .collect();
                let panicked = || Err(ForgeError::Api("lecture interrompue".into()));
                handles.into_iter().map(|h| h.join().unwrap_or_else(|_| panicked())).collect()
            });
            let mut slices = self.slices.lock().unwrap_or_else(|e| e.into_inner());
            let mut failed = None;
            for (k, result) in chunk.iter().zip(read) {
                match result {
                    Ok(events) => {
                        slices.insert(*k, (events, now));
                    }
                    Err(e) => failed = failed.or(Some(e)),
                }
            }
            if let Some(e) = failed {
                return Err(e);
            }
        }
        let slices = self.slices.lock().unwrap_or_else(|e| e.into_inner());
        Ok(wanted
            .filter_map(|k| slices.get(&k))
            .flat_map(|(events, _)| events.iter().filter(|e| e.at >= from).copied())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use robiade_core::contributions::ContributionKind;
    use robiade_core::forge::{Approvals, Discussion, Issue, IssueRef, MergeRequest};
    use robiade_core::issue_actions::{IssuePlanned, NewIssue};
    use robiade_core::merge_actions::Planned;

    use super::*;

    /// Un commit au milieu de chaque tranche lue ; compte les lectures.
    #[derive(Default)]
    struct Events {
        reads: AtomicUsize,
    }

    impl Forge for Events {
        fn my_contributions(&self, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(vec![Contribution { kind: ContributionKind::Commits, at: (from + to) / 2, count: 1 }])
        }
        fn me(&self) -> Result<String, ForgeError> {
            unreachable!()
        }
        fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
            unreachable!()
        }
        fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
            unreachable!()
        }
        fn merge_request(&self, _: &MrRef) -> Result<MergeRequest, ForgeError> {
            unreachable!()
        }
        fn merge_request_of_branch(&self, _: &str, _: &str) -> Result<Option<MergeRequest>, ForgeError> {
            unreachable!()
        }
        fn my_issues(&self) -> Result<Vec<Issue>, ForgeError> {
            unreachable!()
        }
        fn issue(&self, _: &IssueRef) -> Result<Issue, ForgeError> {
            unreachable!()
        }
        fn approvals(&self, _: &MrRef) -> Result<Approvals, ForgeError> {
            unreachable!()
        }
        fn merge_request_discussions(&self, _: &MrRef) -> Result<Vec<Discussion>, ForgeError> {
            unreachable!()
        }
        fn issue_discussions(&self, _: &IssueRef) -> Result<Vec<Discussion>, ForgeError> {
            unreachable!()
        }
        fn my_project_issues(&self, _: &str) -> Result<Vec<Issue>, ForgeError> {
            unreachable!()
        }
        fn default_branch(&self, _: &str) -> Result<String, ForgeError> {
            unreachable!()
        }
        fn execute(&self, _: &MrRef, _: &Planned) -> Result<(), ForgeError> {
            unreachable!()
        }
        fn execute_on_issue(&self, _: &IssueRef, _: &IssuePlanned) -> Result<(), ForgeError> {
            unreachable!()
        }
        fn create_issue(&self, _: &NewIssue) -> Result<IssueRef, ForgeError> {
            unreachable!()
        }
    }

    #[test]
    fn rereads_only_the_slice_still_in_progress() {
        let forge = Arc::new(Events::default());
        let cache = Contributions::new(forge.clone());
        let now = 100 * SLICE_MS + 10 * DAY_MS;
        let from = 97 * SLICE_MS + 20 * DAY_MS;

        // Tranches 97 à 100 ; le commit de la 97e tombe avant `from`.
        assert_eq!(cache.since(from, now).unwrap().len(), 3);
        assert_eq!(forge.reads.load(Ordering::SeqCst), 4);

        // Dans le délai : rien n'est relu.
        cache.since(from, now + 60_000).unwrap();
        assert_eq!(forge.reads.load(Ordering::SeqCst), 4);

        // Délai passé : seule la tranche en cours est relue, les autres sont définitives.
        cache.since(from, now + OPEN_TTL_MS).unwrap();
        assert_eq!(forge.reads.load(Ordering::SeqCst), 5);
    }
}

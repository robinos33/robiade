//! Cycle de vie du ticket d'un agent : l'étape où il en est et ce qui le retient, déduits de faits
//! observés au moment de la lecture. Rien n'est gardé : une MR repassée en brouillon ramène la
//! frise en arrière d'elle-même.

use crate::forge::{MergeRequest, MrState, PipelineStatus};
use crate::issue_start::issue_of_branch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Rien d'écrit dans le dossier de travail.
    Exploration,
    /// Au moins un fichier du dossier de travail écrit, pas encore de MR.
    Code,
    Draft,
    Review,
    Merged,
    /// Fermée sans merge.
    Closed,
}

/// Ce qui retient une MR ouverte, le plus urgent seulement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    Conflicts,
    /// Pipeline en échec, annulé, arrêté sur un job manuel, ou en cours.
    Pipeline(PipelineStatus),
    Discussions,
    /// Approbations qui manquent.
    Approvals(u32),
    /// Approbations illisibles avec ce jeton : jamais lues comme accordées.
    ApprovalsUnknown,
    /// La forge refuse le merge pour une autre raison : son statut (`detailed_merge_status`), tel quel.
    NotMergeable(String),
    ReadyToMerge,
}

impl Blocker {
    /// Attend quelqu'un d'autre (la CI, les relecteurs, la forge) ; le reste est à faire.
    pub fn is_waiting(&self) -> bool {
        match self {
            Self::Pipeline(p) => *p == PipelineStatus::Running,
            Self::Approvals(_) | Self::ApprovalsUnknown => true,
            Self::NotMergeable(status) => {
                matches!(status.as_str(), "checking" | "unchecked" | "ci_still_running" | "blocked_status")
            }
            Self::Conflicts | Self::Discussions | Self::ReadyToMerge => false,
        }
    }
}

/// Ce qu'on sait de l'agent au moment de la lecture.
pub struct Facts<'a> {
    /// Branche extraite dans son dossier de travail ; `None` hors dépôt ou tête détachée.
    pub branch: Option<&'a str>,
    /// Fichiers qu'il a écrits dans son dossier de travail.
    pub worktree_writes: usize,
    /// MR de sa branche, quel que soit son état.
    pub merge_request: Option<&'a MergeRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    /// Issue de la branche de ticket ; `None` : la frise n'a pas d'étape Ticket.
    pub ticket: Option<u64>,
    pub step: Step,
    pub blocker: Option<Blocker>,
}

impl Lifecycle {
    pub fn of(facts: &Facts) -> Self {
        let step = match facts.merge_request.map(|m| m.state) {
            Some(MrState::Draft) => Step::Draft,
            Some(MrState::Opened) => Step::Review,
            Some(MrState::Merged) => Step::Merged,
            Some(MrState::Closed) => Step::Closed,
            None if facts.worktree_writes > 0 => Step::Code,
            None => Step::Exploration,
        };
        Self {
            ticket: facts.branch.and_then(issue_of_branch),
            step,
            blocker: facts.merge_request.and_then(blocker),
        }
    }
}

/// Du plus urgent au moins urgent. Un brouillon n'attend ni approbation ni merge : seuls ses
/// conflits, son pipeline et ses discussions comptent.
fn blocker(mr: &MergeRequest) -> Option<Blocker> {
    let ready = match mr.state {
        MrState::Draft => false,
        MrState::Opened => true,
        MrState::Merged | MrState::Closed => return None,
    };
    if mr.has_conflicts {
        return Some(Blocker::Conflicts);
    }
    if let Some(p @ (PipelineStatus::Failed | PipelineStatus::Canceled | PipelineStatus::Manual)) = mr.pipeline {
        return Some(Blocker::Pipeline(p));
    }
    if mr.unresolved_discussions {
        return Some(Blocker::Discussions);
    }
    if mr.pipeline == Some(PipelineStatus::Running) {
        return Some(Blocker::Pipeline(PipelineStatus::Running));
    }
    if !ready {
        return None;
    }
    Some(match &mr.approvals {
        None => Blocker::ApprovalsUnknown,
        Some(a) if a.left > 0 => Blocker::Approvals(a.left),
        Some(_) if mr.merge_status == "mergeable" => Blocker::ReadyToMerge,
        Some(_) => Blocker::NotMergeable(mr.merge_status.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{Approvals, MergeMethods, MrRef};

    fn mr(state: MrState) -> MergeRequest {
        MergeRequest {
            mr: MrRef { project: "acme/shop".into(), iid: 7 },
            title: String::new(),
            description: String::new(),
            author: "moi".into(),
            state,
            source_branch: "ticket-51-frise".into(),
            target_branch: "main".into(),
            web_url: String::new(),
            updated_at: 0,
            sha: String::new(),
            has_conflicts: false,
            unresolved_discussions: false,
            merge_status: "mergeable".into(),
            pipeline: Some(PipelineStatus::Success),
            pipeline_id: None,
            approvals: Some(Approvals { required: 1, left: 0, by: vec!["relecteur".into()] }),
            merge_methods: MergeMethods::ANY,
        }
    }

    fn of(writes: usize, mr: Option<&MergeRequest>) -> Lifecycle {
        Lifecycle::of(&Facts { branch: Some("ticket-51-frise"), worktree_writes: writes, merge_request: mr })
    }

    #[test]
    fn before_a_merge_request_only_a_written_file_moves_on_to_code() {
        assert_eq!(of(0, None), Lifecycle { ticket: Some(51), step: Step::Exploration, blocker: None });
        assert_eq!(of(3, None).step, Step::Code);
    }

    #[test]
    fn a_branch_that_is_not_a_ticket_has_no_ticket_step() {
        let free = Lifecycle::of(&Facts { branch: Some("main"), worktree_writes: 0, merge_request: None });
        assert_eq!(free.ticket, None);
        let detached = Lifecycle::of(&Facts { branch: None, worktree_writes: 0, merge_request: None });
        assert_eq!(detached.ticket, None);
    }

    #[test]
    fn the_merge_request_sets_the_step_even_when_nothing_was_written_here() {
        assert_eq!(of(0, Some(&mr(MrState::Draft))).step, Step::Draft);
        assert_eq!(of(0, Some(&mr(MrState::Opened))).step, Step::Review);
        assert_eq!(of(0, Some(&mr(MrState::Merged))).step, Step::Merged);
        assert_eq!(of(0, Some(&mr(MrState::Closed))).step, Step::Closed);
    }

    #[test]
    fn a_draft_is_held_back_by_conflicts_pipeline_or_discussions_only() {
        let mut draft = mr(MrState::Draft);
        draft.approvals = Some(Approvals { required: 1, left: 1, by: vec![] });
        assert_eq!(of(1, Some(&draft)).blocker, None, "ni approbation ni merge attendus d'un brouillon");
        draft.pipeline = Some(PipelineStatus::Running);
        assert_eq!(of(1, Some(&draft)).blocker, Some(Blocker::Pipeline(PipelineStatus::Running)));
        draft.unresolved_discussions = true;
        assert_eq!(of(1, Some(&draft)).blocker, Some(Blocker::Discussions));
    }

    #[test]
    fn the_most_urgent_blocker_of_an_open_merge_request_wins() {
        let mut open = mr(MrState::Opened);
        open.has_conflicts = true;
        open.pipeline = Some(PipelineStatus::Failed);
        open.unresolved_discussions = true;
        open.approvals = Some(Approvals { required: 2, left: 2, by: vec![] });
        let next = |m: &MergeRequest| of(1, Some(m)).blocker;
        assert_eq!(next(&open), Some(Blocker::Conflicts));
        open.has_conflicts = false;
        assert_eq!(next(&open), Some(Blocker::Pipeline(PipelineStatus::Failed)));
        open.pipeline = Some(PipelineStatus::Running);
        assert_eq!(next(&open), Some(Blocker::Discussions), "une discussion se traite pendant que la CI tourne");
        open.unresolved_discussions = false;
        assert_eq!(next(&open), Some(Blocker::Pipeline(PipelineStatus::Running)));
        open.pipeline = Some(PipelineStatus::Success);
        assert_eq!(next(&open), Some(Blocker::Approvals(2)));
        open.approvals = None;
        assert_eq!(next(&open), Some(Blocker::ApprovalsUnknown), "illisibles : jamais « prête »");
        open.approvals = Some(Approvals { required: 2, left: 0, by: vec![] });
        assert_eq!(next(&open), Some(Blocker::ReadyToMerge));
        open.merge_status = "need_rebase".into();
        assert_eq!(next(&open), Some(Blocker::NotMergeable("need_rebase".into())));
    }

    #[test]
    fn a_canceled_pipeline_or_a_manual_job_holds_the_merge_request_back() {
        for p in [PipelineStatus::Canceled, PipelineStatus::Manual] {
            let mut open = mr(MrState::Opened);
            open.pipeline = Some(p);
            assert_eq!(of(1, Some(&open)).blocker, Some(Blocker::Pipeline(p)));
        }
    }

    #[test]
    fn a_merged_or_closed_merge_request_holds_nothing_back() {
        for state in [MrState::Merged, MrState::Closed] {
            let mut done = mr(state);
            done.pipeline = Some(PipelineStatus::Failed);
            assert_eq!(of(1, Some(&done)).blocker, None);
        }
    }

    #[test]
    fn the_ci_and_the_reviewers_are_waited_for_the_rest_is_to_do() {
        let waiting = [
            Blocker::Pipeline(PipelineStatus::Running),
            Blocker::Approvals(1),
            Blocker::ApprovalsUnknown,
            Blocker::NotMergeable("checking".into()),
        ];
        assert!(waiting.iter().all(Blocker::is_waiting));
        let to_do = [
            Blocker::Conflicts,
            Blocker::Pipeline(PipelineStatus::Failed),
            Blocker::Pipeline(PipelineStatus::Manual),
            Blocker::Discussions,
            Blocker::NotMergeable("requested_changes".into()),
            Blocker::ReadyToMerge,
        ];
        assert!(!to_do.iter().any(Blocker::is_waiting));
    }
}

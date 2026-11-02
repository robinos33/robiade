//! Activité GitLab : mes contributions par jour, semaine, mois ou année.
//!
//! Sources : mes événements GitLab, et mes MR mergées. Un événement a pour auteur celui qui agit,
//! d'où ce que compte chaque série :
//! - **MR ouvertes** : `opened` sur une MR ;
//! - **MR mergées** : mes MR mergées, par moi ou par n'importe qui, datées de leur merge. Elles ne
//!   viennent pas des événements : le merge d'une MR à moi par un collègue est un événement à lui.
//!   Une MR d'un autre que j'ai mergée n'y est pas ;
//! - **commits** : `commit_count` de chaque push (`pushed to`, `pushed new`). Un push forcé après
//!   rebase recompte ses commits : c'est un volume poussé, pas un nombre de commits distincts ;
//! - **reviews** : `approved` sur une MR. Les commentaires ne comptent pas : rien dans l'événement
//!   ne dit si la MR commentée est la mienne, et répondre sur sa propre MR n'est pas une review.

use crate::Millis;

const DAY_MS: Millis = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributionKind {
    MrOpened,
    MrMerged,
    Commits,
    Review,
}

/// Un événement GitLab retenu, réduit à ce que les statistiques en lisent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contribution {
    pub kind: ContributionKind,
    pub at: Millis,
    pub count: u32,
}

impl Contribution {
    /// Une de mes MR, mergée à l'instant `merged_at`.
    pub fn merged(merged_at: Millis) -> Self {
        Self { kind: ContributionKind::MrMerged, at: merged_at, count: 1 }
    }

    /// `None` pour un événement qui n'est pas une contribution comptée (commentaire, suppression
    /// de branche, issue, merge — lu ailleurs…). `commit_count` : celui du `push_data`, s'il y en a un.
    pub fn classify(action: &str, target_type: Option<&str>, commit_count: Option<u32>, at: Millis) -> Option<Self> {
        let on_mr = target_type == Some("MergeRequest");
        let (kind, count) = match action {
            "opened" if on_mr => (ContributionKind::MrOpened, 1),
            "approved" if on_mr => (ContributionKind::Review, 1),
            "pushed to" | "pushed new" => (ContributionKind::Commits, commit_count.filter(|c| *c > 0)?),
            _ => return None,
        };
        Some(Self { kind, at, count })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
    Month,
    Year,
}

impl Granularity {
    /// Nombre de périodes affichées, la courante comprise. Trois ans au plus : GitLab ne garde
    /// pas les événements au-delà.
    pub fn periods(self) -> usize {
        match self {
            Self::Day => 30,
            Self::Week => 26,
            Self::Month => 12,
            Self::Year => 3,
        }
    }

    /// Premier jour de la période qui contient `day`. Une semaine commence le lundi.
    fn start_of(self, day: i64) -> i64 {
        match self {
            Self::Day => day,
            Self::Week => day - (day + 3).rem_euclid(7),
            Self::Month => {
                let (y, m, _) = civil_from_days(day);
                days_from_civil(y, m, 1)
            }
            Self::Year => days_from_civil(civil_from_days(day).0, 1, 1),
        }
    }

    /// Premier jour de la période qui précède celle commençant à `start`.
    fn previous(self, start: i64) -> i64 {
        match self {
            Self::Day => start - 1,
            Self::Week => start - 7,
            Self::Month | Self::Year => self.start_of(start - 1),
        }
    }
}

/// Contributions d'une période.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContributionBucket {
    /// Premier jour local de la période, en jours depuis l'epoch.
    pub start_day: i64,
    pub mrs_opened: u32,
    pub mrs_merged: u32,
    pub commits: u32,
    pub reviews: u32,
}

/// Premiers jours locaux des périodes affichées, de la plus ancienne à la courante.
fn period_starts(granularity: Granularity, now: Millis, utc_offset_ms: Millis) -> Vec<i64> {
    let today = (now + utc_offset_ms).div_euclid(DAY_MS);
    let mut starts = vec![granularity.start_of(today)];
    while starts.len() < granularity.periods() {
        starts.push(granularity.previous(*starts.last().unwrap_or(&today)));
    }
    starts.reverse();
    starts
}

/// Instant à partir duquel il faut lire les événements : minuit local du premier jour affiché.
pub fn window_start(granularity: Granularity, now: Millis, utc_offset_ms: Millis) -> Millis {
    period_starts(granularity, now, utc_offset_ms)[0] * DAY_MS - utc_offset_ms
}

/// Contributions regroupées par période locale, toutes présentes, vides comprises.
/// Les événements hors des périodes affichées sont ignorés.
pub fn by_period(
    events: &[Contribution],
    granularity: Granularity,
    now: Millis,
    utc_offset_ms: Millis,
) -> Vec<ContributionBucket> {
    let starts = period_starts(granularity, now, utc_offset_ms);
    let mut buckets: Vec<ContributionBucket> =
        starts.iter().map(|&start_day| ContributionBucket { start_day, ..Default::default() }).collect();
    for e in events.iter().filter(|e| e.at <= now) {
        let day = (e.at + utc_offset_ms).div_euclid(DAY_MS);
        // Dernière période commençant au plus tard ce jour-là.
        let Some(i) = starts.partition_point(|&s| s <= day).checked_sub(1) else { continue };
        let b = &mut buckets[i];
        let field = match e.kind {
            ContributionKind::MrOpened => &mut b.mrs_opened,
            ContributionKind::MrMerged => &mut b.mrs_merged,
            ContributionKind::Commits => &mut b.commits,
            ContributionKind::Review => &mut b.reviews,
        };
        *field += e.count;
    }
    buckets
}

/// Jours depuis l'epoch → (année, mois, jour), calendrier grégorien proleptique.
/// Algorithme de Howard Hinnant (`civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// (année, mois, jour) → jours depuis l'epoch ; réciproque de [`civil_from_days`].
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Midi UTC du jour donné.
    fn noon(y: i64, m: u32, d: u32) -> Millis {
        days_from_civil(y, m, d) * DAY_MS + DAY_MS / 2
    }

    fn event(kind: ContributionKind, at: Millis, count: u32) -> Contribution {
        Contribution { kind, at, count }
    }

    #[test]
    fn calendar_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 2)), (2026, 11, 2));
        assert_eq!(days_from_civil(2026, 3, 1) - days_from_civil(2026, 2, 28), 1);
    }

    #[test]
    fn classifies_the_events_seen_on_gitlab() {
        let c = |a, t, n| Contribution::classify(a, t, n, 0).map(|c| (c.kind, c.count));
        assert_eq!(c("opened", Some("MergeRequest"), None), Some((ContributionKind::MrOpened, 1)));
        // Un merge vient de mes MR mergées, pas de l'événement : il ne compterait que les miens.
        assert_eq!(c("accepted", Some("MergeRequest"), None), None);
        assert_eq!(c("approved", Some("MergeRequest"), None), Some((ContributionKind::Review, 1)));
        assert_eq!(c("pushed to", None, Some(3)), Some((ContributionKind::Commits, 3)));
        assert_eq!(c("pushed new", Some("Project"), Some(2)), Some((ContributionKind::Commits, 2)));
        // Suppression de branche, commentaire, issue : pas des contributions comptées.
        assert_eq!(c("deleted", None, Some(0)), None);
        assert_eq!(c("pushed to", None, Some(0)), None);
        assert_eq!(c("commented on", Some("DiffNote"), None), None);
        assert_eq!(c("opened", Some("Issue"), None), None);
    }

    #[test]
    fn weeks_start_on_monday() {
        // Lundi 2 novembre 2026.
        let monday = days_from_civil(2026, 11, 2);
        assert_eq!(Granularity::Week.start_of(monday), monday);
        assert_eq!(Granularity::Week.start_of(monday + 6), monday);
        assert_eq!(Granularity::Week.start_of(monday - 1), monday - 7);
    }

    #[test]
    fn lists_every_period_oldest_first_including_empty_ones() {
        let now = noon(2026, 11, 2);
        let months = by_period(&[], Granularity::Month, now, 0);
        assert_eq!(months.len(), 12);
        assert_eq!(months[0].start_day, days_from_civil(2025, 12, 1));
        assert_eq!(months[11].start_day, days_from_civil(2026, 11, 1));

        let years = by_period(&[], Granularity::Year, now, 0);
        let starts: Vec<_> = years.iter().map(|b| civil_from_days(b.start_day)).collect();
        assert_eq!(starts, [(2024, 1, 1), (2025, 1, 1), (2026, 1, 1)]);
        assert_eq!(window_start(Granularity::Year, now, 0), days_from_civil(2024, 1, 1) * DAY_MS);
    }

    #[test]
    fn adds_each_series_into_its_local_period() {
        let now = noon(2026, 11, 2);
        let events = [
            event(ContributionKind::Commits, noon(2026, 10, 30), 4),
            event(ContributionKind::Commits, noon(2026, 10, 1), 2),
            event(ContributionKind::MrOpened, noon(2026, 10, 15), 1),
            event(ContributionKind::Review, noon(2026, 11, 1), 1),
            // Hors fenêtre : ignoré.
            event(ContributionKind::MrMerged, noon(2025, 11, 30), 1),
        ];
        let months = by_period(&events, Granularity::Month, now, 0);
        let october = &months[10];
        assert_eq!((october.commits, october.mrs_opened, october.reviews), (6, 1, 0));
        assert_eq!(months[11].reviews, 1);
        assert_eq!(months.iter().map(|b| b.mrs_merged).sum::<u32>(), 0);
    }

    #[test]
    fn uses_the_local_day_not_the_utc_one() {
        // 23 h 30 UTC le 1er novembre, soit 0 h 30 le 2 à Paris (UTC+1).
        let late = days_from_civil(2026, 11, 2) * DAY_MS - 30 * 60_000;
        let now = noon(2026, 11, 2);
        let events = [event(ContributionKind::Commits, late, 1)];
        let paris = by_period(&events, Granularity::Day, now, 3_600_000);
        let utc = by_period(&events, Granularity::Day, now, 0);
        assert_eq!((paris[29].commits, paris[28].commits), (1, 0));
        assert_eq!((utc[29].commits, utc[28].commits), (0, 1));
    }
}

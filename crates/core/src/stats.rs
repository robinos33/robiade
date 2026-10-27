//! Statistiques : modèle de lecture calculé à partir de l'historique.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::history::{SessionSummary, TokenUsage};
use crate::Millis;

const DAY_MS: Millis = 86_400_000;

#[derive(Debug, Clone, Default)]
pub struct DayActivity {
    /// Jour local, en nombre de jours depuis l'epoch.
    pub day: i64,
    pub sessions: u32,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectActivity {
    pub project_path: String,
    pub sessions: u32,
    pub tokens: TokenUsage,
    pub last_activity_at: Millis,
}

#[derive(Debug, Clone, Default)]
pub struct Overview {
    pub sessions: u32,
    pub tokens: TokenUsage,
    pub tool_calls: u64,
    pub merge_requests: u32,
    /// Jours consécutifs, de `from_day` à aujourd'hui inclus, y compris les jours vides.
    pub days: Vec<DayActivity>,
    pub projects: Vec<ProjectActivity>,
    pub tools: Vec<(String, u64)>,
}

/// `utc_offset_ms` : décalage du fuseau local, pour que « aujourd'hui » soit le mien.
/// `project_of` : racine du projet d'un dossier de travail (lire le disque est l'affaire des adapters).
pub fn overview(
    sessions: &[SessionSummary],
    project_of: impl Fn(&str) -> String,
    now: Millis,
    utc_offset_ms: Millis,
    window_days: i64,
) -> Overview {
    let local_day = |at: Millis| (at + utc_offset_ms).div_euclid(DAY_MS);
    let today = local_day(now);
    let from_day = today - window_days + 1;

    let mut out = Overview::default();
    let mut days: BTreeMap<i64, DayActivity> =
        (from_day..=today).map(|day| (day, DayActivity { day, ..Default::default() })).collect();
    let mut projects: BTreeMap<String, ProjectActivity> = BTreeMap::new();
    let mut tools: BTreeMap<&str, u64> = BTreeMap::new();

    for s in sessions.iter().filter(|s| local_day(s.last_activity_at) >= from_day) {
        out.sessions += 1;
        out.tokens.add(&s.tokens);
        out.merge_requests += u32::from(s.merge_request_url.is_some());
        if let Some(d) = days.get_mut(&local_day(s.started_at.max(now - window_days * DAY_MS))) {
            d.sessions += 1;
            d.tokens += s.tokens.total();
        }
        let root = project_of(&s.project_path);
        let p = projects.entry(root.clone()).or_insert_with(|| ProjectActivity {
            project_path: root,
            ..Default::default()
        });
        p.sessions += 1;
        p.tokens.add(&s.tokens);
        p.last_activity_at = p.last_activity_at.max(s.last_activity_at);
        for (name, count) in &s.tools {
            *tools.entry(name).or_default() += u64::from(*count);
            out.tool_calls += u64::from(*count);
        }
    }

    out.days = days.into_values().collect();
    out.projects = projects.into_values().collect();
    out.projects.sort_by_key(|p| Reverse(p.tokens.total()));
    out.tools = tools.into_iter().map(|(n, c)| (n.to_owned(), c)).collect();
    out.tools.sort_by_key(|t| Reverse(t.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(project: &str, at: Millis, output: u64, tools: &[(&str, u32)]) -> SessionSummary {
        SessionSummary {
            session_id: format!("{project}-{at}"),
            project_path: project.into(),
            title: String::new(),
            git_branch: None,
            started_at: at,
            last_activity_at: at,
            user_messages: 1,
            assistant_messages: 1,
            tokens: TokenUsage { output, ..Default::default() },
            tools: tools.iter().map(|(n, c)| ((*n).into(), *c)).collect(),
            models: vec![],
            merge_request_url: None,
        }
    }

    #[test]
    fn buckets_by_local_day_and_ignores_older_sessions() {
        let now = 10 * DAY_MS + 3_600_000;
        let sessions = [
            session("a", now - 1_000, 5, &[("Bash", 2)]),
            session("b", now - DAY_MS, 7, &[("Bash", 1), ("Read", 4)]),
            session("a", now - 30 * DAY_MS, 100, &[]),
        ];
        let o = overview(&sessions, str::to_owned, now, 0, 7);
        assert_eq!(o.sessions, 2);
        assert_eq!(o.days.len(), 7);
        assert_eq!(o.days.last().unwrap().sessions, 1);
        assert_eq!(o.days[5].sessions, 1);
        assert_eq!(o.tools[0], ("Read".into(), 4));
        assert_eq!(o.projects[0].project_path, "b");
    }
}

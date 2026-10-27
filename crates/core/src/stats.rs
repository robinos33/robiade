//! Statistiques : modèle de lecture calculé à partir de l'historique.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::history::{SessionSummary, TokenUsage};
use crate::pricing::{self, Estimate};
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
    pub cost: Estimate,
    pub last_activity_at: Millis,
}

#[derive(Debug, Clone, Default)]
pub struct ModelActivity {
    pub model: String,
    pub tokens: TokenUsage,
    /// `None` si le modèle n'est pas dans la grille tarifaire.
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct Overview {
    pub sessions: u32,
    pub tokens: TokenUsage,
    pub cost: Estimate,
    pub tool_calls: u64,
    pub merge_requests: u32,
    /// Du premier au dernier message ; une session laissée ouverte une nuit compte sa nuit.
    pub median_duration_ms: Option<Millis>,
    /// Jours consécutifs, de `from_day` à aujourd'hui inclus, y compris les jours vides.
    pub days: Vec<DayActivity>,
    pub projects: Vec<ProjectActivity>,
    pub models: Vec<ModelActivity>,
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
    let mut durations: Vec<Millis> = Vec::new();
    let mut models: BTreeMap<&str, TokenUsage> = BTreeMap::new();

    for s in sessions.iter().filter(|s| local_day(s.last_activity_at) >= from_day) {
        out.sessions += 1;
        out.tokens.add(&s.tokens);
        durations.push((s.last_activity_at - s.started_at).max(0));
        let cost = session_cost(s);
        out.cost.add(cost);
        for (model, usage) in &s.tokens_by_model {
            models.entry(model).or_default().add(usage);
        }
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
        p.cost.add(cost);
        p.last_activity_at = p.last_activity_at.max(s.last_activity_at);
        for (name, count) in &s.tools {
            *tools.entry(name).or_default() += u64::from(*count);
            out.tool_calls += u64::from(*count);
        }
    }

    durations.sort_unstable();
    out.median_duration_ms = median(&durations);
    out.days = days.into_values().collect();
    out.projects = projects.into_values().collect();
    out.projects.sort_by_key(|p| Reverse(p.tokens.total()));
    out.models = models
        .into_iter()
        .map(|(model, tokens)| ModelActivity {
            model: model.to_owned(),
            cost: pricing::price_of(model).map(|p| pricing::cost(&tokens, &p)),
            tokens,
        })
        .collect();
    out.models.sort_by_key(|m| Reverse(m.tokens.total()));
    out.tools = tools.into_iter().map(|(n, c)| (n.to_owned(), c)).collect();
    out.tools.sort_by_key(|t| Reverse(t.1));
    out
}

/// Coût estimé d'une session, sous-agents compris.
pub fn session_cost(s: &SessionSummary) -> Estimate {
    Estimate::of(s.tokens_by_model.iter().map(|(m, u)| (m.as_str(), u)))
}

/// Médiane d'une liste triée ; moyenne des deux valeurs centrales pour un nombre pair.
fn median(sorted: &[Millis]) -> Option<Millis> {
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[mid]),
        _ => Some((sorted[mid - 1] + sorted[mid]) / 2),
    }
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
            tokens_by_model: vec![],
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

    #[test]
    fn costs_add_up_by_project_and_by_model() {
        let now = 10 * DAY_MS;
        let usage = TokenUsage { output: 1_000_000, ..Default::default() };
        let mut a = session("a", now, 0, &[]);
        a.tokens_by_model =
            vec![("claude-opus-5-5".into(), usage.clone()), ("claude-haiku-4-5-20251001".into(), usage.clone())];
        let mut b = session("b", now, 0, &[]);
        b.tokens_by_model = vec![("claude-opus-5-5".into(), usage.clone()), ("modele-inconnu".into(), usage)];
        let o = overview(&[a, b], str::to_owned, now, 0, 7);
        assert!((o.cost.dollars - (20.0 + 5.0 + 20.0)).abs() < 1e-9);
        assert_eq!(o.cost.unpriced_tokens, 1_000_000);
        assert_eq!(o.models[0].model, "claude-opus-5-5");
        assert_eq!(o.models.iter().find(|m| m.model == "modele-inconnu").unwrap().cost, None);
        let project_a = o.projects.iter().find(|p| p.project_path == "a").unwrap();
        assert!((project_a.cost.dollars - 25.0).abs() < 1e-9);
    }

    #[test]
    fn median_duration_runs_from_first_to_last_message() {
        let now = 10 * DAY_MS;
        let mut sessions: Vec<SessionSummary> = [10, 30, 20, 1000]
            .iter()
            .map(|m| SessionSummary { started_at: now - m * 60_000, ..session("a", now, 0, &[]) })
            .collect();
        assert_eq!(overview(&sessions, str::to_owned, now, 0, 7).median_duration_ms, Some(25 * 60_000));
        sessions.truncate(3);
        assert_eq!(overview(&sessions, str::to_owned, now, 0, 7).median_duration_ms, Some(20 * 60_000));
        assert_eq!(overview(&[], str::to_owned, now, 0, 7).median_duration_ms, None);
    }
}

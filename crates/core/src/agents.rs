//! Agents : sessions Claude Code lancées par Robiade, et leur status.

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::Assets;
use crate::Millis;

/// Identifiant d'un agent, attribué par Robiade au lancement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    /// Accepte uniquement un UUID textuel (36 caractères, hexadécimal et tirets) :
    /// la valeur finit en argument de ligne de commande et en variable d'environnement.
    pub fn parse(raw: &str) -> Result<Self, InvalidAgentId> {
        let well_formed = raw.len() == 36
            && raw.chars().enumerate().all(|(i, c)| match i {
                8 | 13 | 18 | 23 => c == '-',
                _ => c.is_ascii_hexdigit(),
            });
        if well_formed {
            Ok(Self(raw.to_ascii_lowercase()))
        } else {
            Err(InvalidAgentId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("identifiant d'agent invalide")]
pub struct InvalidAgentId;

/// Nom qu'on donne à un onglet d'agent pour s'y retrouver ; remplace le titre tiré du premier prompt.
pub const NAME_MAX_CHARS: usize = 60;

/// Mode de permission de Claude Code (`default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`…),
/// tel que rapporté par les hooks. Les modes inconnus sont gardés tels quels : le CLI en ajoute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionMode(String);

impl PermissionMode {
    /// Un identifiant court en lettres ASCII ; toute autre valeur est ignorée.
    pub fn parse(raw: &str) -> Option<Self> {
        let well_formed = (1..=32).contains(&raw.len()) && raw.chars().all(|c| c.is_ascii_alphabetic());
        well_formed.then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// L'agent agit sans me demander : indicateur de sécurité, pas de confort.
    pub fn is_permissive(&self) -> bool {
        matches!(self.0.as_str(), "auto" | "bypassPermissions")
    }
}

/// Effort de raisonnement de la session (`low`, `medium`, `high`, `xhigh`, `max`…), tel que rapporté
/// par les hooks. Comme pour le mode de permission, une valeur inconnue mais bien formée est gardée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effort(String);

impl Effort {
    /// Un identifiant court en minuscules ASCII ; toute autre valeur est ignorée.
    pub fn parse(raw: &str) -> Option<Self> {
        let well_formed = (1..=16).contains(&raw.len()) && raw.chars().all(|c| c.is_ascii_lowercase());
        well_formed.then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Jauges rapportées par la statusline de Claude Code : modèle, contexte, quotas.
/// Chaque champ est optionnel : le JSON de la statusline n'est pas un contrat stable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gauges {
    pub model: Option<String>,
    pub context_percent: Option<u8>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Quotas du compte, pas de la session : le dernier agent à les rapporter fait foi.
    pub five_hour_percent: Option<u8>,
    pub five_hour_resets_at: Option<Millis>,
    pub week_percent: Option<u8>,
}

/// Seuils de ma statusline : sous 50 % tout va bien, à partir de 80 % il faut agir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Comfortable,
    Watch,
    Critical,
}

impl Level {
    pub fn of(percent: u8) -> Self {
        match percent {
            0..=49 => Self::Comfortable,
            50..=79 => Self::Watch,
            _ => Self::Critical,
        }
    }
}

/// Événement de cycle de vie, traduit depuis les hooks Claude Code par l'adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    SessionStarted,
    PromptSubmitted,
    ToolStarted { tool: String },
    ToolFinished,
    PermissionRequested,
    WaitingForInput,
    TurnFinished,
    TurnFailed,
    SessionEnded,
    ProcessExited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Starting,
    Running,
    AwaitingPermission,
    AwaitingInput,
    Idle,
    Failed,
    Ended,
}

impl AgentStatus {
    /// Le status exige-t-il une action de ma part ?
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::AwaitingPermission | Self::AwaitingInput | Self::Failed)
    }

    /// Fermer Robiade interromprait un travail en cours : la fermeture demande confirmation.
    pub fn interrupted_by_quit(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingPermission)
    }

    /// L'agent est bloqué tant que je n'agis pas (niveau 1 de l'Inbox).
    pub fn is_waiting_on_me(self) -> bool {
        matches!(self, Self::AwaitingPermission | Self::AwaitingInput)
    }

    fn next(self, event: &AgentEvent) -> Self {
        use AgentEvent as E;
        if self == Self::Ended {
            return self;
        }
        match event {
            E::SessionStarted => Self::Idle,
            E::PromptSubmitted | E::ToolStarted { .. } | E::ToolFinished => Self::Running,
            E::PermissionRequested => Self::AwaitingPermission,
            // Une notification d'inactivité ne doit pas faire remonter une session déjà lue.
            E::WaitingForInput if self == Self::Idle => Self::Idle,
            E::WaitingForInput | E::TurnFinished => Self::AwaitingInput,
            E::TurnFailed => Self::Failed,
            E::SessionEnded | E::ProcessExited => Self::Ended,
        }
    }
}

/// Ce qu'un onglet garde d'une exécution de Robiade à l'autre : son nom et ses compteurs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Saved {
    pub name: Option<String>,
    pub started_at: Millis,
    pub last_activity_at: Millis,
    pub waited_ms: Millis,
    pub permission_ms: Millis,
    pub failed_turns: u32,
}

/// Un agent vivant dans Robiade.
#[derive(Debug, Clone)]
pub struct Agent {
    id: AgentId,
    project_path: String,
    status: AgentStatus,
    started_at: Millis,
    last_activity_at: Millis,
    waiting_since: Option<Millis>,
    waited_ms: Millis,
    permission_since: Option<Millis>,
    permission_ms: Millis,
    tool_calls: u32,
    failed_turns: u32,
    tools: BTreeMap<String, u32>,
    files_modified: BTreeSet<String>,
    last_tool: Option<String>,
    permission_mode: Option<PermissionMode>,
    effort: Option<Effort>,
    gauges: Gauges,
    assets: Assets,
    /// Session Claude Code en cours : celle du dernier hook reçu.
    session: String,
    /// Sessions Claude Code successives de l'agent : la première, puis une par `/clear` ou `/resume`.
    sessions: Vec<String>,
    name: Option<String>,
}

impl Agent {
    /// `resume` : la session reprise par `claude --resume`, `None` pour une session neuve.
    pub fn start(id: AgentId, project_path: String, resume: Option<String>, now: Millis) -> Self {
        let session = resume.unwrap_or_else(|| id.as_str().to_owned());
        Self {
            id,
            project_path,
            status: AgentStatus::Starting,
            started_at: now,
            last_activity_at: now,
            waiting_since: None,
            waited_ms: 0,
            permission_since: None,
            permission_ms: 0,
            tool_calls: 0,
            failed_turns: 0,
            tools: BTreeMap::new(),
            files_modified: BTreeSet::new(),
            last_tool: None,
            permission_mode: None,
            effort: None,
            gauges: Gauges::default(),
            assets: Assets::default(),
            sessions: vec![session.clone()],
            session,
            name: None,
        }
    }

    /// Onglet d'une exécution précédente de Robiade : son process n'existe plus, il se reprend.
    pub fn restore(id: AgentId, project_path: String, session: Option<String>, saved: Saved) -> Self {
        let mut agent = Self::start(id, project_path, session, saved.started_at);
        agent.status = AgentStatus::Ended;
        agent.last_activity_at = saved.last_activity_at;
        agent.name = saved.name;
        agent.waited_ms = saved.waited_ms;
        agent.permission_ms = saved.permission_ms;
        agent.failed_turns = saved.failed_turns;
        agent
    }

    /// Ce qu'il faut garder pour rouvrir l'onglet, attentes en cours comprises.
    pub fn saved(&self, now: Millis) -> Saved {
        Saved {
            name: self.name.clone(),
            started_at: self.started_at,
            last_activity_at: self.last_activity_at,
            waited_ms: self.waited_ms(now),
            permission_ms: self.permission_ms(now),
            failed_turns: self.failed_turns,
        }
    }

    /// Un nom vide efface le nom ; au-delà de la limite, il est tronqué.
    pub fn rename(&mut self, raw: &str) {
        let name: String = raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX_CHARS).collect();
        self.name = (!name.is_empty()).then_some(name);
    }

    /// Session Claude Code en cours : celle qu'une reprise ou un redémarrage relance. Au départ,
    /// celle qu'il reprend, sinon la sienne ; puis celle du dernier hook (`/clear`, `/resume`).
    pub fn session_id(&self) -> &str {
        &self.session
    }

    pub fn apply(&mut self, event: AgentEvent, now: Millis) {
        if let AgentEvent::ToolStarted { tool } = &event {
            self.tool_calls += 1;
            self.last_tool = Some(tool.clone());
            *self.tools.entry(tool.clone()).or_default() += 1;
        }
        if event == AgentEvent::TurnFailed && self.status != AgentStatus::Ended {
            self.failed_turns += 1;
        }
        let next = self.status.next(&event);
        self.track_waiting(next, now);
        self.status = next;
        self.last_activity_at = now;
    }

    /// Le mode peut changer en cours de session (`Shift+Tab`) : le dernier rapporté fait foi.
    pub fn observe_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode = Some(mode);
    }

    /// L'effort se change en cours de session (`/effort`) : le dernier rapporté fait foi.
    pub fn observe_effort(&mut self, effort: Effort) {
        self.effort = Some(effort);
    }

    /// Session rapportée par un hook : elle devient la session en cours. Nouvelle après un `/clear`,
    /// parfois déjà connue après un `/resume` dans le même process.
    pub fn observe_session(&mut self, session: &str) {
        if !self.sessions.iter().any(|s| s == session) {
            self.sessions.push(session.to_owned());
        }
        if self.session != session {
            session.clone_into(&mut self.session);
        }
    }

    /// La statusline est rafraîchie en continu : le dernier relevé remplace le précédent.
    pub fn observe_gauges(&mut self, gauges: Gauges) {
        self.gauges = gauges;
    }

    /// L'agent a écrit un fichier : il est compté, et rejoint les assets s'il en est un.
    /// `in_scratchpad` est son chemin résolu s'il est dans le scratchpad de l'agent : tout fichier
    /// y est un asset, gardé sous ce chemin pour que `/tmp/…` et `/private/tmp/…` n'en fassent qu'un.
    pub fn observe_file_written(&mut self, path: &str, in_scratchpad: Option<&str>, now: Millis) {
        self.files_modified.insert(path.to_owned());
        match in_scratchpad {
            Some(resolved) => self.assets.record_scratchpad(resolved, now),
            None => self.assets.record(path, now),
        }
    }

    /// L'agent a lu une image : elle rejoint les assets, sans compter comme fichier modifié.
    pub fn observe_image_viewed(&mut self, path: &str, now: Millis) {
        self.assets.record(path, now);
    }

    /// J'ai ouvert la session : un tour terminé n'est plus « non lu ».
    pub fn mark_seen(&mut self, now: Millis) {
        if self.status == AgentStatus::AwaitingInput {
            self.track_waiting(AgentStatus::Idle, now);
            self.status = AgentStatus::Idle;
        }
    }

    /// Temps total où l'agent m'a attendu, y compris l'attente en cours.
    pub fn waited_ms(&self, now: Millis) -> Millis {
        self.waited_ms + self.waiting_since.map_or(0, |since| (now - since).max(0))
    }

    /// Part de l'attente passée sur une demande de permission, en cours comprise.
    pub fn permission_ms(&self, now: Millis) -> Millis {
        self.permission_ms + self.permission_since.map_or(0, |since| (now - since).max(0))
    }

    fn track_waiting(&mut self, next: AgentStatus, now: Millis) {
        match (self.permission_since, next == AgentStatus::AwaitingPermission) {
            (None, true) => self.permission_since = Some(now),
            (Some(since), false) => {
                self.permission_ms += (now - since).max(0);
                self.permission_since = None;
            }
            _ => {}
        }
        match (self.waiting_since, next.is_waiting_on_me()) {
            (None, true) => self.waiting_since = Some(now),
            (Some(since), false) => {
                self.waited_ms += (now - since).max(0);
                self.waiting_since = None;
            }
            _ => {}
        }
    }

    pub fn id(&self) -> &AgentId {
        &self.id
    }
    pub fn project_path(&self) -> &str {
        &self.project_path
    }
    pub fn status(&self) -> AgentStatus {
        self.status
    }
    pub fn started_at(&self) -> Millis {
        self.started_at
    }
    pub fn last_activity_at(&self) -> Millis {
        self.last_activity_at
    }
    pub fn tool_calls(&self) -> u32 {
        self.tool_calls
    }
    /// Appels par outil, le plus fréquent en tête.
    pub fn tools(&self) -> Vec<(&str, u32)> {
        let mut tools: Vec<(&str, u32)> = self.tools.iter().map(|(t, n)| (t.as_str(), *n)).collect();
        tools.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        tools
    }
    /// Sous-agents lancés par l'outil `Agent` (anciennement `Task`).
    pub fn subagents(&self) -> u32 {
        ["Agent", "Task"].iter().filter_map(|t| self.tools.get(*t)).sum()
    }
    pub fn failed_turns(&self) -> u32 {
        self.failed_turns
    }
    pub fn files_modified(&self) -> usize {
        self.files_modified.len()
    }
    pub fn last_tool(&self) -> Option<&str> {
        self.last_tool.as_deref()
    }
    pub fn permission_mode(&self) -> Option<&PermissionMode> {
        self.permission_mode.as_ref()
    }
    pub fn effort(&self) -> Option<&Effort> {
        self.effort.as_ref()
    }
    pub fn gauges(&self) -> &Gauges {
        &self.gauges
    }
    pub fn assets(&self) -> &Assets {
        &self.assets
    }
    pub fn sessions(&self) -> &[String] {
        &self.sessions
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("journal des agents indisponible : {0}")]
pub struct JournalError(pub String);

/// Onglets d'agents gardés d'une exécution de Robiade à l'autre : on les retrouve après un redémarrage.
pub trait AgentJournal: Send + Sync {
    /// Onglets encore ouverts, actifs depuis `since`, le plus récent en tête, restaurés comme terminés.
    fn recent(&self, since: Millis) -> Result<Vec<Agent>, JournalError>;
    /// Tous les onglets actifs depuis `since`, fermés compris : la matière des statistiques.
    fn since(&self, since: Millis) -> Result<Vec<Agent>, JournalError>;
    fn save(&self, agent: &Agent, now: Millis) -> Result<(), JournalError>;
    /// Ferme l'onglet : il n'est plus rouvert, mais ses compteurs restent dans les statistiques.
    fn close(&self, id: &AgentId) -> Result<(), JournalError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Agent {
        let id = AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").unwrap();
        Agent::start(id, "/tmp/p".into(), None, 0)
    }

    #[test]
    fn rejects_anything_but_a_uuid() {
        assert!(AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").is_ok());
        assert_eq!(AgentId::parse("'; rm -rf / #"), Err(InvalidAgentId));
        assert_eq!(AgentId::parse("0b6f1c2e06b1d04a5e09c3302f1e8d7a4b10"), Err(InvalidAgentId));
    }

    #[test]
    fn permission_mode_is_a_short_identifier_and_only_auto_or_bypass_is_permissive() {
        assert!(PermissionMode::parse("bypassPermissions").unwrap().is_permissive());
        assert!(PermissionMode::parse("auto").unwrap().is_permissive());
        assert!(!PermissionMode::parse("acceptEdits").unwrap().is_permissive());
        assert_eq!(PermissionMode::parse(""), None);
        assert_eq!(PermissionMode::parse("<script>"), None);
    }

    #[test]
    fn effort_is_a_short_lowercase_identifier_and_the_last_one_wins() {
        assert_eq!(Effort::parse("xhigh").unwrap().as_str(), "xhigh");
        assert_eq!(Effort::parse(""), None);
        assert_eq!(Effort::parse("High"), None);
        assert_eq!(Effort::parse("<script>"), None);
        let mut agent = Agent::start(AgentId::parse("00000000-0000-4000-8000-000000000000").unwrap(), "/p".into(), None, 0);
        agent.observe_effort(Effort::parse("medium").unwrap());
        agent.observe_effort(Effort::parse("max").unwrap());
        assert_eq!(agent.effort().map(Effort::as_str), Some("max"));
    }

    #[test]
    fn keeps_every_session_of_the_agent_once() {
        let mut agent = agent();
        agent.observe_session("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10");
        agent.observe_session("9d259b93-5dc0-47c3-b1c8-3084e032dd44");
        agent.observe_session("9d259b93-5dc0-47c3-b1c8-3084e032dd44");
        assert_eq!(agent.sessions(), ["0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10", "9d259b93-5dc0-47c3-b1c8-3084e032dd44"]);

        let id = AgentId::parse("00000000-0000-4000-8000-000000000000").unwrap();
        let resumed = Agent::start(id, "/p".into(), Some("1b6f64dc-5a64-49af-aa54-67bf35b5afcc".into()), 0);
        assert_eq!(resumed.sessions(), ["1b6f64dc-5a64-49af-aa54-67bf35b5afcc"]);
    }

    #[test]
    fn the_session_in_progress_is_the_last_one_reported() {
        let first = "1b6f64dc-5a64-49af-aa54-67bf35b5afcc";
        let after_clear = "9d259b93-5dc0-47c3-b1c8-3084e032dd44";
        let id = AgentId::parse("00000000-0000-4000-8000-000000000000").unwrap();
        let mut agent = Agent::start(id, "/p".into(), Some(first.into()), 0);
        assert_eq!(agent.session_id(), first);

        agent.observe_session(after_clear);
        assert_eq!(agent.session_id(), after_clear);
        // `/resume` dans le même process : retour à une session déjà connue.
        agent.observe_session(first);
        assert_eq!(agent.session_id(), first);
        assert_eq!(agent.sessions(), [first, after_clear]);
    }

    #[test]
    fn levels_follow_the_statusline_thresholds() {
        assert_eq!(Level::of(49), Level::Comfortable);
        assert_eq!(Level::of(50), Level::Watch);
        assert_eq!(Level::of(80), Level::Critical);
    }

    #[test]
    fn follows_a_turn_with_a_permission_prompt() {
        let mut a = agent();
        a.apply(AgentEvent::SessionStarted, 1);
        a.apply(AgentEvent::PromptSubmitted, 2);
        a.apply(AgentEvent::ToolStarted { tool: "Bash".into() }, 3);
        a.apply(AgentEvent::PermissionRequested, 4);
        assert_eq!(a.status(), AgentStatus::AwaitingPermission);
        assert!(a.status().needs_attention());
        a.apply(AgentEvent::ToolFinished, 10);
        a.apply(AgentEvent::TurnFinished, 12);
        assert_eq!(a.status(), AgentStatus::AwaitingInput);
        a.mark_seen(20);
        assert_eq!(a.status(), AgentStatus::Idle);
        assert_eq!(a.waited_ms(100), (10 - 4) + (20 - 12));
        assert_eq!(a.permission_ms(100), 10 - 4);
        assert_eq!(a.tool_calls(), 1);
    }

    #[test]
    fn counts_tools_subagents_and_distinct_files() {
        let mut a = agent();
        for tool in ["Bash", "Agent", "Bash", "Edit"] {
            a.apply(AgentEvent::ToolStarted { tool: tool.into() }, 1);
        }
        a.observe_file_written("/p/a.rs", None, 2);
        a.observe_file_written("/p/a.rs", None, 3);
        a.observe_file_written("/p/b.md", None, 4);
        assert_eq!(a.tools(), [("Bash", 2), ("Agent", 1), ("Edit", 1)]);
        assert_eq!(a.subagents(), 1);
        assert_eq!(a.files_modified(), 2);
    }

    #[test]
    fn idle_notification_does_not_resurface_a_read_session() {
        let mut a = agent();
        a.apply(AgentEvent::TurnFinished, 1);
        a.mark_seen(2);
        a.apply(AgentEvent::WaitingForInput, 3);
        assert_eq!(a.status(), AgentStatus::Idle);
    }

    #[test]
    fn a_name_is_trimmed_bounded_and_erasable() {
        let mut a = agent();
        a.rename("  refonte   du login ");
        assert_eq!(a.name(), Some("refonte du login"));
        a.rename(&"x".repeat(100));
        assert_eq!(a.name().map(str::len), Some(NAME_MAX_CHARS));
        a.rename("   ");
        assert_eq!(a.name(), None);
    }

    #[test]
    fn a_restored_tab_is_ended_and_points_to_its_session() {
        let id = AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").unwrap();
        let saved =
            Saved { name: Some("login".into()), started_at: 1, last_activity_at: 9, waited_ms: 4, ..Saved::default() };
        let a = Agent::restore(id, "/p".into(), Some("s-1".into()), saved.clone());
        assert_eq!(a.status(), AgentStatus::Ended);
        assert_eq!(a.session_id(), "s-1");
        assert_eq!(a.saved(100), saved);
    }

    #[test]
    fn counts_failed_turns() {
        let mut a = agent();
        a.apply(AgentEvent::TurnFailed, 1);
        a.apply(AgentEvent::PromptSubmitted, 2);
        a.apply(AgentEvent::TurnFailed, 3);
        assert_eq!(a.failed_turns(), 2);
    }

    #[test]
    fn ended_is_terminal() {
        let mut a = agent();
        a.apply(AgentEvent::ProcessExited, 1);
        a.apply(AgentEvent::PromptSubmitted, 2);
        assert_eq!(a.status(), AgentStatus::Ended);
    }
}

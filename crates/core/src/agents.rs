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
    gauges: Gauges,
    assets: Assets,
    resumed_from: Option<String>,
    name: Option<String>,
}

impl Agent {
    pub fn start(id: AgentId, project_path: String, resumed_from: Option<String>, now: Millis) -> Self {
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
            gauges: Gauges::default(),
            assets: Assets::default(),
            resumed_from,
            name: None,
        }
    }

    /// Onglet d'une exécution précédente de Robiade : son process n'existe plus, il se reprend.
    pub fn restore(id: AgentId, project_path: String, resumed_from: Option<String>, saved: Saved) -> Self {
        let mut agent = Self::start(id, project_path, resumed_from, saved.started_at);
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

    /// Session Claude Code de l'agent : celle qu'il reprend, sinon la sienne.
    pub fn session_id(&self) -> &str {
        self.resumed_from.as_deref().unwrap_or(self.id.as_str())
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

    /// La statusline est rafraîchie en continu : le dernier relevé remplace le précédent.
    pub fn observe_gauges(&mut self, gauges: Gauges) {
        self.gauges = gauges;
    }

    /// L'agent a écrit un fichier : il est compté, et rejoint les assets s'il en est un.
    pub fn observe_file_written(&mut self, path: &str, now: Millis) {
        self.files_modified.insert(path.to_owned());
        self.assets.record(path, now);
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
    pub fn gauges(&self) -> &Gauges {
        &self.gauges
    }
    pub fn assets(&self) -> &Assets {
        &self.assets
    }
    pub fn resumed_from(&self) -> Option<&str> {
        self.resumed_from.as_deref()
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
        a.observe_file_written("/p/a.rs", 2);
        a.observe_file_written("/p/a.rs", 3);
        a.observe_file_written("/p/b.md", 4);
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

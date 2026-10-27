//! Agents : sessions Claude Code lancées par Robiade, et leur status.

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

    fn is_waiting_on_me(self) -> bool {
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
    tool_calls: u32,
    last_tool: Option<String>,
    permission_mode: Option<PermissionMode>,
    resumed_from: Option<String>,
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
            tool_calls: 0,
            last_tool: None,
            permission_mode: None,
            resumed_from,
        }
    }

    pub fn apply(&mut self, event: AgentEvent, now: Millis) {
        if let AgentEvent::ToolStarted { tool } = &event {
            self.tool_calls += 1;
            self.last_tool = Some(tool.clone());
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

    fn track_waiting(&mut self, next: AgentStatus, now: Millis) {
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
    pub fn last_tool(&self) -> Option<&str> {
        self.last_tool.as_deref()
    }
    pub fn permission_mode(&self) -> Option<&PermissionMode> {
        self.permission_mode.as_ref()
    }
    pub fn resumed_from(&self) -> Option<&str> {
        self.resumed_from.as_deref()
    }
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
        assert_eq!(a.tool_calls(), 1);
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
    fn ended_is_terminal() {
        let mut a = agent();
        a.apply(AgentEvent::ProcessExited, 1);
        a.apply(AgentEvent::PromptSubmitted, 2);
        assert_eq!(a.status(), AgentStatus::Ended);
    }
}

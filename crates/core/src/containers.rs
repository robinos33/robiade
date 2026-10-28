//! Conteneurs Docker, rattachés à leur projet par les labels de Docker Compose. Robiade les lit,
//! suit leurs logs, et les démarre, arrête ou redémarre à ma demande ; il n'en crée ni n'en
//! supprime aucun. Noms, images et logs viennent de Docker : données non fiables.

/// Projet Compose, et dossier d'où il a été lancé : c'est lui qui rattache le conteneur à un dépôt.
pub const COMPOSE_PROJECT: &str = "com.docker.compose.project";
pub const COMPOSE_WORKING_DIR: &str = "com.docker.compose.project.working_dir";
pub const COMPOSE_SERVICE: &str = "com.docker.compose.service";

/// Identifiant Docker, complet ou abrégé. Il finit dans un chemin d'URL de l'API : rien d'autre
/// que de l'hexadécimal n'y entre.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerId(String);

impl ContainerId {
    pub fn parse(raw: &str) -> Result<Self, ContainerError> {
        let valid = (12..=64).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_hexdigit());
        valid.then(|| Self(raw.to_ascii_lowercase())).ok_or(ContainerError::InvalidId)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Même conteneur, que l'un des deux identifiants soit abrégé ou non.
    pub fn matches(&self, other: &ContainerId) -> bool {
        self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Dead,
    /// État transitoire ou inconnu (`removing`, `stopping`…) : aucune action proposée.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port {
    pub private: u16,
    /// Port publié sur l'hôte ; `None` s'il n'est exposé qu'au réseau Docker.
    pub public: Option<u16>,
    pub protocol: String,
}

impl Port {
    /// Docker liste une fois par adresse (IPv4, IPv6) le même port publié : un seul suffit.
    pub fn distinct(mut ports: Vec<Port>) -> Vec<Port> {
        ports.sort();
        ports.dedup();
        ports
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub id: ContainerId,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    /// Résumé de Docker (« Up 3 hours », « Exited (0) 2 days ago »).
    pub status: String,
    pub ports: Vec<Port>,
    pub compose_project: Option<String>,
    pub service: Option<String>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    /// Raison de refuser l'action dans cet état ; `None` si elle a un sens.
    pub fn refused_in(self, state: ContainerState) -> Option<&'static str> {
        use ContainerState::*;
        let allowed = match self {
            Self::Start => matches!(state, Created | Exited),
            Self::Stop => matches!(state, Running | Paused | Restarting),
            Self::Restart => matches!(state, Running),
        };
        (!allowed).then_some(match self {
            Self::Start => "le conteneur n'est pas arrêté",
            Self::Stop | Self::Restart => "le conteneur ne tourne pas",
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContainerError {
    #[error("Docker ne répond pas : démarre Docker Desktop ou OrbStack")]
    Unavailable,
    #[error("identifiant de conteneur invalide")]
    InvalidId,
    #[error("conteneur introuvable")]
    NotFound,
    #[error("Docker a refusé : {0}")]
    Docker(String),
}

/// Reçoit les logs au fil de l'eau, octets bruts ; `None` quand le flux se termine.
pub type LogSink = Box<dyn FnMut(Option<&[u8]>) + Send>;
/// Arrête un suivi de logs.
pub type StopLogs = Box<dyn FnOnce() + Send>;

/// Le démon Docker local.
pub trait Containers: Send + Sync {
    /// Tous les conteneurs, arrêtés compris.
    fn list(&self) -> Result<Vec<Container>, ContainerError>;
    fn run(&self, id: &ContainerId, action: ContainerAction) -> Result<(), ContainerError>;
    /// Suit les logs depuis les `tail` dernières lignes, sans bloquer l'appelant.
    fn follow_logs(&self, id: &ContainerId, tail: u32, sink: LogSink) -> Result<StopLogs, ContainerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_id_is_hexadecimal_and_nothing_else() {
        assert!(ContainerId::parse("3f2a9c1b7d4e").is_ok());
        assert_eq!(ContainerId::parse(&"A".repeat(64)).unwrap().as_str(), "a".repeat(64));
        assert_eq!(ContainerId::parse("3f2a9c1b7d4"), Err(ContainerError::InvalidId));
        assert_eq!(ContainerId::parse("../../images/json"), Err(ContainerError::InvalidId));
        assert_eq!(ContainerId::parse("boutique-postgres"), Err(ContainerError::InvalidId));
        assert_eq!(ContainerId::parse(&"a".repeat(65)), Err(ContainerError::InvalidId));
    }

    #[test]
    fn a_short_id_designates_the_same_container() {
        let full = ContainerId::parse(&format!("3f2a9c1b7d4e{}", "0".repeat(52))).unwrap();
        assert!(full.matches(&ContainerId::parse("3f2a9c1b7d4e").unwrap()));
        assert!(!full.matches(&ContainerId::parse("3f2a9c1b7d4f").unwrap()));
    }

    #[test]
    fn an_action_only_applies_to_a_container_in_the_right_state() {
        use ContainerAction::*;
        use ContainerState::*;
        assert_eq!(Start.refused_in(Exited), None);
        assert!(Start.refused_in(Running).is_some());
        assert_eq!(Stop.refused_in(Running), None);
        assert!(Stop.refused_in(Exited).is_some());
        assert_eq!(Restart.refused_in(Running), None);
        assert!(Restart.refused_in(Created).is_some());
        assert!(Stop.refused_in(Other).is_some());
        assert_eq!(ContainerAction::parse("kill"), None);
    }

    #[test]
    fn a_port_published_on_ipv4_and_ipv6_is_listed_once() {
        let port = |public| Port { private: 5432, public, protocol: "tcp".into() };
        let ports = Port::distinct(vec![port(Some(8032)), port(None), port(Some(8032))]);
        assert_eq!(ports, vec![port(None), port(Some(8032))]);
    }
}

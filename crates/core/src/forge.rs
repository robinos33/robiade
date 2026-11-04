//! Forge : MR et issues GitLab, lues sur l'instance réglée. Couche anti-corruption : le domaine ne
//! garde que ce qu'il affiche ou ce sur quoi il raisonne, GitLab reste la source.

use crate::contributions::Contribution;
use crate::issue_actions::{IssuePlanned, NewIssue};
use crate::merge_actions::Planned;
use crate::Millis;

/// Une MR désignée sans ambiguïté : chemin complet du projet et numéro dans ce projet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MrRef {
    pub project: String,
    pub iid: u64,
}

impl MrRef {
    /// Référence désignée par ses parties (front, base locale) : `None` si le chemin n'en est pas un.
    pub fn new(project: &str, iid: u64) -> Option<Self> {
        is_project_path(project).then(|| Self { project: project.to_owned(), iid })
    }
}

/// Une issue désignée sans ambiguïté.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueRef {
    pub project: String,
    pub iid: u64,
}

impl IssueRef {
    /// Référence désignée par ses parties (front, base locale) : `None` si le chemin n'en est pas un.
    pub fn new(project: &str, iid: u64) -> Option<Self> {
        is_project_path(project).then(|| Self { project: project.to_owned(), iid })
    }

    /// Référence saisie à la main, comme la forge l'écrit : son URL, `groupe/projet#12`, ou `#12`
    /// dans `default_project` (le projet de forge du contexte, s'il y en a un).
    pub fn parse(raw: &str, default_project: Option<&str>, urls: &dyn ForgeUrls) -> Option<Self> {
        let raw = raw.trim();
        if raw.starts_with("https://") {
            return urls.issue(raw);
        }
        let (project, iid) = raw.split_once('#')?;
        let project = if project.is_empty() { default_project? } else { project };
        if iid.is_empty() || !iid.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Self::new(project, iid.parse().ok()?)
    }
}

/// Forme des adresses de la forge : URL web d'une MR ou d'une issue, remote Git. Le cœur ne la
/// connaît pas ; l'adaptateur de la forge réglée la porte. Pur : aucun appel réseau.
pub trait ForgeUrls: Send + Sync {
    /// URL web d'une MR de l'instance ; `None` pour tout autre hôte ou forme.
    fn merge_request(&self, url: &str) -> Option<MrRef>;
    /// URL web d'une issue de l'instance ; `None` pour tout autre hôte ou forme.
    fn issue(&self, url: &str) -> Option<IssueRef>;
    /// URL web d'une issue ; `None` sans forge réglée.
    fn issue_url(&self, issue: &IssueRef) -> Option<String>;
    /// Projet d'une URL de remote Git, s'il est sur l'instance.
    fn project_of_remote(&self, url: &str) -> Option<String>;
    /// URL web de l'instance, en `https`, sans identifiants : la seule que Robiade ouvre dans le navigateur.
    fn is_web_url(&self, url: &str) -> bool;
}

/// Aucune forge réglée : aucune adresse n'est reconnue, et tout appel est refusé.
pub struct NoForge;

impl ForgeUrls for NoForge {
    fn merge_request(&self, _: &str) -> Option<MrRef> {
        None
    }
    fn issue(&self, _: &str) -> Option<IssueRef> {
        None
    }
    fn issue_url(&self, _: &IssueRef) -> Option<String> {
        None
    }
    fn project_of_remote(&self, _: &str) -> Option<String> {
        None
    }
    fn is_web_url(&self, _: &str) -> bool {
        false
    }
}

impl Forge for NoForge {
    fn me(&self) -> Result<String, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn merge_request(&self, _: &MrRef) -> Result<MergeRequest, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn my_issues(&self) -> Result<Vec<Issue>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn approvals(&self, _: &MrRef) -> Result<Approvals, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn merge_request_discussions(&self, _: &MrRef) -> Result<Vec<Discussion>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn issue(&self, _: &IssueRef) -> Result<Issue, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn issue_discussions(&self, _: &IssueRef) -> Result<Vec<Discussion>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn project_issues(&self, _: &str) -> Result<Vec<Issue>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn execute(&self, _: &MrRef, _: &Planned) -> Result<(), ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn execute_on_issue(&self, _: &IssueRef, _: &IssuePlanned) -> Result<(), ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn create_issue(&self, _: &NewIssue) -> Result<IssueRef, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
    fn my_contributions(&self, _: Millis, _: Millis) -> Result<Vec<Contribution>, ForgeError> {
        Err(ForgeError::NotConfigured)
    }
}

/// Une tâche d'équipe : GitLab en est la source, Robiade la montre sans rien en garder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub issue: IssueRef,
    pub title: String,
    pub state: IssueState,
    /// Markdown de GitLab, non fiable.
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub web_url: String,
    pub updated_at: Millis,
    /// Échéance `AAAA-MM-JJ`, telle que GitLab la donne.
    pub due_date: Option<String>,
    /// MR liées à l'issue.
    pub merge_requests: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Opened,
    Closed,
}

/// Chemin complet d'un projet GitLab (`groupe/sous-groupe/projet`) : segments non vides, sans
/// `.` ni `..`, et aucun caractère qu'il faudrait encoder dans une URL.
pub fn is_project_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
}

/// Nom d'utilisateur GitLab : de quoi le placer dans une URL ou une requête sans l'encoder.
pub fn is_username(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MrState {
    Draft,
    Opened,
    Merged,
    Closed,
}

/// Dernier pipeline d'une MR, ramené à ce que l'interface distingue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStatus {
    /// En file, en préparation ou en cours.
    Running,
    Success,
    Failed,
    /// Bloqué sur un job manuel.
    Manual,
    /// Annulé ou sauté.
    Canceled,
}

impl PipelineStatus {
    /// Statut GitLab (`ci_status`) ; `None` pour une valeur inconnue plutôt qu'une supposition.
    pub fn parse(status: &str) -> Option<Self> {
        Some(match status {
            "created" | "waiting_for_resource" | "preparing" | "waiting_for_callback" | "pending" | "running"
            | "scheduled" => Self::Running,
            "success" => Self::Success,
            "failed" => Self::Failed,
            "manual" => Self::Manual,
            "canceled" | "canceling" | "skipped" => Self::Canceled,
            _ => return None,
        })
    }
}

/// Approbations d'une MR : combien en manquent, et qui a déjà approuvé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approvals {
    pub required: u32,
    pub left: u32,
    pub by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub mr: MrRef,
    pub title: String,
    /// Markdown de GitLab, non fiable.
    pub description: String,
    pub author: String,
    pub state: MrState,
    pub source_branch: String,
    pub target_branch: String,
    pub web_url: String,
    pub updated_at: Millis,
    /// Commit de tête : un merge ne part que sur le commit que j'ai vu.
    pub sha: String,
    pub has_conflicts: bool,
    /// Des discussions bloquantes ne sont pas résolues.
    pub unresolved_discussions: bool,
    /// `detailed_merge_status` de GitLab (`mergeable`, `not_approved`, `ci_must_pass`…), tel quel.
    pub merge_status: String,
    /// `None` sans pipeline, ou quand il n'a pas été lu (MR des autres).
    pub pipeline: Option<PipelineStatus>,
    /// Identifiant du dernier pipeline, pour le relancer.
    pub pipeline_id: Option<u64>,
    /// `None` quand elles n'ont pas été lues (MR des autres).
    pub approvals: Option<Approvals>,
}

impl MergeRequest {
    /// Wagon intermédiaire d'un train de MR : il ne cible ni `staging` ni `master` (ni `main`, tronc
    /// des dépôts récents), et ne doit donc pas être squashé : les wagons en aval portent ses commits
    /// par ancestralité.
    pub fn is_intermediate_wagon(&self) -> bool {
        !matches!(self.target_branch.as_str(), "staging" | "master" | "main")
    }
}

/// Un message d'un fil d'échanges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub author: String,
    /// Markdown de GitLab, non fiable.
    pub body: String,
    pub created_at: Millis,
    /// Note écrite par GitLab lui-même (« a changé le label », « a ajouté 1 commit »).
    pub system: bool,
}

/// Un fil d'échanges sur une issue ou une MR : un commentaire et ses réponses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discussion {
    pub notes: Vec<Note>,
    /// `None` pour un fil qui ne se résout pas (commentaire d'issue, note simple).
    pub resolved: Option<bool>,
}

/// Les fils tels qu'on les lit : sans les notes système, et sans les fils qu'elles laissent vides.
pub fn readable(discussions: impl IntoIterator<Item = Discussion>) -> Vec<Discussion> {
    discussions
        .into_iter()
        .filter_map(|mut d| {
            d.notes.retain(|n| !n.system);
            (!d.notes.is_empty()).then_some(d)
        })
        .collect()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ForgeError {
    #[error("aucune forge réglée : renseigne-la dans Réglages")]
    NotConfigured,
    #[error("aucun token GitLab dans le trousseau (service {0})")]
    NoToken(String),
    #[error("token GitLab refusé")]
    Unauthorized,
    #[error("MR introuvable")]
    NotFound,
    #[error("GitLab injoignable : {0}")]
    Network(String),
    #[error("réponse GitLab inattendue : {0}")]
    Api(String),
}

/// MR et issues sur GitLab : lecture, et exécution des actions déjà validées par `merge_actions::plan`
/// et `issue_actions::plan`.
pub trait Forge: Send + Sync {
    /// Mon nom d'utilisateur GitLab.
    fn me(&self) -> Result<String, ForgeError>;
    /// Mes MR ouvertes, avec leur dernier pipeline et leurs approbations.
    fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError>;
    /// MR ouvertes des autres où je suis relecteur, brouillons et MR déjà approuvées par moi exclus.
    fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError>;
    /// Une MR précise, quel que soit son état.
    fn merge_request(&self, mr: &MrRef) -> Result<MergeRequest, ForgeError>;
    /// Issues ouvertes qui me sont assignées, tous projets confondus.
    fn my_issues(&self) -> Result<Vec<Issue>, ForgeError>;
    /// Approbations d'une MR.
    fn approvals(&self, mr: &MrRef) -> Result<Approvals, ForgeError>;
    /// Fils d'échanges d'une MR, dans l'ordre chronologique, sans les notes système.
    fn merge_request_discussions(&self, mr: &MrRef) -> Result<Vec<Discussion>, ForgeError>;
    /// Une issue précise, avec sa description.
    fn issue(&self, issue: &IssueRef) -> Result<Issue, ForgeError>;
    /// Fils d'échanges d'une issue, dans l'ordre chronologique, sans les notes système.
    fn issue_discussions(&self, issue: &IssueRef) -> Result<Vec<Discussion>, ForgeError>;
    /// Issues ouvertes d'un projet (chemin complet, validé par `is_project_path`).
    fn project_issues(&self, project: &str) -> Result<Vec<Issue>, ForgeError>;
    /// Envoie une action planifiée sur une MR.
    fn execute(&self, mr: &MrRef, planned: &Planned) -> Result<(), ForgeError>;
    /// Envoie une action planifiée sur une issue.
    fn execute_on_issue(&self, issue: &IssueRef, planned: &IssuePlanned) -> Result<(), ForgeError>;
    /// Crée une issue validée par `NewIssue::new` ; renvoie la référence attribuée par GitLab.
    fn create_issue(&self, new: &NewIssue) -> Result<IssueRef, ForgeError>;
    /// Mes contributions (voir `contributions`) dont l'événement date de `[from, to)`.
    fn my_contributions(&self, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError>;
}

/// MR produites par des sessions : combien, et combien ont été mergées.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MrOutcomes {
    pub total: usize,
    pub merged: usize,
    pub opened: usize,
    pub closed: usize,
    /// MR qu'on n'a pas pu lire (supprimée, sans droit, réseau).
    pub unknown: usize,
}

impl MrOutcomes {
    pub fn of<'a>(states: impl IntoIterator<Item = Option<&'a MrState>>) -> Self {
        let mut out = Self::default();
        for state in states {
            out.total += 1;
            match state {
                Some(MrState::Merged) => out.merged += 1,
                Some(MrState::Opened | MrState::Draft) => out.opened += 1,
                Some(MrState::Closed) => out.closed += 1,
                None => out.unknown += 1,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_is_built_only_on_a_project_path() {
        assert_eq!(MrRef::new("a/b-c", 3), Some(MrRef { project: "a/b-c".into(), iid: 3 }));
        assert_eq!(IssueRef::new("a/b", 4), Some(IssueRef { project: "a/b".into(), iid: 4 }));
        for bad in ["", "a/../b", "a//b", "a b", "a%2Fb"] {
            assert_eq!(MrRef::new(bad, 1), None, "{bad}");
            assert_eq!(IssueRef::new(bad, 1), None, "{bad}");
        }
    }

    /// Reconnaît une seule URL d'issue, pour vérifier que le cœur lui délègue la forme URL.
    struct OneIssue;

    impl ForgeUrls for OneIssue {
        fn merge_request(&self, _: &str) -> Option<MrRef> {
            None
        }
        fn issue(&self, url: &str) -> Option<IssueRef> {
            (url == "https://forge/p/12").then(|| IssueRef { project: "shop/boutique".into(), iid: 12 })
        }
        fn issue_url(&self, _: &IssueRef) -> Option<String> {
            None
        }
        fn project_of_remote(&self, _: &str) -> Option<String> {
            None
        }
        fn is_web_url(&self, _: &str) -> bool {
            false
        }
    }

    #[test]
    fn an_issue_is_typed_as_the_forge_writes_it() {
        let expected = Some(IssueRef { project: "shop/boutique".into(), iid: 12 });
        assert_eq!(IssueRef::parse(" #12 ", Some("shop/boutique"), &OneIssue), expected);
        assert_eq!(IssueRef::parse("shop/boutique#12", Some("autre/projet"), &OneIssue), expected);
        assert_eq!(IssueRef::parse("shop/boutique#12", None, &OneIssue), expected);
        assert_eq!(IssueRef::parse("https://forge/p/12", None, &OneIssue), expected);
        for bad in ["#12", "12", "a/../b#1", "a b#1", "a/b#", "a/b#1x", "https://forge/p/13"] {
            assert_eq!(IssueRef::parse(bad, None, &OneIssue), None, "{bad}");
        }
    }

    #[test]
    fn a_wagon_targets_neither_staging_nor_master() {
        let mut mr = MergeRequest {
            mr: MrRef { project: "p".into(), iid: 1 },
            title: String::new(),
            description: String::new(),
            author: String::new(),
            state: MrState::Opened,
            source_branch: "ticket-2".into(),
            target_branch: "ticket-1".into(),
            web_url: String::new(),
            updated_at: 0,
            sha: String::new(),
            has_conflicts: false,
            unresolved_discussions: false,
            merge_status: String::new(),
            pipeline: None,
            pipeline_id: None,
            approvals: None,
        };
        assert!(mr.is_intermediate_wagon());
        mr.target_branch = "staging".into();
        assert!(!mr.is_intermediate_wagon());
    }

    #[test]
    fn a_thread_is_read_without_the_notes_written_by_gitlab() {
        let note = |author: &str, system| Note { author: author.into(), body: String::new(), created_at: 0, system };
        let discussions = vec![
            Discussion { notes: vec![note("gitlab", true)], resolved: None },
            Discussion { notes: vec![note("a", false), note("gitlab", true), note("b", false)], resolved: Some(false) },
        ];
        let read = readable(discussions);
        assert_eq!(read.len(), 1);
        let authors: Vec<_> = read[0].notes.iter().map(|n| n.author.as_str()).collect();
        assert_eq!((authors, read[0].resolved), (vec!["a", "b"], Some(false)));
    }

    #[test]
    fn counts_merged_merge_requests_apart_from_unknown_ones() {
        let o = MrOutcomes::of([Some(&MrState::Merged), Some(&MrState::Draft), None, Some(&MrState::Merged)]);
        assert_eq!(o, MrOutcomes { total: 4, merged: 2, opened: 1, closed: 0, unknown: 1 });
    }
}

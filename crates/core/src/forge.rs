//! Forge : MR et issues GitLab, lues sur l'instance configurée. Couche anti-corruption : le domaine ne
//! garde que ce qu'il affiche ou ce sur quoi il raisonne, GitLab reste la source.

use crate::merge_actions::Planned;
use crate::Millis;

/// Seule instance GitLab connue de Robiade : tout autre hôte est ignoré.
pub const GITLAB_HOST: &str = "gitlab.example.com";

/// Une MR désignée sans ambiguïté : chemin complet du projet et numéro dans ce projet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MrRef {
    pub project: String,
    pub iid: u64,
}

impl MrRef {
    /// `https://gitlab.example.com/<projet>/-/merge_requests/<iid>`, tel que Claude Code
    /// l'écrit dans le `pr-link` d'un transcript. Tout autre hôte ou forme renvoie `None`.
    pub fn from_url(url: &str) -> Option<Self> {
        let (project, iid) = parse_item_url(url, &["merge_requests"])?;
        Some(Self { project, iid })
    }
}

/// `https://<instance>/<projet>/-/<genre>/<iid>[/…]` → projet et numéro, pour l'un des genres donnés.
fn parse_item_url(url: &str, kinds: &[&str]) -> Option<(String, u64)> {
    let rest = url.strip_prefix("https://")?.strip_prefix(GITLAB_HOST)?.strip_prefix('/')?;
    let (project, tail) = rest.split_once("/-/")?;
    let (kind, tail) = tail.split_once('/')?;
    let iid = tail.split(['/', '?', '#']).next()?;
    if !kinds.contains(&kind) || !is_project_path(project) || iid.is_empty() || !iid.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((project.to_owned(), iid.parse().ok()?))
}

/// Une issue désignée sans ambiguïté. GitLab 19 la publie en `/-/work_items/<iid>`, les liens plus
/// anciens en `/-/issues/<iid>` : les deux désignent la même issue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueRef {
    pub project: String,
    pub iid: u64,
}

impl IssueRef {
    pub fn from_url(url: &str) -> Option<Self> {
        let (project, iid) = parse_item_url(url, &["issues", "work_items"])?;
        Some(Self { project, iid })
    }
}

/// Une tâche d'équipe : GitLab en est la source, Robiade la montre sans rien en garder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub issue: IssueRef,
    pub title: String,
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

/// Chemin complet d'un projet GitLab (`groupe/sous-groupe/projet`) : segments non vides, sans
/// `.` ni `..`, et aucun caractère qu'il faudrait encoder dans une URL.
pub fn is_project_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ForgeError {
    #[error("aucun token GitLab dans le trousseau (service gitlab-api-token)")]
    NoToken,
    #[error("token GitLab refusé")]
    Unauthorized,
    #[error("MR introuvable")]
    NotFound,
    #[error("GitLab injoignable : {0}")]
    Network(String),
    #[error("réponse GitLab inattendue : {0}")]
    Api(String),
}

/// MR et issues sur GitLab : lecture, et exécution des actions déjà validées par `merge_actions::plan`.
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
    /// Une issue précise, avec sa description.
    fn issue(&self, issue: &IssueRef) -> Result<Issue, ForgeError>;
    /// Issues ouvertes d'un projet (chemin complet, validé par `is_project_path`).
    fn project_issues(&self, project: &str) -> Result<Vec<Issue>, ForgeError>;
    /// Envoie une action planifiée ; seul appel qui écrit sur GitLab.
    fn execute(&self, mr: &MrRef, planned: &Planned) -> Result<(), ForgeError>;
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
    fn only_merge_requests_of_the_configured_instance_are_recognised() {
        let mr = MrRef::from_url("https://gitlab.example.com/a/b-c/d_e/-/merge_requests/42");
        assert_eq!(mr, Some(MrRef { project: "a/b-c/d_e".into(), iid: 42 }));
        let with_tail = MrRef::from_url("https://gitlab.example.com/a/b/-/merge_requests/7/diffs#x");
        assert_eq!(with_tail.map(|m| m.iid), Some(7));
        assert_eq!(MrRef::from_url("https://gitlab.com/a/b/-/merge_requests/1"), None);
        assert_eq!(MrRef::from_url("https://gitlab.example.com.evil.io/a/-/merge_requests/1"), None);
        assert_eq!(MrRef::from_url("https://gitlab.example.com/a/../b/-/merge_requests/1"), None);
        assert_eq!(MrRef::from_url("https://gitlab.example.com/a%2Fb/-/merge_requests/1"), None);
        assert_eq!(MrRef::from_url("https://gitlab.example.com/a/b/-/merge_requests/1x"), None);
        assert_eq!(MrRef::from_url("https://gitlab.example.com/a/b/-/issues/1"), None);
    }

    #[test]
    fn an_issue_is_recognised_under_both_of_its_urls() {
        let expected = Some(IssueRef { project: "shop/boutique".into(), iid: 5021 });
        let base = "https://gitlab.example.com/shop/boutique/-";
        assert_eq!(IssueRef::from_url(&format!("{base}/work_items/5021")), expected);
        assert_eq!(IssueRef::from_url(&format!("{base}/issues/5021")), expected);
        assert_eq!(IssueRef::from_url(&format!("{base}/merge_requests/1")), None);
    }

    #[test]
    fn a_wagon_targets_neither_staging_nor_master() {
        let mut mr = MergeRequest {
            mr: MrRef { project: "p".into(), iid: 1 },
            title: String::new(),
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
    fn counts_merged_merge_requests_apart_from_unknown_ones() {
        let o = MrOutcomes::of([Some(&MrState::Merged), Some(&MrState::Draft), None, Some(&MrState::Merged)]);
        assert_eq!(o, MrOutcomes { total: 4, merged: 2, opened: 1, closed: 0, unknown: 1 });
    }
}

//! Actions sur une MR. Les règles de domaine sont portées ici, pas laissées à l'utilisateur : une
//! action interdite n'est jamais proposée à GitLab, et ce qui part est décrit avant d'être confirmé.

use crate::forge::{MergeRequest, MrRef, MrState, PipelineStatus};
use crate::Millis;
use crate::trunks::TrunkBranches;

/// Un commentaire plus long n'a pas sa place dans une confirmation relue en un coup d'œil.
pub const COMMENT_MAX_CHARS: usize = 10_000;

/// Ce que je demande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrAction {
    Comment(String),
    Approve,
    Rebase,
    RetryPipeline,
    MarkReady,
    MarkDraft,
    /// `expected_sha` : commit de tête vu au moment de confirmer.
    Merge { expected_sha: String },
}

/// Ce qui part vers GitLab, une fois les règles appliquées.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    Comment(String),
    Approve,
    Rebase,
    RetryPipeline { pipeline_id: u64 },
    Retitle(String),
    /// Merge refusé par GitLab si la tête a bougé depuis `sha`.
    Merge { sha: String, squash: bool },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Refused {
    #[error("action réservée à mes MR")]
    NotMine,
    #[error("on n'approuve pas sa propre MR")]
    OwnMergeRequest,
    #[error("la MR n'est plus ouverte")]
    NotOpen,
    #[error("commentaire vide ou trop long")]
    InvalidComment,
    #[error("aucun pipeline en échec à relancer")]
    NothingToRetry,
    #[error("la MR est déjà dans cet état")]
    AlreadyThere,
    #[error("la MR n'est pas mergeable ({0})")]
    NotMergeable(String),
    #[error("la MR a changé depuis l'affichage : relis-la avant de merger")]
    Stale,
}

/// Applique les règles à l'état **courant** de la MR, relu juste avant d'agir.
pub fn plan(mr: &MergeRequest, action: &MrAction, me: &str, trunks: &TrunkBranches) -> Result<Planned, Refused> {
    let mine = mr.author == me;
    let open = matches!(mr.state, MrState::Opened | MrState::Draft);
    if !open {
        return Err(Refused::NotOpen);
    }
    match action {
        MrAction::Comment(body) => {
            let body = body.trim();
            if body.is_empty() || body.chars().count() > COMMENT_MAX_CHARS {
                return Err(Refused::InvalidComment);
            }
            Ok(Planned::Comment(body.to_owned()))
        }
        MrAction::Approve if mine => Err(Refused::OwnMergeRequest),
        MrAction::Approve => Ok(Planned::Approve),
        _ if !mine => Err(Refused::NotMine),
        MrAction::Rebase => Ok(Planned::Rebase),
        MrAction::RetryPipeline => match (mr.pipeline, mr.pipeline_id) {
            (Some(PipelineStatus::Failed | PipelineStatus::Canceled), Some(pipeline_id)) => {
                Ok(Planned::RetryPipeline { pipeline_id })
            }
            _ => Err(Refused::NothingToRetry),
        },
        MrAction::MarkReady if mr.state == MrState::Draft => Ok(Planned::Retitle(ready_title(&mr.title))),
        MrAction::MarkDraft if mr.state == MrState::Opened => Ok(Planned::Retitle(format!("Draft: {}", mr.title))),
        MrAction::MarkReady | MrAction::MarkDraft => Err(Refused::AlreadyThere),
        MrAction::Merge { expected_sha } => {
            if expected_sha.is_empty() || *expected_sha != mr.sha {
                return Err(Refused::Stale);
            }
            if mr.state == MrState::Draft || mr.merge_status != "mergeable" {
                return Err(Refused::NotMergeable(mr.merge_status.clone()));
            }
            // Un wagon garde ses commits : les wagons en aval en descendent par ancestralité.
            Ok(Planned::Merge { sha: mr.sha.clone(), squash: !mr.is_intermediate_wagon(trunks) })
        }
    }
}

/// Titre sans les marqueurs de brouillon que GitLab reconnaît en tête.
fn ready_title(title: &str) -> String {
    let mut t = title.trim_start();
    loop {
        let lower = t.to_ascii_lowercase();
        let marker = ["draft:", "[draft]", "(draft)", "wip:", "[wip]"].iter().find(|m| lower.starts_with(*m));
        match marker {
            Some(m) => t = t[m.len()..].trim_start(),
            None => return t.to_owned(),
        }
    }
}

impl Planned {
    /// Description lisible, pour la confirmation et le journal d'audit.
    pub fn describe(&self) -> String {
        match self {
            Planned::Comment(body) => format!("commenter ({} caractères)", body.chars().count()),
            Planned::Approve => "approuver".into(),
            Planned::Rebase => "rebaser sur la branche cible".into(),
            Planned::RetryPipeline { pipeline_id } => format!("relancer les jobs en échec du pipeline #{pipeline_id}"),
            Planned::Retitle(title) => format!("renommer en « {title} »"),
            Planned::Merge { sha, squash: true } => format!("merger {} avec squash", &sha[..sha.len().min(8)]),
            Planned::Merge { sha, squash: false } => format!("merger {} sans squash (wagon)", &sha[..sha.len().min(8)]),
        }
    }
}

/// Une action sortante, telle que le journal d'audit la garde : jamais de secret ni de contenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub at: Millis,
    pub target: String,
    pub action: String,
    /// `ok`, ou le message d'erreur.
    pub outcome: String,
}

impl AuditEntry {
    pub fn new(at: Millis, mr: &MrRef, planned: &Planned, outcome: Result<(), String>) -> Self {
        Self {
            at,
            target: format!("{}!{}", mr.project, mr.iid),
            action: planned.describe(),
            outcome: outcome.err().unwrap_or_else(|| "ok".into()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("journal d'audit inaccessible : {0}")]
pub struct AuditError(pub String);

/// Journal local des actions sortantes.
pub trait AuditLog: Send + Sync {
    fn record(&self, entry: &AuditEntry) -> Result<(), AuditError>;
    fn recent(&self, limit: usize) -> Result<Vec<AuditEntry>, AuditError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mr(author: &str, target: &str) -> MergeRequest {
        MergeRequest {
            mr: MrRef { project: "a/b".into(), iid: 1 },
            title: "Draft: Ajoute le cache".into(),
            description: String::new(),
            author: author.into(),
            state: MrState::Opened,
            source_branch: "cache".into(),
            target_branch: target.into(),
            web_url: String::new(),
            updated_at: 0,
            sha: "0123456789abcdef".into(),
            has_conflicts: false,
            unresolved_discussions: false,
            merge_status: "mergeable".into(),
            pipeline: Some(PipelineStatus::Failed),
            pipeline_id: Some(42),
            approvals: None,
        }
    }

    fn trunks() -> TrunkBranches {
        TrunkBranches::new(["staging", "master"]).unwrap()
    }

    fn merge(sha: &str) -> MrAction {
        MrAction::Merge { expected_sha: sha.into() }
    }

    #[test]
    fn a_wagon_is_merged_without_squash_and_a_trunk_merge_with_it() {
        let sha = "0123456789abcdef";
        let merged = |target| plan(&mr("moi", target), &merge(sha), "moi", &trunks());
        assert_eq!(merged("ticket-1"), Ok(Planned::Merge { sha: sha.into(), squash: false }));
        assert_eq!(merged("staging"), Ok(Planned::Merge { sha: sha.into(), squash: true }));
        let main_only = TrunkBranches::new(["main"]).unwrap();
        let into_staging = plan(&mr("moi", "staging"), &merge(sha), "moi", &main_only);
        assert_eq!(into_staging, Ok(Planned::Merge { sha: sha.into(), squash: false }), "staging n'y est qu'une branche");
    }

    #[test]
    fn a_merge_only_leaves_on_the_commit_that_was_seen() {
        assert_eq!(plan(&mr("moi", "staging"), &merge("autre"), "moi", &trunks()), Err(Refused::Stale));
        assert_eq!(plan(&mr("moi", "staging"), &merge(""), "moi", &trunks()), Err(Refused::Stale));
        let mut blocked = mr("moi", "staging");
        blocked.merge_status = "not_approved".into();
        let refused = plan(&blocked, &merge("0123456789abcdef"), "moi", &trunks());
        assert_eq!(refused, Err(Refused::NotMergeable("not_approved".into())));
    }

    #[test]
    fn only_my_merge_requests_can_be_changed_and_only_the_others_approved() {
        let theirs = mr("collegue", "staging");
        assert_eq!(plan(&theirs, &MrAction::Rebase, "moi", &trunks()), Err(Refused::NotMine));
        assert_eq!(plan(&theirs, &merge("0123456789abcdef"), "moi", &trunks()), Err(Refused::NotMine));
        assert_eq!(plan(&theirs, &MrAction::Approve, "moi", &trunks()), Ok(Planned::Approve));
        assert_eq!(plan(&mr("moi", "staging"), &MrAction::Approve, "moi", &trunks()), Err(Refused::OwnMergeRequest));
        assert_eq!(plan(&theirs, &MrAction::Comment("  ok  ".into()), "moi", &trunks()), Ok(Planned::Comment("ok".into())));
        assert_eq!(plan(&theirs, &MrAction::Comment(" ".into()), "moi", &trunks()), Err(Refused::InvalidComment));
    }

    #[test]
    fn draft_markers_are_toggled_through_the_title() {
        let mut draft = mr("moi", "staging");
        draft.state = MrState::Draft;
        draft.title = "[Draft] (draft) Draft: Ajoute le cache".into();
        assert_eq!(plan(&draft, &MrAction::MarkReady, "moi", &trunks()), Ok(Planned::Retitle("Ajoute le cache".into())));
        assert_eq!(plan(&draft, &MrAction::MarkDraft, "moi", &trunks()), Err(Refused::AlreadyThere));
        let mut ready = mr("moi", "staging");
        ready.title = "Ajoute le cache".into();
        assert_eq!(plan(&ready, &MrAction::MarkDraft, "moi", &trunks()), Ok(Planned::Retitle("Draft: Ajoute le cache".into())));
    }

    #[test]
    fn only_a_failed_pipeline_is_retried_and_closed_requests_are_left_alone() {
        let failing = mr("moi", "staging");
        assert_eq!(plan(&failing, &MrAction::RetryPipeline, "moi", &trunks()), Ok(Planned::RetryPipeline { pipeline_id: 42 }));
        let mut green = failing.clone();
        green.pipeline = Some(PipelineStatus::Success);
        assert_eq!(plan(&green, &MrAction::RetryPipeline, "moi", &trunks()), Err(Refused::NothingToRetry));
        let mut merged = failing;
        merged.state = MrState::Merged;
        assert_eq!(plan(&merged, &MrAction::Comment("x".into()), "moi", &trunks()), Err(Refused::NotOpen));
    }
}

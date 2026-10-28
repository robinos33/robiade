//! Actions sur une issue GitLab. Comme pour les MR, ce qui part est validé ici sur l'état courant de
//! l'issue, relu juste avant d'agir, puis décrit pour la confirmation et le journal d'audit.

use crate::forge::{Issue, IssueRef, IssueState};
use crate::merge_actions::AuditEntry;
use crate::Millis;

/// Ce que je demande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueAction {
    Close,
    Reopen,
}

/// Ce qui part vers GitLab, une fois les règles appliquées.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssuePlanned {
    Close,
    Reopen,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IssueRefused {
    #[error("l'issue est déjà fermée")]
    AlreadyClosed,
    #[error("l'issue est déjà ouverte")]
    AlreadyOpen,
}

/// Applique les règles à l'état **courant** de l'issue, relu juste avant d'agir.
pub fn plan(issue: &Issue, action: &IssueAction) -> Result<IssuePlanned, IssueRefused> {
    match (action, issue.state) {
        (IssueAction::Close, IssueState::Opened) => Ok(IssuePlanned::Close),
        (IssueAction::Close, IssueState::Closed) => Err(IssueRefused::AlreadyClosed),
        (IssueAction::Reopen, IssueState::Closed) => Ok(IssuePlanned::Reopen),
        (IssueAction::Reopen, IssueState::Opened) => Err(IssueRefused::AlreadyOpen),
    }
}

impl IssuePlanned {
    /// Description lisible, pour la confirmation et le journal d'audit.
    pub fn describe(&self) -> String {
        match self {
            IssuePlanned::Close => "fermer".into(),
            IssuePlanned::Reopen => "rouvrir".into(),
        }
    }
}

impl AuditEntry {
    pub fn on_issue(at: Millis, issue: &IssueRef, planned: &IssuePlanned, outcome: Result<(), String>) -> Self {
        Self {
            at,
            target: format!("{}#{}", issue.project, issue.iid),
            action: planned.describe(),
            outcome: outcome.err().unwrap_or_else(|| "ok".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(state: IssueState) -> Issue {
        Issue {
            issue: IssueRef { project: "a/b".into(), iid: 7 },
            title: "Cache".into(),
            state,
            description: String::new(),
            labels: vec![],
            assignees: vec![],
            web_url: String::new(),
            updated_at: 0,
            due_date: None,
            merge_requests: 0,
        }
    }

    #[test]
    fn an_issue_is_closed_or_reopened_only_from_the_other_state() {
        assert_eq!(plan(&issue(IssueState::Opened), &IssueAction::Close), Ok(IssuePlanned::Close));
        assert_eq!(plan(&issue(IssueState::Closed), &IssueAction::Close), Err(IssueRefused::AlreadyClosed));
        assert_eq!(plan(&issue(IssueState::Closed), &IssueAction::Reopen), Ok(IssuePlanned::Reopen));
        assert_eq!(plan(&issue(IssueState::Opened), &IssueAction::Reopen), Err(IssueRefused::AlreadyOpen));
    }

    #[test]
    fn the_audit_names_the_issue_with_a_hash() {
        let r = IssueRef { project: "a/b".into(), iid: 7 };
        let entry = AuditEntry::on_issue(1, &r, &IssuePlanned::Close, Err("statut HTTP 403".into()));
        assert_eq!((entry.target.as_str(), entry.action.as_str()), ("a/b#7", "fermer"));
        assert_eq!(entry.outcome, "statut HTTP 403");
    }
}

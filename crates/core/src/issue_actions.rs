//! Actions sur une issue GitLab. Comme pour les MR, ce qui part est validé ici sur l'état courant de
//! l'issue, relu juste avant d'agir, puis décrit pour la confirmation et le journal d'audit.

use crate::forge::{is_username, Issue, IssueRef, IssueState};
use crate::merge_actions::AuditEntry;
use crate::Millis;

/// Au-delà, ce n'est plus une assignation ni un étiquetage qu'on relit d'un coup d'œil.
pub const MAX_ASSIGNEES: usize = 20;
pub const MAX_LABELS: usize = 50;
/// Limite de GitLab sur le nom d'un label.
pub const LABEL_MAX_CHARS: usize = 255;

/// Ce que je demande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueAction {
    Close,
    Reopen,
    /// Assignés voulus, liste complète.
    Assign(Vec<String>),
    /// Labels voulus, liste complète.
    Label(Vec<String>),
}

/// Ce qui part vers GitLab, une fois les règles appliquées.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssuePlanned {
    Close,
    Reopen,
    /// GitLab remplace la liste des assignés : on envoie la liste finale, l'écart sert à la décrire.
    Assign { assignees: Vec<String>, added: Vec<String>, removed: Vec<String> },
    /// Seul l'écart part, en `add_labels` / `remove_labels` : un label posé entre-temps par un
    /// autre n'est pas effacé.
    Relabel { add: Vec<String>, remove: Vec<String> },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IssueRefused {
    #[error("l'issue est déjà fermée")]
    AlreadyClosed,
    #[error("l'issue est déjà ouverte")]
    AlreadyOpen,
    #[error("nom d'utilisateur invalide : {0}")]
    InvalidUsername(String),
    #[error("label invalide : {0}")]
    InvalidLabel(String),
    #[error("trop d'assignés ou de labels")]
    TooMany,
    #[error("rien ne change")]
    NoChange,
}

/// Applique les règles à l'état **courant** de l'issue, relu juste avant d'agir.
pub fn plan(issue: &Issue, action: &IssueAction) -> Result<IssuePlanned, IssueRefused> {
    match (action, issue.state) {
        (IssueAction::Close, IssueState::Opened) => Ok(IssuePlanned::Close),
        (IssueAction::Close, IssueState::Closed) => Err(IssueRefused::AlreadyClosed),
        (IssueAction::Reopen, IssueState::Closed) => Ok(IssuePlanned::Reopen),
        (IssueAction::Reopen, IssueState::Opened) => Err(IssueRefused::AlreadyOpen),
        (IssueAction::Assign(wanted), _) => {
            let assignees = usernames(wanted)?;
            let (added, removed) = difference(&issue.assignees, &assignees);
            if added.is_empty() && removed.is_empty() {
                return Err(IssueRefused::NoChange);
            }
            Ok(IssuePlanned::Assign { assignees, added, removed })
        }
        (IssueAction::Label(wanted), _) => {
            let (add, remove) = difference(&issue.labels, &labels(wanted)?);
            if add.is_empty() && remove.is_empty() {
                return Err(IssueRefused::NoChange);
            }
            Ok(IssuePlanned::Relabel { add, remove })
        }
    }
}

/// Noms d'utilisateur validés, sans `@` ni doublon, dans l'ordre donné.
pub fn usernames(raw: &[String]) -> Result<Vec<String>, IssueRefused> {
    let mut out: Vec<String> = Vec::new();
    for name in raw {
        let name = name.trim().trim_start_matches('@');
        if !is_username(name) {
            return Err(IssueRefused::InvalidUsername(name.to_owned()));
        }
        if !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
    }
    if out.len() > MAX_ASSIGNEES {
        return Err(IssueRefused::TooMany);
    }
    Ok(out)
}

/// Labels validés, sans doublon. La virgule est refusée : GitLab s'en sert de séparateur.
pub fn labels(raw: &[String]) -> Result<Vec<String>, IssueRefused> {
    let mut out: Vec<String> = Vec::new();
    for label in raw {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > LABEL_MAX_CHARS || label.contains([',', '\n']) {
            return Err(IssueRefused::InvalidLabel(label.to_owned()));
        }
        if !out.iter().any(|l| l == label) {
            out.push(label.to_owned());
        }
    }
    if out.len() > MAX_LABELS {
        return Err(IssueRefused::TooMany);
    }
    Ok(out)
}

/// Ce qui est dans `wanted` sans être dans `current`, et l'inverse.
fn difference(current: &[String], wanted: &[String]) -> (Vec<String>, Vec<String>) {
    let added = wanted.iter().filter(|w| !current.contains(w)).cloned().collect();
    let removed = current.iter().filter(|c| !wanted.contains(c)).cloned().collect();
    (added, removed)
}

impl IssuePlanned {
    /// Description lisible, pour la confirmation et le journal d'audit.
    pub fn describe(&self) -> String {
        match self {
            IssuePlanned::Close => "fermer".into(),
            IssuePlanned::Reopen => "rouvrir".into(),
            IssuePlanned::Assign { added, removed, .. } => both(
                (!added.is_empty()).then(|| format!("assigner à {}", added.join(", "))),
                (!removed.is_empty()).then(|| format!("désassigner {}", removed.join(", "))),
            ),
            IssuePlanned::Relabel { add, remove } => both(
                (!add.is_empty()).then(|| format!("ajouter les labels {}", add.join(", "))),
                (!remove.is_empty()).then(|| format!("retirer les labels {}", remove.join(", "))),
            ),
        }
    }
}

fn both(a: Option<String>, b: Option<String>) -> String {
    [a, b].into_iter().flatten().collect::<Vec<_>>().join(" et ")
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

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn assignees_are_replaced_and_the_change_is_described() {
        let mut current = issue(IssueState::Opened);
        current.assignees = strings(&["alice", "bob"]);
        let planned = plan(&current, &IssueAction::Assign(strings(&["@bob", " carol ", "bob"]))).unwrap();
        let expected = IssuePlanned::Assign {
            assignees: strings(&["bob", "carol"]),
            added: strings(&["carol"]),
            removed: strings(&["alice"]),
        };
        assert_eq!(planned, expected);
        assert_eq!(planned.describe(), "assigner à carol et désassigner alice");
        assert_eq!(plan(&current, &IssueAction::Assign(strings(&["bob", "alice"]))), Err(IssueRefused::NoChange));
        let refused = plan(&current, &IssueAction::Assign(strings(&["a/b"])));
        assert_eq!(refused, Err(IssueRefused::InvalidUsername("a/b".into())));
    }

    #[test]
    fn only_the_label_difference_is_sent() {
        let mut current = issue(IssueState::Closed);
        current.labels = strings(&["bug", "category::tech"]);
        let planned = plan(&current, &IssueAction::Label(strings(&["category::tech", "urgent "]))).unwrap();
        assert_eq!(planned, IssuePlanned::Relabel { add: strings(&["urgent"]), remove: strings(&["bug"]) });
        assert_eq!(planned.describe(), "ajouter les labels urgent et retirer les labels bug");
        let comma = plan(&current, &IssueAction::Label(strings(&["a,b"])));
        assert_eq!(comma, Err(IssueRefused::InvalidLabel("a,b".into())));
        assert_eq!(plan(&current, &IssueAction::Label(strings(&[""]))), Err(IssueRefused::InvalidLabel(String::new())));
    }

    #[test]
    fn the_audit_names_the_issue_with_a_hash() {
        let r = IssueRef { project: "a/b".into(), iid: 7 };
        let entry = AuditEntry::on_issue(1, &r, &IssuePlanned::Close, Err("statut HTTP 403".into()));
        assert_eq!((entry.target.as_str(), entry.action.as_str()), ("a/b#7", "fermer"));
        assert_eq!(entry.outcome, "statut HTTP 403");
    }
}

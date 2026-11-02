//! Démarrer un agent sur une tâche perso : même chemin que pour une issue (branche, worktree, prompt
//! relu avant envoi). Une tâche liée à une issue part de l'issue, qui porte le contexte ; mes notes
//! s'y ajoutent.

use crate::forge::Issue;
use crate::issue_start::{self, one_line, DESCRIPTION_MAX_CHARS, TITLE_MAX_CHARS};
use crate::tasks::Task;

/// `ticket-<iid>-…` pour une tâche liée à une issue, `tache-<titre>` sinon.
pub fn branch_name(task: &Task, issue: Option<&Issue>) -> String {
    if let Some(issue) = issue {
        return issue_start::branch_name(issue);
    }
    match issue_start::slug(task.title()) {
        slug if slug.is_empty() => format!("tache-{}", task.id().0.chars().take(8).collect::<String>()),
        slug => format!("tache-{slug}"),
    }
}

/// Nom de l'onglet de l'agent.
pub fn tab_name(task: &Task, issue: Option<&Issue>) -> String {
    issue.map_or_else(|| one_line(task.title(), TITLE_MAX_CHARS), issue_start::tab_name)
}

/// Prompt initial proposé ; il est relu et modifiable avant l'envoi. Les notes sont les miennes :
/// elles sont reprises telles quelles, à la différence d'une description venue de GitLab.
pub fn initial_prompt(task: &Task, issue: Option<&Issue>, branch: &str) -> String {
    let mut prompt = match issue {
        Some(issue) => issue_start::initial_prompt(issue, branch),
        None => format!(
            "Travaille sur ma tâche : « {title} ».\n\n\
             Tu es dans un worktree dédié, sur la branche `{branch}`.\n\
             Commence par me proposer un plan, sans modifier de fichier avant que je l'aie validé.",
            title = one_line(task.title(), TITLE_MAX_CHARS),
        ),
    };
    let notes = task.notes().trim();
    if !notes.is_empty() {
        let kept: String = notes.chars().take(DESCRIPTION_MAX_CHARS).collect();
        prompt.push_str("\n\nMes notes sur cette tâche :\n");
        prompt.push_str(&kept);
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{IssueRef, IssueState};
    use crate::tasks::TaskId;

    fn task(title: &str, notes: &str) -> Task {
        let mut t = Task::create(TaskId("4f1c9a2e-0000".into()), title, Some("/r/ade".into()), 0).unwrap();
        t.set_notes(notes);
        t
    }

    fn issue() -> Issue {
        Issue {
            issue: IssueRef { project: "robinos33/robiade".into(), iid: 6 },
            title: "Démarrer un agent".into(),
            state: IssueState::Opened,
            description: "Depuis l'onglet Tâches.".into(),
            labels: vec![],
            assignees: vec![],
            web_url: "https://gitlab.example.com/robinos33/robiade/-/work_items/6".into(),
            updated_at: 0,
            due_date: None,
            merge_requests: 0,
        }
    }

    #[test]
    fn a_plain_task_names_its_own_branch() {
        assert_eq!(branch_name(&task("Notifs propres !", ""), None), "tache-notifs-propres");
        assert_eq!(branch_name(&task("🔥", ""), None), "tache-4f1c9a2e");
        assert!(issue_start::is_valid_branch(&branch_name(&task("🔥", ""), None)));
        assert_eq!(tab_name(&task("Notifs\npropres", ""), None), "Notifs propres");
    }

    #[test]
    fn a_task_linked_to_an_issue_starts_from_the_issue() {
        let t = task("Ma version", "");
        assert_eq!(branch_name(&t, Some(&issue())), "ticket-6-demarrer-un-agent");
        assert_eq!(tab_name(&t, Some(&issue())), "#6 Démarrer un agent");
        assert!(initial_prompt(&t, Some(&issue()), "b").contains("robinos33/robiade#6"));
    }

    #[test]
    fn my_notes_follow_the_prompt() {
        let prompt = initial_prompt(&task("Notifs", "  Commencer par le spike.  "), None, "tache-notifs");
        assert!(prompt.starts_with("Travaille sur ma tâche : « Notifs »."));
        assert!(prompt.contains("`tache-notifs`"));
        assert!(prompt.ends_with("Mes notes sur cette tâche :\nCommencer par le spike."));
        assert!(!initial_prompt(&task("Notifs", " "), None, "b").contains("Mes notes"));
    }
}

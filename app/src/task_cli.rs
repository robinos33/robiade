//! `robiade task done` : un agent ferme la tâche qui l'a lancé, quand je le lui demande.
//!
//! Sans argument : la tâche est celle liée à la session de l'agent, lue dans son environnement.
//! Ces variables ne sont pas une barrière (l'agent tourne sous mon utilisateur) : elles évitent
//! seulement de fermer une autre tâche par erreur.

use std::path::Path;

use robiade_adapters::sqlite_tasks::SqliteTaskRepository;
use robiade_core::tasks::{finish_session_task, SessionId, TaskError};

/// Session Claude Code de l'agent : celle qu'il reprend, sinon la sienne.
pub const ENV_SESSION_ID: &str = "ROBIADE_SESSION_ID";
/// `robiade.db` de l'instance qui a lancé l'agent (installée ou `tauri dev`).
pub const ENV_DB: &str = "ROBIADE_DB";
/// Ce binaire : il n'est pas dans le PATH de l'agent. Nommé tel quel dans le prompt de `task_start`.
pub const ENV_BIN: &str = "ROBIADE_BIN";

const USAGE: &str = "usage : robiade task done";

/// Code de sortie et message : sur la sortie standard si 0, sur l'erreur sinon.
/// 1 : refusé ou impossible ; 2 : mauvais usage ou lancé hors d'un agent de Robiade.
pub fn run(args: &[String], var: impl Fn(&str) -> Option<String>, now: i64) -> (i32, String) {
    if args != ["done"] {
        return (2, USAGE.into());
    }
    let missing = |name| (2, format!("{name} absent : à lancer depuis un agent démarré par Robiade"));
    let Some(session) = var(ENV_SESSION_ID) else { return missing(ENV_SESSION_ID) };
    let Some(db) = var(ENV_DB) else { return missing(ENV_DB) };
    let session = match SessionId::parse(&session) {
        Ok(session) => session,
        Err(e) => return (2, e.to_string()),
    };
    // `open` créerait une base vide : une base absente est une erreur, pas une liste vide.
    if !Path::new(&db).is_file() {
        return (2, format!("base des tâches introuvable : {db}"));
    }
    let done = SqliteTaskRepository::open(Path::new(&db)).and_then(|tasks| finish_session_task(&tasks, &session, now));
    match done {
        Ok(task) => (0, format!("Tâche « {} » marquée terminée.", task.title())),
        Err(e @ (TaskError::NoLinkedTask | TaskError::AlreadyDone(_) | TaskError::SeveralLinked(_))) => (1, e.to_string()),
        Err(e) => (1, format!("tâche non fermée : {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::tasks::{Task, TaskId, TaskRepository, TaskStatus};

    const SESSION: &str = "0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6b";

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn closes_the_task_of_the_session_then_refuses_twice() {
        let dir = std::env::temp_dir().join(format!("robiade-task-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("robiade.db");
        let _ = std::fs::remove_file(&db);
        let repo = SqliteTaskRepository::open(&db).unwrap();
        let mut task = Task::create(TaskId("t".into()), "Animer le logo", None, 1).unwrap();
        task.start(SessionId::parse(SESSION).unwrap(), 2);
        repo.save(&task).unwrap();

        let vars = [(ENV_SESSION_ID, SESSION), (ENV_DB, db.to_str().unwrap())];
        let args = ["done".to_owned()];
        assert_eq!(run(&args, env(&vars), 30), (0, "Tâche « Animer le logo » marquée terminée.".into()));
        let saved = repo.get(&TaskId("t".into())).unwrap();
        assert_eq!((saved.status(), saved.done_at()), (TaskStatus::Done, Some(30)));
        assert_eq!(run(&args, env(&vars), 40).0, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_outside_an_agent_or_with_arguments() {
        let done = ["done".to_owned()];
        assert_eq!(run(&[], env(&[]), 0).0, 2);
        assert_eq!(run(&["done".to_owned(), "t".to_owned()], env(&[]), 0).0, 2);
        assert_eq!(run(&done, env(&[]), 0).0, 2);
        assert_eq!(run(&done, env(&[(ENV_SESSION_ID, SESSION), (ENV_DB, "/nulle/part.db")]), 0).0, 2);
        assert_eq!(run(&done, env(&[(ENV_SESSION_ID, "pas-un-uuid"), (ENV_DB, "/tmp")]), 0).0, 2);
    }
}

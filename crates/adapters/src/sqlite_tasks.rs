//! Tâches perso dans `robiade.db` — la seule base dont Robiade est la source de vérité.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::forge::IssueRef;
use robiade_core::tasks::{DueDate, SessionId, Task, TaskError, TaskId, TaskRepository, TaskStatus};
use rusqlite::{params, Connection, OptionalExtension, Row};

pub struct SqliteTaskRepository {
    conn: Mutex<Connection>,
}

fn storage(e: impl std::fmt::Display) -> TaskError {
    TaskError::Storage(e.to_string())
}

impl SqliteTaskRepository {
    pub fn open(path: &Path) -> Result<Self, TaskError> {
        let conn = Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS task (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 notes TEXT NOT NULL DEFAULT '',
                 project_path TEXT,
                 status TEXT NOT NULL CHECK (status IN ('todo', 'doing', 'done')),
                 created_at INTEGER NOT NULL,
                 done_at INTEGER
             );",
        )
        .map_err(storage)?;
        Self::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Évolutions du schéma, rejouées une seule fois par base : la table d'origine reste créée telle
    /// quelle, chaque étape s'applique ensuite par-dessus. Ajouter une étape, jamais en modifier une.
    fn migrate(conn: &Connection) -> Result<(), TaskError> {
        const STEPS: &[&str] = &[
            "ALTER TABLE task ADD COLUMN due_on TEXT;",
            "ALTER TABLE task ADD COLUMN issue_project TEXT;
             ALTER TABLE task ADD COLUMN issue_iid INTEGER;
             ALTER TABLE task ADD COLUMN session_id TEXT;",
        ];
        let done: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(storage)?;
        for (i, step) in STEPS.iter().enumerate().skip(done) {
            conn.execute_batch(&format!("BEGIN; {step} PRAGMA user_version = {}; COMMIT;", i + 1))
                .map_err(storage)?;
        }
        Ok(())
    }

    fn from_row(row: &Row) -> rusqlite::Result<Task> {
        // Écrite par `save`, donc valide ; une valeur illisible vaut « pas d'échéance » plutôt qu'une liste vide.
        let due_on = row.get::<_, Option<String>>(5)?.and_then(|d| DueDate::parse(&d).ok());
        // Même règle pour les liens : un lien illisible est un lien absent.
        let issue = match (row.get::<_, Option<String>>(8)?, row.get::<_, Option<i64>>(9)?) {
            (Some(project), Some(iid)) => IssueRef::parse(&format!("{project}#{iid}"), None),
            _ => None,
        };
        let session = row.get::<_, Option<String>>(10)?.and_then(|s| SessionId::parse(&s).ok());
        let status = match row.get::<_, String>(4)?.as_str() {
            "doing" => TaskStatus::Doing,
            "done" => TaskStatus::Done,
            _ => TaskStatus::Todo,
        };
        Ok(Task::restore(
            TaskId(row.get(0)?),
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            status,
            due_on,
            issue,
            session,
            row.get(6)?,
            row.get(7)?,
        ))
    }
}

const COLUMNS: &str =
    "id, title, notes, project_path, status, due_on, created_at, done_at, issue_project, issue_iid, session_id";

impl TaskRepository for SqliteTaskRepository {
    fn all(&self) -> Result<Vec<Task>, TaskError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn
            .prepare(&format!("SELECT {COLUMNS} FROM task ORDER BY created_at DESC"))
            .map_err(storage)?;
        let rows = stmt.query_map([], Self::from_row).map_err(storage)?;
        rows.collect::<Result<_, _>>().map_err(storage)
    }

    fn get(&self, id: &TaskId) -> Result<Task, TaskError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.query_row(&format!("SELECT {COLUMNS} FROM task WHERE id = ?1"), [&id.0], Self::from_row)
            .optional()
            .map_err(storage)?
            .ok_or(TaskError::NotFound)
    }

    fn save(&self, task: &Task) -> Result<(), TaskError> {
        let status = match task.status() {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Done => "done",
        };
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute(
            &format!(
                "INSERT INTO task ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET title = ?2, notes = ?3, project_path = ?4,
                     status = ?5, due_on = ?6, done_at = ?8, issue_project = ?9, issue_iid = ?10,
                     session_id = ?11"
            ),
            params![
                task.id().0,
                task.title(),
                task.notes(),
                task.project_path(),
                status,
                task.due_on().map(DueDate::as_str),
                task.created_at(),
                task.done_at(),
                task.issue().map(|i| i.project.as_str()),
                task.issue().map(|i| i.iid as i64),
                task.session().map(SessionId::as_str)
            ],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn delete(&self, id: &TaskId) -> Result<(), TaskError> {
        let conn = self.conn.lock().map_err(storage)?;
        match conn.execute("DELETE FROM task WHERE id = ?1", [&id.0]).map_err(storage)? {
            0 => Err(TaskError::NotFound),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_task() {
        let repo = SqliteTaskRepository::open(Path::new(":memory:")).unwrap();
        let mut task = Task::create(TaskId("a".into()), "Écrire la spec", Some("/p".into()), 1).unwrap();
        repo.save(&task).unwrap();
        task.schedule(Some(DueDate::parse("2026-11-02").unwrap()));
        task.link_issue(Some(IssueRef { project: "shop/boutique".into(), iid: 12 }));
        task.link_session(Some(SessionId::parse("0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6b").unwrap()));
        task.move_to(TaskStatus::Done, 5);
        repo.save(&task).unwrap();
        assert_eq!(repo.get(&TaskId("a".into())).unwrap(), task);
        assert_eq!(repo.all().unwrap().len(), 1);
        repo.delete(&TaskId("a".into())).unwrap();
        assert_eq!(repo.get(&TaskId("a".into())), Err(TaskError::NotFound));
    }

    #[test]
    fn migrates_a_base_created_before_due_dates_and_only_once() {
        let dir = std::env::temp_dir().join(format!("robiade-tasks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("robiade.db");
        let _ = std::fs::remove_file(&path);
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "CREATE TABLE task (id TEXT PRIMARY KEY, title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '',
                     project_path TEXT, status TEXT NOT NULL, created_at INTEGER NOT NULL, done_at INTEGER);
                 INSERT INTO task VALUES ('old', 'Avant', '', NULL, 'todo', 1, NULL);",
            )
            .unwrap();
        }
        let repo = SqliteTaskRepository::open(&path).unwrap();
        let mut old = repo.get(&TaskId("old".into())).unwrap();
        assert_eq!(old.due_on(), None);
        old.schedule(Some(DueDate::parse("2026-11-03").unwrap()));
        repo.save(&old).unwrap();
        drop(repo);
        // Réouverture : la colonne existe déjà, l'étape ne doit pas être rejouée.
        let repo = SqliteTaskRepository::open(&path).unwrap();
        assert_eq!(repo.get(&TaskId("old".into())).unwrap(), old);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! Tâches perso dans `robiade.db` — la seule base dont Robiade est la source de vérité.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::tasks::{Task, TaskError, TaskId, TaskRepository, TaskStatus};
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
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn from_row(row: &Row) -> rusqlite::Result<Task> {
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
            row.get(5)?,
            row.get(6)?,
        ))
    }
}

const COLUMNS: &str = "id, title, notes, project_path, status, created_at, done_at";

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
                "INSERT INTO task ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET title = ?2, notes = ?3, project_path = ?4,
                     status = ?5, done_at = ?7"
            ),
            params![
                task.id().0,
                task.title(),
                task.notes(),
                task.project_path(),
                status,
                task.created_at(),
                task.done_at()
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
        task.move_to(TaskStatus::Done, 5);
        repo.save(&task).unwrap();
        assert_eq!(repo.get(&TaskId("a".into())).unwrap(), task);
        assert_eq!(repo.all().unwrap().len(), 1);
        repo.delete(&TaskId("a".into())).unwrap();
        assert_eq!(repo.get(&TaskId("a".into())), Err(TaskError::NotFound));
    }
}

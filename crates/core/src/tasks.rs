//! Tâches perso : le seul contexte dont Robiade est la source de vérité.

use crate::Millis;

pub const TITLE_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskError {
    #[error("le titre est vide")]
    EmptyTitle,
    #[error("le titre dépasse {TITLE_MAX_CHARS} caractères")]
    TitleTooLong,
    #[error("tâche introuvable")]
    NotFound,
    #[error("stockage des tâches indisponible : {0}")]
    Storage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    title: String,
    notes: String,
    /// Absent : tâche globale.
    project_path: Option<String>,
    status: TaskStatus,
    created_at: Millis,
    done_at: Option<Millis>,
}

impl Task {
    pub fn create(id: TaskId, title: &str, project_path: Option<String>, now: Millis) -> Result<Self, TaskError> {
        Ok(Self {
            id,
            title: Self::valid_title(title)?,
            notes: String::new(),
            project_path,
            status: TaskStatus::Todo,
            created_at: now,
            done_at: None,
        })
    }

    /// Reconstitution depuis le stockage : les invariants ont été vérifiés à l'écriture.
    pub fn restore(
        id: TaskId,
        title: String,
        notes: String,
        project_path: Option<String>,
        status: TaskStatus,
        created_at: Millis,
        done_at: Option<Millis>,
    ) -> Self {
        Self { id, title, notes, project_path, status, created_at, done_at }
    }

    pub fn rename(&mut self, title: &str) -> Result<(), TaskError> {
        self.title = Self::valid_title(title)?;
        Ok(())
    }

    pub fn set_notes(&mut self, notes: &str) {
        self.notes = notes.to_owned();
    }

    pub fn move_to(&mut self, status: TaskStatus, now: Millis) {
        self.done_at = (status == TaskStatus::Done).then_some(self.done_at.unwrap_or(now));
        self.status = status;
    }

    fn valid_title(raw: &str) -> Result<String, TaskError> {
        let title = raw.trim();
        if title.is_empty() {
            return Err(TaskError::EmptyTitle);
        }
        if title.chars().count() > TITLE_MAX_CHARS {
            return Err(TaskError::TitleTooLong);
        }
        Ok(title.to_owned())
    }

    pub fn id(&self) -> &TaskId {
        &self.id
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn notes(&self) -> &str {
        &self.notes
    }
    pub fn project_path(&self) -> Option<&str> {
        self.project_path.as_deref()
    }
    pub fn status(&self) -> TaskStatus {
        self.status
    }
    pub fn created_at(&self) -> Millis {
        self.created_at
    }
    pub fn done_at(&self) -> Option<Millis> {
        self.done_at
    }
}

pub trait TaskRepository: Send + Sync {
    fn all(&self) -> Result<Vec<Task>, TaskError>;
    fn get(&self, id: &TaskId) -> Result<Task, TaskError>;
    fn save(&self, task: &Task) -> Result<(), TaskError>;
    fn delete(&self, id: &TaskId) -> Result<(), TaskError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task::create(TaskId("t1".into()), "  Relire la MR  ", None, 10).unwrap()
    }

    #[test]
    fn trims_and_validates_the_title() {
        assert_eq!(task().title(), "Relire la MR");
        assert_eq!(Task::create(TaskId("t".into()), "   ", None, 0), Err(TaskError::EmptyTitle));
        let long = "é".repeat(TITLE_MAX_CHARS + 1);
        assert_eq!(Task::create(TaskId("t".into()), &long, None, 0), Err(TaskError::TitleTooLong));
    }

    #[test]
    fn done_keeps_its_first_timestamp_and_clears_when_reopened() {
        let mut t = task();
        t.move_to(TaskStatus::Done, 20);
        t.move_to(TaskStatus::Done, 30);
        assert_eq!(t.done_at(), Some(20));
        t.move_to(TaskStatus::Doing, 40);
        assert_eq!(t.done_at(), None);
    }
}

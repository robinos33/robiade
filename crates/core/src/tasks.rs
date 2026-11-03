//! Tâches perso : le seul contexte dont Robiade est la source de vérité.

use crate::forge::IssueRef;
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
    #[error("échéance invalide : {0} (attendu AAAA-MM-JJ)")]
    InvalidDueDate(String),
    #[error("identifiant de session invalide : {0}")]
    InvalidSession(String),
    #[error("tâche introuvable")]
    NotFound,
    #[error("aucune tâche n'est liée à cette session")]
    NoLinkedTask,
    #[error("la tâche « {0} » est déjà terminée")]
    AlreadyDone(String),
    #[error("{0} tâches sont liées à cette session : fermer la bonne depuis l'onglet Tâches")]
    SeveralLinked(usize),
    #[error("stockage des tâches indisponible : {0}")]
    Storage(String),
}

/// Échéance : un jour du calendrier, sans heure ni fuseau — c'est l'utilisateur qui sait quel jour on est.
/// Format ISO `AAAA-MM-JJ`, dont l'ordre lexical est l'ordre chronologique.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DueDate(String);

impl DueDate {
    pub fn parse(raw: &str) -> Result<Self, TaskError> {
        let invalid = || TaskError::InvalidDueDate(raw.to_owned());
        let bytes = raw.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(invalid());
        }
        let number = |range: std::ops::Range<usize>| -> Result<u32, TaskError> {
            let part = &raw[range];
            if !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(invalid()),
        };
        if day == 0 || day > days_in_month {
            return Err(invalid());
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    title: String,
    notes: String,
    /// Absent : tâche globale.
    project_path: Option<String>,
    status: TaskStatus,
    due_on: Option<DueDate>,
    /// Issue d'équipe référencée, jamais recopiée : rien d'elle n'est gardé ici.
    issue: Option<IssueRef>,
    /// Session Claude Code où la tâche a été menée.
    session: Option<SessionId>,
    created_at: Millis,
    done_at: Option<Millis>,
}

/// Identifiant d'une session Claude Code : un UUID, tel que Robiade le génère au lancement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    pub fn parse(raw: &str) -> Result<Self, TaskError> {
        let groups: Vec<&str> = raw.split('-').collect();
        let lengths = groups.iter().map(|g| g.len()).collect::<Vec<_>>();
        let hex = raw.bytes().all(|b| b == b'-' || b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if lengths != [8, 4, 4, 4, 12] || !hex {
            return Err(TaskError::InvalidSession(raw.to_owned()));
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Task {
    pub fn create(id: TaskId, title: &str, project_path: Option<String>, now: Millis) -> Result<Self, TaskError> {
        Ok(Self {
            id,
            title: Self::valid_title(title)?,
            notes: String::new(),
            project_path,
            status: TaskStatus::Todo,
            due_on: None,
            issue: None,
            session: None,
            created_at: now,
            done_at: None,
        })
    }

    /// Reconstitution depuis le stockage : les invariants ont été vérifiés à l'écriture.
    /// Une valeur par colonne : un objet intermédiaire ne ferait que déplacer la liste.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: TaskId,
        title: String,
        notes: String,
        project_path: Option<String>,
        status: TaskStatus,
        due_on: Option<DueDate>,
        issue: Option<IssueRef>,
        session: Option<SessionId>,
        created_at: Millis,
        done_at: Option<Millis>,
    ) -> Self {
        Self { id, title, notes, project_path, status, due_on, issue, session, created_at, done_at }
    }

    pub fn rename(&mut self, title: &str) -> Result<(), TaskError> {
        self.title = Self::valid_title(title)?;
        Ok(())
    }

    pub fn set_notes(&mut self, notes: &str) {
        self.notes = notes.to_owned();
    }

    /// `None` retire l'échéance.
    pub fn schedule(&mut self, due_on: Option<DueDate>) {
        self.due_on = due_on;
    }

    /// `None` retire le lien.
    pub fn link_issue(&mut self, issue: Option<IssueRef>) {
        self.issue = issue;
    }

    /// `None` retire le lien.
    pub fn link_session(&mut self, session: Option<SessionId>) {
        self.session = session;
    }

    /// Un agent démarré sur la tâche : elle pointe sur sa session et passe en cours, même si elle
    /// était finie — la relancer, c'est la rouvrir.
    pub fn start(&mut self, session: SessionId, now: Millis) {
        self.link_session(Some(session));
        self.move_to(TaskStatus::Doing, now);
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
    pub fn due_on(&self) -> Option<&DueDate> {
        self.due_on.as_ref()
    }
    pub fn issue(&self) -> Option<&IssueRef> {
        self.issue.as_ref()
    }
    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
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

/// Un agent ferme la tâche qui l'a lancé : celle liée à sa session, et elle seule. Refusé si aucune
/// tâche n'y est liée, si elle est déjà terminée, ou si plusieurs le sont — à moi de choisir.
pub fn finish_session_task(tasks: &dyn TaskRepository, session: &SessionId, now: Millis) -> Result<Task, TaskError> {
    let mut linked: Vec<Task> = tasks.all()?.into_iter().filter(|t| t.session() == Some(session)).collect();
    let mut task = match linked.len() {
        0 => return Err(TaskError::NoLinkedTask),
        1 => linked.remove(0),
        n => return Err(TaskError::SeveralLinked(n)),
    };
    if task.status() == TaskStatus::Done {
        return Err(TaskError::AlreadyDone(task.title().to_owned()));
    }
    task.move_to(TaskStatus::Done, now);
    tasks.save(&task)?;
    Ok(task)
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
    fn starting_an_agent_links_its_session_and_reopens_the_task() {
        let mut t = task();
        t.move_to(TaskStatus::Done, 20);
        let session = SessionId::parse("0b6f2c1e-7a4d-4c39-9f2e-5d8a1b3c4e6f").unwrap();
        t.start(session.clone(), 30);
        assert_eq!((t.status(), t.session(), t.done_at()), (TaskStatus::Doing, Some(&session), None));
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

    #[test]
    fn due_date_is_a_real_calendar_day() {
        assert_eq!(DueDate::parse("2026-11-02").unwrap().as_str(), "2026-11-02");
        assert!(DueDate::parse("2028-02-29").is_ok());
        assert!(DueDate::parse("2000-02-29").is_ok());
        for bad in ["", "2026-11-2", "2026/11/02", "2026-13-01", "2026-00-10", "2026-04-31", "2027-02-29", "1900-02-29", "2026-1a-02", "+026-11-02"] {
            assert_eq!(DueDate::parse(bad), Err(TaskError::InvalidDueDate(bad.into())), "{bad}");
        }
    }

    #[test]
    fn due_dates_sort_chronologically() {
        assert!(DueDate::parse("2026-09-30").unwrap() < DueDate::parse("2026-10-01").unwrap());
    }

    #[test]
    fn a_session_id_is_a_lowercase_uuid() {
        let id = "0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6b";
        assert_eq!(SessionId::parse(id).unwrap().as_str(), id);
        for bad in ["", "0b7f3c2e", "0B7F3C2E-9A41-4D3B-8E6F-1C2D3E4F5A6B", "0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6", "../../etc/passwd-0000-0000-000000000000", "0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6g"] {
            assert_eq!(SessionId::parse(bad), Err(TaskError::InvalidSession(bad.into())), "{bad}");
        }
    }

    #[test]
    fn links_are_set_and_cleared() {
        let mut t = task();
        t.link_issue(Some(IssueRef { project: "a/b".into(), iid: 3 }));
        t.link_session(Some(SessionId::parse("0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6b").unwrap()));
        assert_eq!(t.issue().map(|i| i.iid), Some(3));
        assert!(t.session().is_some());
        t.link_issue(None);
        t.link_session(None);
        assert_eq!((t.issue(), t.session()), (None, None));
    }

    #[test]
    fn schedule_sets_and_clears_the_due_date() {
        let mut t = task();
        t.schedule(Some(DueDate::parse("2026-11-02").unwrap()));
        assert_eq!(t.due_on().map(DueDate::as_str), Some("2026-11-02"));
        t.schedule(None);
        assert_eq!(t.due_on(), None);
    }

    /// Dépôt en mémoire, qui compte les écritures.
    #[derive(Default)]
    struct Memory {
        tasks: std::sync::Mutex<Vec<Task>>,
        saves: std::sync::atomic::AtomicUsize,
    }

    impl TaskRepository for Memory {
        fn all(&self) -> Result<Vec<Task>, TaskError> {
            Ok(self.tasks.lock().unwrap().clone())
        }
        fn get(&self, id: &TaskId) -> Result<Task, TaskError> {
            self.all()?.into_iter().find(|t| t.id() == id).ok_or(TaskError::NotFound)
        }
        fn save(&self, task: &Task) -> Result<(), TaskError> {
            self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut tasks = self.tasks.lock().unwrap();
            tasks.retain(|t| t.id() != task.id());
            tasks.push(task.clone());
            Ok(())
        }
        fn delete(&self, id: &TaskId) -> Result<(), TaskError> {
            self.tasks.lock().unwrap().retain(|t| t.id() != id);
            Ok(())
        }
    }

    const MINE: &str = "0b7f3c2e-9a41-4d3b-8e6f-1c2d3e4f5a6b";
    const OTHER: &str = "1c8f4d3f-0b52-4e4c-9f70-2d3e4f5a6b7c";

    fn started(id: &str, session: &str) -> Task {
        let mut t = Task::create(TaskId(id.into()), id, None, 0).unwrap();
        t.start(SessionId::parse(session).unwrap(), 5);
        t
    }

    fn memory(tasks: Vec<Task>) -> Memory {
        Memory { tasks: std::sync::Mutex::new(tasks), ..Memory::default() }
    }

    #[test]
    fn an_agent_finishes_the_task_linked_to_its_session_only() {
        let repo = memory(vec![started("mine", MINE), started("other", OTHER)]);
        let done = finish_session_task(&repo, &SessionId::parse(MINE).unwrap(), 50).unwrap();
        assert_eq!((done.id().0.as_str(), done.status(), done.done_at()), ("mine", TaskStatus::Done, Some(50)));
        assert_eq!(repo.get(&TaskId("mine".into())).unwrap(), done);
        assert_eq!(repo.get(&TaskId("other".into())).unwrap().status(), TaskStatus::Doing);
    }

    #[test]
    fn finishing_is_refused_without_a_linked_task() {
        let repo = memory(vec![started("other", OTHER), task()]);
        assert_eq!(finish_session_task(&repo, &SessionId::parse(MINE).unwrap(), 50), Err(TaskError::NoLinkedTask));
        assert_eq!(repo.saves.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn finishing_twice_is_refused_and_keeps_the_first_timestamp() {
        let repo = memory(vec![started("mine", MINE)]);
        let session = SessionId::parse(MINE).unwrap();
        finish_session_task(&repo, &session, 50).unwrap();
        assert_eq!(finish_session_task(&repo, &session, 60), Err(TaskError::AlreadyDone("mine".into())));
        assert_eq!(repo.get(&TaskId("mine".into())).unwrap().done_at(), Some(50));
        assert_eq!(repo.saves.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn finishing_is_refused_when_several_tasks_share_the_session() {
        let repo = memory(vec![started("a", MINE), started("b", MINE)]);
        assert_eq!(finish_session_task(&repo, &SessionId::parse(MINE).unwrap(), 50), Err(TaskError::SeveralLinked(2)));
        assert_eq!(repo.saves.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

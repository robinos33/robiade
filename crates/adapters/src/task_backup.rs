//! Sauvegarde des tâches perso : un export JSON par jour, à côté de `robiade.db`.
//!
//! Les tâches perso sont la seule donnée dont Robiade est la source de vérité : une base abîmée ne
//! doit pas les emporter. L'export est lisible sans Robiade, horodaté par son nom
//! (`tasks-AAAA-MM-JJ.json`), écrit une fois par jour et jamais réécrit ; les plus anciens au-delà
//! de [`KEEP`] sont supprimés. Il n'y a pas de restauration automatique : on relit le fichier.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use robiade_core::tasks::{DueDate, SessionId, Task, TaskRepository, TaskStatus};
use serde_json::{json, Value};

/// Exports gardés : un mois de jours d'utilisation.
pub const KEEP: usize = 30;

const PREFIX: &str = "tasks-";
const SUFFIX: &str = ".json";

/// Écrit l'export du jour s'il n'existe pas encore, puis élague. Renvoie le fichier écrit, ou `None`
/// si celui du jour était déjà là.
pub fn export_daily(tasks: &dyn TaskRepository, dir: &Path, day: &str) -> Result<Option<PathBuf>, String> {
    let path = dir.join(format!("{PREFIX}{day}{SUFFIX}"));
    if path.exists() {
        return Ok(None);
    }
    let tasks = tasks.all().map_err(|e| e.to_string())?;
    let body = serde_json::to_vec_pretty(&tasks.iter().map(to_json).collect::<Vec<_>>()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Écrit à côté puis renommé : un export interrompu ne laisse pas un fichier du jour tronqué.
    let partial = dir.join(format!(".{PREFIX}{day}{SUFFIX}.partial"));
    std::fs::write(&partial, body).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    prune(dir).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

fn to_json(t: &Task) -> Value {
    json!({
        "id": t.id().0,
        "title": t.title(),
        "notes": t.notes(),
        "projectPath": t.project_path(),
        "status": match t.status() {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Done => "done",
        },
        "dueOn": t.due_on().map(DueDate::as_str),
        "issue": t.issue().map(|i| json!({ "project": i.project, "iid": i.iid })),
        "sessionId": t.session().map(SessionId::as_str),
        "createdAt": t.created_at(),
        "doneAt": t.done_at(),
    })
}

/// Garde les [`KEEP`] exports les plus récents ; l'ordre des noms est l'ordre des jours.
fn prune(dir: &Path) -> std::io::Result<()> {
    let mut exports: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(PREFIX)?.strip_suffix(SUFFIX))
                .is_some_and(|day| DueDate::parse(day).is_ok())
        })
        .collect();
    exports.sort();
    let excess = exports.len().saturating_sub(KEEP);
    for old in &exports[..excess] {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

/// Jour du calendrier local, `AAAA-MM-JJ` : c'est le jour que je vis, pas celui d'UTC.
pub fn local_day(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as libc::time_t;
    // SAFETY : `localtime_r` n'écrit que dans `tm`, une valeur locale initialisée à zéro.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_tasks::SqliteTaskRepository;
    use robiade_core::forge::IssueRef;
    use robiade_core::tasks::TaskId;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("robiade-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_one_readable_export_a_day() {
        let dir = temp_dir("daily");
        let repo = SqliteTaskRepository::open(Path::new(":memory:")).unwrap();
        let mut task = Task::create(TaskId("a".into()), "Écrire la spec", Some("/p".into()), 1).unwrap();
        task.link_issue(Some(IssueRef { project: "a/b".into(), iid: 7 }));
        repo.save(&task).unwrap();

        let written = export_daily(&repo, &dir, "2026-11-02").unwrap().unwrap();
        let exported: Value = serde_json::from_slice(&std::fs::read(&written).unwrap()).unwrap();
        assert_eq!(exported[0]["title"], "Écrire la spec");
        assert_eq!(exported[0]["issue"], json!({ "project": "a/b", "iid": 7 }));
        assert_eq!(exported[0]["dueOn"], Value::Null);

        // Le même jour, rien n'est réécrit, même si les tâches ont changé.
        repo.delete(&TaskId("a".into())).unwrap();
        assert_eq!(export_daily(&repo, &dir, "2026-11-02").unwrap(), None);
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&written).unwrap()).unwrap(), exported);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_only_the_most_recent_exports_and_nothing_else_is_touched() {
        let dir = temp_dir("prune");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.json"), "{}").unwrap();
        std::fs::write(dir.join("tasks-pas-un-jour.json"), "[]").unwrap();
        let repo = SqliteTaskRepository::open(Path::new(":memory:")).unwrap();
        for i in 0..KEEP + 2 {
            export_daily(&repo, &dir, &format!("2026-{:02}-{:02}", 1 + i / 28, 1 + i % 28)).unwrap();
        }
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left.len(), KEEP + 2);
        assert_eq!(left[0], "notes.json");
        assert_eq!(left[1], "tasks-2026-01-03.json");
        assert!(left.contains(&"tasks-pas-un-jour.json".to_owned()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_local_day_is_a_calendar_day() {
        let day = local_day(SystemTime::now());
        assert!(DueDate::parse(&day).is_ok(), "{day}");
    }
}

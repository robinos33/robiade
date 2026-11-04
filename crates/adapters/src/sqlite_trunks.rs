//! Branches de tronc réglées, dépôt par dépôt, dans `robiade.db`. Un dépôt est désigné par la racine
//! de son arbre principal : ses worktrees, qui partagent son `.git`, partagent son réglage.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::trunks::TrunkBranches;
use rusqlite::params;

use crate::sqlite_settings::table_count;

/// Troncs donnés à chaque dépôt connu d'un profil antérieur : ceux que le code fixait alors.
const LEGACY_TRUNKS: [&str; 3] = ["staging", "main", "master"];

#[derive(Debug, thiserror::Error)]
#[error("stockage des branches de tronc : {0}")]
pub struct TrunkStorageError(String);

fn storage(e: impl std::fmt::Display) -> TrunkStorageError {
    TrunkStorageError(e.to_string())
}

pub struct SqliteTrunks {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteTrunks {
    /// `earlier_profile` (voir `sqlite_settings::is_earlier_profile`) : à la création de la table,
    /// chaque dépôt que `known` renvoie reçoit les troncs que le code fixait, une seule fois.
    pub fn open(
        path: &Path,
        earlier_profile: bool,
        known: impl FnOnce() -> Vec<String>,
    ) -> Result<Self, TrunkStorageError> {
        let conn = rusqlite::Connection::open(path).map_err(storage)?;
        conn.execute_batch("PRAGMA journal_mode = WAL;").map_err(storage)?;
        let created = table_count(&conn, Some("trunk_branch")).map_err(storage)? == 0;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS trunk_branch (
                 repository TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 branch TEXT NOT NULL,
                 PRIMARY KEY (repository, position)
             );",
        )
        .map_err(storage)?;
        let store = Self { conn: Mutex::new(conn) };
        if created && earlier_profile {
            let legacy = TrunkBranches::new(LEGACY_TRUNKS).map_err(storage)?;
            for repository in known() {
                store.set(&repository, Some(&legacy))?;
            }
        }
        Ok(store)
    }

    /// `None` : pas de réglage, le tronc est la branche par défaut du remote.
    pub fn get(&self, repository: &str) -> Result<Option<TrunkBranches>, TrunkStorageError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT branch FROM trunk_branch WHERE repository = ?1 ORDER BY position").map_err(storage)?;
        let names = stmt.query_map([repository], |r| r.get::<_, String>(0)).map_err(storage)?;
        let names = names.collect::<Result<Vec<_>, _>>().map_err(storage)?;
        // Écrites validées : une liste illisible vaut « pas de réglage » plutôt qu'une erreur.
        Ok(if names.is_empty() { None } else { TrunkBranches::new(names).ok() })
    }

    /// Dépôts réglés, avec leurs troncs.
    pub fn all(&self) -> Result<Vec<(String, TrunkBranches)>, TrunkStorageError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT repository, branch FROM trunk_branch ORDER BY repository, position").map_err(storage)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(storage)?;
        let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
        for row in rows {
            let (repository, branch) = row.map_err(storage)?;
            match grouped.last_mut() {
                Some((last, names)) if *last == repository => names.push(branch),
                _ => grouped.push((repository, vec![branch])),
            }
        }
        Ok(grouped.into_iter().filter_map(|(r, names)| Some((r, TrunkBranches::new(names).ok()?))).collect())
    }

    /// Remplace ou retire (`None`) le réglage d'un dépôt.
    pub fn set(&self, repository: &str, trunks: Option<&TrunkBranches>) -> Result<(), TrunkStorageError> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(storage)?;
        tx.execute("DELETE FROM trunk_branch WHERE repository = ?1", [repository]).map_err(storage)?;
        for (position, branch) in trunks.map(TrunkBranches::as_slice).unwrap_or_default().iter().enumerate() {
            tx.execute(
                "INSERT INTO trunk_branch (repository, position, branch) VALUES (?1, ?2, ?3)",
                params![repository, position as i64, branch],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("robiade-trunks-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn each_repository_keeps_its_own_ordered_trunks() {
        let path = db("reglage");
        let trunks = SqliteTrunks::open(&path, false, || panic!("aucune reprise sur une base neuve")).unwrap();
        assert_eq!(trunks.get("/r/a").unwrap(), None);
        let staging = TrunkBranches::new(["staging", "master"]).unwrap();
        let main = TrunkBranches::new(["main"]).unwrap();
        trunks.set("/r/a", Some(&staging)).unwrap();
        trunks.set("/r/b", Some(&main)).unwrap();
        assert_eq!(trunks.get("/r/a").unwrap(), Some(staging.clone()));
        assert_eq!(trunks.all().unwrap(), [("/r/a".to_owned(), staging), ("/r/b".to_owned(), main.clone())]);
        trunks.set("/r/a", None).unwrap();
        assert_eq!(trunks.get("/r/a").unwrap(), None);
        assert_eq!(trunks.all().unwrap(), [("/r/b".to_owned(), main)]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_repositories_of_an_earlier_profile_keep_the_trunks_the_code_fixed() {
        let path = db("ancien");
        let legacy = TrunkBranches::new(["staging", "main", "master"]).unwrap();
        let trunks = SqliteTrunks::open(&path, true, || vec!["/r/a".into(), "/r/b".into()]).unwrap();
        assert_eq!(trunks.get("/r/b").unwrap(), Some(legacy));
        drop(trunks);
        let reopened = SqliteTrunks::open(&path, true, || panic!("reprise déjà faite")).unwrap();
        assert_eq!(reopened.all().unwrap().len(), 2);
        std::fs::remove_file(path).unwrap();
    }
}

//! Journal d'audit des actions sortantes dans `robiade.db` : quoi, quand, sur quelle cible, avec quel
//! résultat. Ni secret ni contenu (un commentaire n'y figure que par sa longueur).

use std::path::Path;
use std::sync::Mutex;

use robiade_core::merge_actions::{AuditEntry, AuditError, AuditLog};
use rusqlite::{params, Connection};

pub struct SqliteAuditLog {
    conn: Mutex<Connection>,
}

fn storage(e: impl std::fmt::Display) -> AuditError {
    AuditError(e.to_string())
}

impl SqliteAuditLog {
    pub fn open(path: &Path) -> Result<Self, AuditError> {
        let conn = Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS audit (
                 at INTEGER NOT NULL,
                 target TEXT NOT NULL,
                 action TEXT NOT NULL,
                 outcome TEXT NOT NULL
             );",
        )
        .map_err(storage)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl AuditLog for SqliteAuditLog {
    fn record(&self, e: &AuditEntry) -> Result<(), AuditError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute(
            "INSERT INTO audit (at, target, action, outcome) VALUES (?1, ?2, ?3, ?4)",
            params![e.at, e.target, e.action, e.outcome],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<AuditEntry>, AuditError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn
            .prepare("SELECT at, target, action, outcome FROM audit ORDER BY at DESC, rowid DESC LIMIT ?1")
            .map_err(storage)?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(AuditEntry { at: r.get(0)?, target: r.get(1)?, action: r.get(2)?, outcome: r.get(3)? })
            })
            .map_err(storage)?;
        rows.collect::<Result<_, _>>().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_actions_first() {
        let log = SqliteAuditLog::open(Path::new(":memory:")).unwrap();
        let entry = |at, outcome: &str| AuditEntry {
            at,
            target: "a/b!1".into(),
            action: "approuver".into(),
            outcome: outcome.into(),
        };
        log.record(&entry(1, "ok")).unwrap();
        log.record(&entry(2, "statut HTTP 401")).unwrap();
        assert_eq!(log.recent(10).unwrap(), [entry(2, "statut HTTP 401"), entry(1, "ok")]);
    }
}

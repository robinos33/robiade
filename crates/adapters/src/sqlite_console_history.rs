//! Historique de la console dans `robiade.db` : le texte des requêtes envoyées, jamais leurs
//! résultats.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::databases::DbError;
use robiade_core::db_console::{ConsoleHistory, HistoryEntry, HISTORY_MAX};
use rusqlite::params;

pub struct SqliteConsoleHistory {
    conn: Mutex<rusqlite::Connection>,
}

fn storage(e: impl std::fmt::Display) -> DbError {
    DbError::Storage(e.to_string())
}

impl SqliteConsoleHistory {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = rusqlite::Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS db_console_history (
                 id INTEGER PRIMARY KEY,
                 connection TEXT NOT NULL,
                 sql TEXT NOT NULL,
                 at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS db_console_history_by_connection ON db_console_history (connection, id);",
        )
        .map_err(storage)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl ConsoleHistory for SqliteConsoleHistory {
    fn record(&self, connection: &str, sql: &str, at: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute("INSERT INTO db_console_history (connection, sql, at) VALUES (?1, ?2, ?3)", params![connection, sql, at])
            .map_err(storage)?;
        conn.execute(
            "DELETE FROM db_console_history WHERE connection = ?1 AND id NOT IN
               (SELECT id FROM db_console_history WHERE connection = ?1 ORDER BY id DESC LIMIT ?2)",
            params![connection, HISTORY_MAX as i64],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn recent(&self, connection: &str) -> Result<Vec<HistoryEntry>, DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn
            .prepare("SELECT sql, at FROM db_console_history WHERE connection = ?1 ORDER BY id DESC")
            .map_err(storage)?;
        let rows = stmt.query_map([connection], |r| Ok(HistoryEntry { sql: r.get(0)?, at: r.get(1)? })).map_err(storage)?;
        rows.collect::<Result<_, _>>().map_err(storage)
    }

    fn forget(&self, connection: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute("DELETE FROM db_console_history WHERE connection = ?1", [connection]).map_err(storage)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_queries_of_each_connection() {
        let history = SqliteConsoleHistory::open(Path::new(":memory:")).unwrap();
        for i in 0..HISTORY_MAX + 5 {
            history.record("a", &format!("SELECT {i}"), i as i64).unwrap();
        }
        history.record("b", "SELECT 'b'", 0).unwrap();
        let recent = history.recent("a").unwrap();
        assert_eq!(recent.len(), HISTORY_MAX);
        assert_eq!(recent[0].sql, format!("SELECT {}", HISTORY_MAX + 4));
        history.forget("a").unwrap();
        assert!(history.recent("a").unwrap().is_empty());
        assert_eq!(history.recent("b").unwrap().len(), 1);
    }
}

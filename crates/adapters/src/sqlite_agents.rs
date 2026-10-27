//! Journal des onglets d'agents dans `robiade.db` : de quoi les rouvrir après un redémarrage.
//! Ni transcript ni sortie de terminal : seulement l'identité de l'onglet et son nom.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::agents::{Agent, AgentId, AgentJournal, JournalError};
use robiade_core::Millis;
use rusqlite::{params, Connection, Row};

pub struct SqliteAgentJournal {
    conn: Mutex<Connection>,
}

fn storage(e: impl std::fmt::Display) -> JournalError {
    JournalError(e.to_string())
}

impl SqliteAgentJournal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let conn = Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS agent (
                 id TEXT PRIMARY KEY,
                 project_path TEXT NOT NULL,
                 resumed_from TEXT,
                 name TEXT,
                 started_at INTEGER NOT NULL,
                 last_activity_at INTEGER NOT NULL
             );",
        )
        .map_err(storage)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn from_row(row: &Row) -> rusqlite::Result<Option<Agent>> {
        let Ok(id) = AgentId::parse(&row.get::<_, String>(0)?) else { return Ok(None) };
        Ok(Some(Agent::restore(id, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
    }
}

impl AgentJournal for SqliteAgentJournal {
    fn recent(&self, since: Millis) -> Result<Vec<Agent>, JournalError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_path, resumed_from, name, started_at, last_activity_at
                 FROM agent WHERE last_activity_at >= ?1 ORDER BY last_activity_at DESC",
            )
            .map_err(storage)?;
        let rows = stmt.query_map([since], Self::from_row).map_err(storage)?;
        let agents: Vec<Option<Agent>> = rows.collect::<Result<_, _>>().map_err(storage)?;
        Ok(agents.into_iter().flatten().collect())
    }

    fn save(&self, agent: &Agent) -> Result<(), JournalError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute(
            "INSERT INTO agent (id, project_path, resumed_from, name, started_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = ?4, last_activity_at = ?6",
            params![
                agent.id().as_str(),
                agent.project_path(),
                agent.resumed_from(),
                agent.name(),
                agent.started_at(),
                agent.last_activity_at()
            ],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn forget(&self, id: &AgentId) -> Result<(), JournalError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute("DELETE FROM agent WHERE id = ?1", [id.as_str()]).map_err(storage)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::agents::{AgentEvent, AgentStatus};

    #[test]
    fn reopens_named_tabs_as_ended_and_forgets_on_demand() {
        let journal = SqliteAgentJournal::open(Path::new(":memory:")).unwrap();
        let id = AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").unwrap();
        let mut agent = Agent::start(id.clone(), "/p".into(), Some("s-1".into()), 10);
        agent.rename("login");
        agent.apply(AgentEvent::PromptSubmitted, 50);
        journal.save(&agent).unwrap();

        let restored = journal.recent(40).unwrap();
        assert_eq!(restored.len(), 1);
        let tab = &restored[0];
        assert_eq!(tab.status(), AgentStatus::Ended);
        assert_eq!((tab.name(), tab.session_id(), tab.last_activity_at()), (Some("login"), "s-1", 50));
        assert!(journal.recent(60).unwrap().is_empty());

        journal.forget(&id).unwrap();
        assert!(journal.recent(0).unwrap().is_empty());
    }
}

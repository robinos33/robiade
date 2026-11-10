//! Journal des onglets d'agents dans `robiade.db` : de quoi les rouvrir après un redémarrage.
//! Ni transcript ni sortie de terminal : seulement l'identité de l'onglet et son nom.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::agents::{Agent, AgentId, AgentJournal, JournalError, LaunchChoice, Saved};
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
        // Colonnes ajoutées après la création de la table : ajoutées si elles manquent.
        for column in [
            "waited_ms INTEGER NOT NULL DEFAULT 0",
            "permission_ms INTEGER NOT NULL DEFAULT 0",
            "failed_turns INTEGER NOT NULL DEFAULT 0",
            "closed INTEGER NOT NULL DEFAULT 0",
            // Session en cours, qui suit `/clear` et `/resume` ; à défaut, `resumed_from` (onglets
            // enregistrés avant elle), puis l'identifiant de l'onglet.
            "session_id TEXT",
            // Modèle et effort choisis pour l'onglet, repassés à chaque reprise (issue #59).
            "model_choice TEXT",
            "effort_choice TEXT",
        ] {
            let name = column.split(' ').next().unwrap_or_default();
            let exists: bool = conn
                .query_row("SELECT count(*) FROM pragma_table_info('agent') WHERE name = ?1", [name], |r| r.get(0))
                .map_err(storage)?;
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE agent ADD COLUMN {column};")).map_err(storage)?;
            }
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn select(&self, filter: &str, since: Millis) -> Result<Vec<Agent>, JournalError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM agent WHERE last_activity_at >= ?1 {filter} ORDER BY last_activity_at DESC",
                COLUMNS.replacen("session_id", "coalesce(session_id, resumed_from)", 1)
            ))
            .map_err(storage)?;
        let rows = stmt.query_map([since], Self::from_row).map_err(storage)?;
        let agents: Vec<Option<Agent>> = rows.collect::<Result<_, _>>().map_err(storage)?;
        Ok(agents.into_iter().flatten().collect())
    }

    fn from_row(row: &Row) -> rusqlite::Result<Option<Agent>> {
        let Ok(id) = AgentId::parse(&row.get::<_, String>(0)?) else { return Ok(None) };
        let saved = Saved {
            name: row.get(3)?,
            started_at: row.get(4)?,
            last_activity_at: row.get(5)?,
            waited_ms: row.get(6)?,
            permission_ms: row.get(7)?,
            failed_turns: row.get(8)?,
            launch: LaunchChoice::restore(
                row.get::<_, Option<String>>(9)?.as_deref(),
                row.get::<_, Option<String>>(10)?.as_deref(),
            ),
        };
        Ok(Some(Agent::restore(id, row.get(1)?, row.get(2)?, saved)))
    }
}

const COLUMNS: &str = "id, project_path, session_id, name, started_at, last_activity_at, waited_ms, permission_ms, \
                       failed_turns, model_choice, effort_choice";

impl AgentJournal for SqliteAgentJournal {
    fn recent(&self, since: Millis) -> Result<Vec<Agent>, JournalError> {
        self.select("AND closed = 0", since)
    }

    fn since(&self, since: Millis) -> Result<Vec<Agent>, JournalError> {
        self.select("", since)
    }

    fn save(&self, agent: &Agent, now: Millis) -> Result<(), JournalError> {
        let saved = agent.saved(now);
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute(
            &format!(
                "INSERT INTO agent ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET session_id = ?3, name = ?4, last_activity_at = ?6,
                     waited_ms = ?7, permission_ms = ?8, failed_turns = ?9, model_choice = ?10,
                     effort_choice = ?11"
            ),
            params![
                agent.id().as_str(),
                agent.project_path(),
                agent.session_id(),
                saved.name,
                saved.started_at,
                saved.last_activity_at,
                saved.waited_ms,
                saved.permission_ms,
                saved.failed_turns,
                saved.launch.model(),
                saved.launch.effort()
            ],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn close(&self, id: &AgentId) -> Result<(), JournalError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute("UPDATE agent SET closed = 1 WHERE id = ?1", [id.as_str()]).map_err(storage)?;
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
        agent.apply(AgentEvent::TurnFailed, 60);
        journal.save(&agent, 70).unwrap();

        let restored = journal.recent(55).unwrap();
        assert_eq!(restored.len(), 1);
        let tab = &restored[0];
        assert_eq!(tab.status(), AgentStatus::Ended);
        assert_eq!((tab.name(), tab.session_id(), tab.last_activity_at()), (Some("login"), "s-1", 60));
        assert_eq!(tab.failed_turns(), 1);
        assert!(journal.recent(61).unwrap().is_empty());

        journal.close(&id).unwrap();
        assert!(journal.recent(0).unwrap().is_empty());
        assert_eq!(journal.since(0).unwrap().len(), 1);
    }

    #[test]
    fn reopens_a_tab_on_its_session_in_progress() {
        let journal = SqliteAgentJournal::open(Path::new(":memory:")).unwrap();
        let id = AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").unwrap();
        let mut agent = Agent::start(id, "/p".into(), None, 10);
        journal.save(&agent, 10).unwrap();
        agent.observe_session("9d259b93-5dc0-47c3-b1c8-3084e032dd44");
        journal.save(&agent, 20).unwrap();
        assert_eq!(journal.recent(0).unwrap()[0].session_id(), "9d259b93-5dc0-47c3-b1c8-3084e032dd44");
    }

    #[test]
    fn reopens_a_tab_with_its_model_and_effort() {
        let journal = SqliteAgentJournal::open(Path::new(":memory:")).unwrap();
        let id = AgentId::parse("0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10").unwrap();
        let mut agent = Agent::start(id, "/p".into(), None, 10);
        let launch = LaunchChoice::parse(Some("sonnet"), Some("low")).unwrap();
        agent.choose(launch.clone());
        journal.save(&agent, 10).unwrap();
        assert_eq!(journal.recent(0).unwrap()[0].launch(), &launch);
        agent.choose(LaunchChoice::default());
        journal.save(&agent, 20).unwrap();
        assert_eq!(journal.recent(0).unwrap()[0].launch(), &LaunchChoice::default());
    }

    #[test]
    fn a_tab_saved_before_the_session_column_reopens_on_the_session_it_resumed() {
        let journal = SqliteAgentJournal::open(Path::new(":memory:")).unwrap();
        journal
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO agent (id, project_path, resumed_from, started_at, last_activity_at) VALUES
                     ('0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10', '/p', '1b6f64dc-5a64-49af-aa54-67bf35b5afcc', 1, 2),
                     ('9d259b93-5dc0-47c3-b1c8-3084e032dd44', '/p', NULL, 1, 3);",
            )
            .unwrap();
        let sessions: Vec<String> = journal.recent(0).unwrap().iter().map(|a| a.session_id().to_owned()).collect();
        assert_eq!(sessions, ["9d259b93-5dc0-47c3-b1c8-3084e032dd44", "1b6f64dc-5a64-49af-aa54-67bf35b5afcc"]);
    }
}

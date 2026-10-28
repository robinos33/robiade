//! Connexions aux bases dans `robiade.db`. Rien de secret n'y est écrit : les champs ne portent
//! que des valeurs non sensibles ou des références (trousseau, Vault).

use std::path::Path;
use std::sync::Mutex;

use robiade_core::databases::{Connection, ConnectionId, ConnectionRepository, DbError, Engine, Part, Source, Stage, VaultRef};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

pub struct SqliteConnectionRepository {
    conn: Mutex<rusqlite::Connection>,
}

fn storage(e: impl std::fmt::Display) -> DbError {
    DbError::Storage(e.to_string())
}

const ENGINES: [(Engine, &str); 4] = [
    (Engine::Postgres, "postgres"),
    (Engine::MariaDb, "mariadb"),
    (Engine::Mongo, "mongo"),
    (Engine::Elasticsearch, "elasticsearch"),
];
const STAGES: [(Stage, &str); 3] = [(Stage::Dev, "dev"), (Stage::Staging, "staging"), (Stage::Prod, "prod")];
const PARTS: [(Part, &str); 6] = [
    (Part::Url, "url"),
    (Part::Host, "host"),
    (Part::Port, "port"),
    (Part::Database, "database"),
    (Part::User, "user"),
    (Part::Password, "password"),
];

fn name_of<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> &'static str {
    table.iter().find(|(v, _)| *v == value).map_or("", |(_, n)| n)
}

fn value_of<T: Copy>(table: &[(T, &'static str)], name: &str) -> Option<T> {
    table.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

fn fields_to_json(fields: &[(Part, Source)]) -> String {
    let list: Vec<Value> = fields
        .iter()
        .map(|(part, source)| {
            let part = name_of(&PARTS, *part);
            match source {
                Source::Literal(value) => json!({ "part": part, "literal": value }),
                Source::Keychain(service) => json!({ "part": part, "keychain": service }),
                Source::Vault(r) => json!({ "part": part, "vault": { "mount": r.mount, "path": r.path, "key": r.key } }),
            }
        })
        .collect();
    Value::Array(list).to_string()
}

fn fields_from_json(raw: &str) -> Vec<(Part, Source)> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let list: Vec<Value> = serde_json::from_str(raw).unwrap_or_default();
    list.iter()
        .filter_map(|f| {
            let part = value_of(&PARTS, f.get("part")?.as_str()?)?;
            let source = if let Some(value) = text(f, "literal") {
                Source::Literal(value)
            } else if let Some(service) = text(f, "keychain") {
                Source::Keychain(service)
            } else {
                let v = f.get("vault")?;
                Source::Vault(VaultRef { mount: text(v, "mount")?, path: text(v, "path")?, key: text(v, "key")? })
            };
            Some((part, source))
        })
        .collect()
}

impl SqliteConnectionRepository {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = rusqlite::Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS db_connection (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 engine TEXT NOT NULL,
                 stage TEXT NOT NULL CHECK (stage IN ('dev', 'staging', 'prod')),
                 project_path TEXT,
                 read_only INTEGER NOT NULL,
                 fields TEXT NOT NULL
             );",
        )
        .map_err(storage)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn from_row(row: &Row) -> rusqlite::Result<Option<Connection>> {
        let (Some(engine), Some(stage)) =
            (value_of(&ENGINES, &row.get::<_, String>(2)?), value_of(&STAGES, &row.get::<_, String>(3)?))
        else {
            return Ok(None);
        };
        Ok(Some(Connection::restore(
            ConnectionId(row.get(0)?),
            row.get(1)?,
            engine,
            stage,
            row.get(4)?,
            row.get(5)?,
            fields_from_json(&row.get::<_, String>(6)?),
        )))
    }
}

const COLUMNS: &str = "id, name, engine, stage, project_path, read_only, fields";

impl ConnectionRepository for SqliteConnectionRepository {
    fn all(&self) -> Result<Vec<Connection>, DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM db_connection ORDER BY name")).map_err(storage)?;
        let rows = stmt.query_map([], Self::from_row).map_err(storage)?;
        Ok(rows.collect::<Result<Vec<_>, _>>().map_err(storage)?.into_iter().flatten().collect())
    }

    fn get(&self, id: &ConnectionId) -> Result<Connection, DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.query_row(&format!("SELECT {COLUMNS} FROM db_connection WHERE id = ?1"), [&id.0], Self::from_row)
            .optional()
            .map_err(storage)?
            .flatten()
            .ok_or(DbError::NotFound)
    }

    fn save(&self, c: &Connection) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        conn.execute(
            &format!(
                "INSERT INTO db_connection ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, engine = ?3, stage = ?4, project_path = ?5,
                     read_only = ?6, fields = ?7"
            ),
            params![
                c.id().0,
                c.name(),
                name_of(&ENGINES, c.engine()),
                name_of(&STAGES, c.stage()),
                c.project_path(),
                c.read_only(),
                fields_to_json(c.fields()),
            ],
        )
        .map_err(storage)?;
        Ok(())
    }

    fn delete(&self, id: &ConnectionId) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(storage)?;
        match conn.execute("DELETE FROM db_connection WHERE id = ?1", [&id.0]).map_err(storage)? {
            0 => Err(DbError::NotFound),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_connection_with_every_kind_of_source() {
        let repo = SqliteConnectionRepository::open(Path::new(":memory:")).unwrap();
        let fields = vec![
            (Part::Url, Source::Vault(VaultRef { mount: "forum-kv".into(), path: "internal/prod/DATABASE".into(), key: "URL".into() })),
            (Part::User, Source::Literal("lecteur".into())),
            (Part::Password, Source::Keychain("pg-forum-ro".into())),
        ];
        let id = ConnectionId("a".into());
        let c = Connection::create(id.clone(), "Forum prod", Engine::Postgres, Stage::Prod, Some("/p".into()), true, fields).unwrap();
        repo.save(&c).unwrap();
        assert_eq!(repo.get(&id).unwrap(), c);
        assert_eq!(repo.all().unwrap().len(), 1);
        repo.delete(&id).unwrap();
        assert_eq!(repo.get(&id), Err(DbError::NotFound));
    }

    #[test]
    fn a_prod_row_tampered_to_read_write_comes_back_read_only() {
        let repo = SqliteConnectionRepository::open(Path::new(":memory:")).unwrap();
        repo.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO db_connection VALUES ('x', 'Forum', 'postgres', 'prod', NULL, 0, '[]')",
                [],
            )
            .unwrap();
        assert!(repo.get(&ConnectionId("x".into())).unwrap().read_only());
    }
}

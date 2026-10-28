//! Sessions Postgres. La lecture seule est posée à l'ouverture de la session, par le paramètre de
//! démarrage `default_transaction_read_only`, avant toute requête.

use std::time::Duration;

use postgres::config::SslMode;
use postgres_native_tls::MakeTlsConnector;
use postgres::types::ToSql;
use postgres::Client;
use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};
use postgres::SimpleQueryMessage;
use robiade_core::db_browse::{
    Canceller, Column, DatabaseBrowser, DbSession, PageQuery, Relation, RelationKind, TableRef, TableShape,
};
use robiade_core::db_console::{Executed, StatementKind};

pub struct Postgres;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Délai d'une requête de parcours : une page sur une grosse table sans index peut être lente,
/// mais pas au point d'occuper le serveur indéfiniment.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Le chiffrement décidé par le domaine. Le connecteur TLS est celui du système, avec ses réglages
/// par défaut : certificat et nom d'hôte vérifiés, aucune exception (`require` vaut `verify-full`).
fn ssl_mode(tls: Tls) -> SslMode {
    match tls {
        Tls::Required => SslMode::Require,
        Tls::Preferred => SslMode::Prefer,
        Tls::Disabled => SslMode::Disable,
    }
}

/// Paramètres de session envoyés au démarrage : lecture seule demandée, et une requête qui
/// traînerait est coupée par le serveur.
fn options(read_only: bool, timeout: Duration) -> String {
    let mut options = format!("-c statement_timeout={}", timeout.as_millis());
    if read_only {
        options.push_str(" -c default_transaction_read_only=on");
    }
    options
}

fn unreachable(e: postgres::Error) -> DbError {
    // `as_db_error` porte le message du serveur, plus parlant que l'enveloppe du client.
    let message = e.as_db_error().map_or_else(|| e.to_string(), |db| db.message().to_owned());
    DbError::Unreachable(message)
}

fn connect(engine: Engine, target: &Target, tls: Tls, read_only: bool, timeout: Duration) -> Result<Client, DbError> {
    if engine != Engine::Postgres {
        return Err(DbError::Unsupported("ce moteur"));
    }
    let mut config = postgres::Config::new();
    config
        .host(&target.host)
        .port(target.port)
        .ssl_mode(ssl_mode(tls))
        .connect_timeout(TIMEOUT)
        .application_name("robiade")
        .options(&options(read_only, timeout));
    if let Some(database) = &target.database {
        config.dbname(database);
    }
    if let Some(user) = &target.user {
        config.user(user);
    }
    if let Some(password) = &target.password {
        config.password(password);
    }
    let tls = native_tls::TlsConnector::new().map_err(|e| DbError::Unreachable(e.to_string()))?;
    config.connect(MakeTlsConnector::new(tls)).map_err(unreachable)
}

impl DatabaseProbe for Postgres {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Probe, DbError> {
        let mut client = connect(engine, target, tls, read_only, TIMEOUT)?;
        let row = client
            .query_one(
                "SELECT current_setting('server_version'), current_user::text, current_database()::text,
                        current_setting('transaction_read_only')",
                &[],
            )
            .map_err(unreachable)?;
        let probe = Probe {
            server_version: row.get(0),
            user: row.get(1),
            database: row.get(2),
            read_only: row.get::<_, String>(3) == "on",
        };
        let _ = client.close();
        Ok(probe)
    }
}

impl DatabaseBrowser for Postgres {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError> {
        Ok(Box::new(PgSession(connect(engine, target, tls, read_only, BROWSE_TIMEOUT)?)))
    }
}

/// Une session de parcours. Elle se ferme quand elle est lâchée.
struct PgSession(Client);

/// `relkind` → nature de la relation ; une table partitionnée se lit comme une table.
fn relation_kind(relkind: &str) -> Option<RelationKind> {
    match relkind {
        "r" | "p" => Some(RelationKind::Table),
        "v" => Some(RelationKind::View),
        "m" => Some(RelationKind::MaterializedView),
        "f" => Some(RelationKind::Foreign),
        _ => None,
    }
}

impl DbSession for PgSession {
    fn read_only(&mut self) -> Result<bool, DbError> {
        let row = self.0.query_one("SELECT current_setting('transaction_read_only')", &[]).map_err(unreachable)?;
        Ok(row.get::<_, String>(0) == "on")
    }

    fn current_database(&mut self) -> Result<String, DbError> {
        Ok(self.0.query_one("SELECT current_database()::text", &[]).map_err(unreachable)?.get(0))
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        let rows = self
            .0
            .query(
                "SELECT datname::text FROM pg_database
                 WHERE datallowconn AND NOT datistemplate AND has_database_privilege(datname, 'CONNECT')
                 ORDER BY 1",
                &[],
            )
            .map_err(unreachable)?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    fn relations(&mut self) -> Result<Vec<Relation>, DbError> {
        // Les partitions se lisent par leur table mère ; seules les relations lisibles sont listées.
        let rows = self
            .0
            .query(
                "SELECT n.nspname::text, c.relname::text, c.relkind::text, c.reltuples::float8
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND NOT c.relispartition
                   AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                   AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%'
                   AND has_table_privilege(c.oid, 'SELECT')
                 ORDER BY 1, 2",
                &[],
            )
            .map_err(unreachable)?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let estimate: f64 = r.get(3);
                Some(Relation {
                    table: TableRef { schema: r.get(0), name: r.get(1) },
                    kind: relation_kind(r.get(2))?,
                    // -1 : jamais analysée (PostgreSQL 14 et plus).
                    estimated_rows: (estimate >= 0.0).then_some(estimate as i64),
                })
            })
            .collect())
    }

    fn describe(&mut self, table: &TableRef) -> Result<TableShape, DbError> {
        let rows = self
            .0
            .query(
                "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod),
                        (SELECT k.rank FROM pg_index i, unnest(i.indkey) WITH ORDINALITY k(attnum, rank)
                         WHERE i.indrelid = c.oid AND i.indisprimary AND k.attnum = a.attnum)::int4
                 FROM pg_attribute a
                 JOIN pg_class c ON c.oid = a.attrelid
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
                 ORDER BY a.attnum",
                &[&table.schema, &table.name],
            )
            .map_err(unreachable)?;
        if rows.is_empty() {
            return Err(DbError::UnknownColumn(format!("{}.{}", table.schema, table.name)));
        }
        let columns = rows
            .iter()
            .map(|r| Column {
                name: r.get(0),
                data_type: r.get(1),
                primary_key: r.get::<_, Option<i32>>(2).and_then(|p| u16::try_from(p).ok()),
            })
            .collect();
        Ok(TableShape { table: table.clone(), columns })
    }

    fn fetch(&mut self, query: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let params: Vec<&(dyn ToSql + Sync)> = query.params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
        let rows = self.0.query(query.sql.as_str(), &params).map_err(unreachable)?;
        Ok(rows.iter().map(|r| (0..r.len()).map(|i| r.get::<_, Option<String>>(i)).collect()).collect())
    }

    fn execute(&mut self, statement: &str, kind: StatementKind, max_rows: usize) -> Result<Executed, DbError> {
        // Analysée sans être exécutée : le serveur refuse plusieurs instructions dans une requête
        // préparée, ce qui double le découpage du domaine. Donne aussi le type de chaque colonne.
        let prepared = self.0.prepare(statement).map_err(unreachable)?;
        let types: Vec<String> = prepared.columns().iter().map(|c| c.type_().name().to_owned()).collect();
        let messages = if kind == StatementKind::Query {
            // Un curseur borne ce qui est lu : un SELECT sur une grosse table ne charge que
            // `max_rows` lignes. Le texte est celui que `prepare` vient d'accepter, seul.
            self.0.batch_execute("BEGIN").map_err(unreachable)?;
            let fetched = self
                .0
                .batch_execute(&format!("DECLARE robiade_console NO SCROLL CURSOR FOR {statement}"))
                .and_then(|()| self.0.simple_query(&format!("FETCH {} FROM robiade_console", max_rows + 1)));
            let end = if fetched.is_ok() { "COMMIT" } else { "ROLLBACK" };
            let ended = self.0.batch_execute(end);
            let messages = fetched.map_err(unreachable)?;
            ended.map_err(unreachable)?;
            messages
        } else {
            self.0.simple_query(statement).map_err(unreachable)?
        };
        Ok(executed(messages, &types, kind, max_rows))
    }

    fn canceller(&self) -> Canceller {
        let token = self.0.cancel_token();
        Box::new(move || {
            // Même chiffrement que la session : le jeton garde son `sslmode`.
            let tls = native_tls::TlsConnector::new().map_err(|e| DbError::Unreachable(e.to_string()))?;
            token.cancel_query(MakeTlsConnector::new(tls)).map_err(unreachable)
        })
    }
}

/// Réponse du protocole simple, en texte : NULL reste `None`, distinct de la chaîne vide.
fn executed(messages: Vec<SimpleQueryMessage>, types: &[String], kind: StatementKind, max_rows: usize) -> Executed {
    let mut out = Executed::default();
    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                out.columns = columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Column {
                        name: c.name().to_owned(),
                        data_type: types.get(i).cloned().unwrap_or_default(),
                        primary_key: None,
                    })
                    .collect();
            }
            SimpleQueryMessage::Row(row) if out.rows.len() <= max_rows => {
                out.rows.push((0..row.len()).map(|i| row.get(i).map(str::to_owned)).collect());
            }
            // Pour un curseur, c'est le nombre de lignes lues, pas touchées.
            SimpleQueryMessage::CommandComplete(n) if kind != StatementKind::Query => out.affected = Some(n),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_tls_never_falls_back_to_clear_text() {
        assert!(matches!(ssl_mode(Tls::Required), SslMode::Require));
        assert!(matches!(ssl_mode(Tls::Preferred), SslMode::Prefer));
        assert!(matches!(ssl_mode(Tls::Disabled), SslMode::Disable));
    }

    /// Contre un Postgres jetable : `ROBIADE_PG_TEST_PORT` et `ROBIADE_PG_TEST_PASSWORD` (utilisateur
    /// `postgres`, sur localhost), avec le schéma `app` décrit dans le message du commit qui l'ajoute.
    #[test]
    #[ignore = "demande un Postgres local jetable"]
    fn browses_a_real_server_read_only() {
        use robiade_core::db_browse::{page, page_query, Cursor, Filter, FilterOp, PageRequest};

        let target = Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_PG_TEST_PORT").unwrap().parse().unwrap(),
            database: None,
            user: Some("postgres".into()),
            password: Some(std::env::var("ROBIADE_PG_TEST_PASSWORD").unwrap()),
            ssl_mode: None,
        };
        let mut session = Postgres.open(Engine::Postgres, &target, Tls::Disabled, true).unwrap();
        assert!(session.read_only().unwrap());
        assert!(session.databases().unwrap().contains(&"postgres".to_owned()));
        let relations = session.relations().unwrap();
        let user = relations.iter().find(|r| r.table.name == "user").unwrap();
        assert_eq!((user.kind, user.estimated_rows), (RelationKind::Table, Some(453)));
        assert!(relations.iter().any(|r| r.table.name == "recent" && r.kind == RelationKind::View));

        let shape = session.describe(&user.table).unwrap();
        assert_eq!(shape.columns[0].primary_key, Some(1));
        let mut cursor = Cursor::Start;
        let mut seen = 0;
        loop {
            let request = PageRequest { sort: None, filters: vec![], cursor };
            let query = page_query(&shape, &request).unwrap();
            let p = page(&shape, &query, session.fetch(&query).unwrap());
            seen += p.rows.len();
            match p.next {
                Some(next) => cursor = next,
                None => break,
            }
        }
        assert_eq!(seen, 453);

        let filter = |column: &str, op| PageRequest { sort: None, filters: vec![Filter { column: column.into(), op }], cursor: Cursor::Start };
        let fetch = |session: &mut Box<dyn DbSession>, request: &PageRequest| {
            let query = page_query(&shape, request).unwrap();
            page(&shape, &query, session.fetch(&query).unwrap())
        };
        assert_eq!(fetch(&mut session, &filter("email", FilterOp::Contains("50%_".into()))).rows.len(), 1);
        assert_eq!(fetch(&mut session, &filter("id", FilterOp::Equals("3".into()))).rows[0][3].as_ref().unwrap().text, "<2 octets>");
        let empty = fetch(&mut session, &filter("email", FilterOp::Equals(String::new())));
        assert_eq!((empty.rows.len(), empty.rows[0][1].as_ref().map(|c| c.text.as_str())), (1, Some("")));
        assert_eq!(fetch(&mut session, &filter("email", FilterOp::IsNull)).rows[0][1], None);

        // Clé composite (jour, rang) et valeurs longues tronquées.
        let event = session.describe(&TableRef { schema: "app".into(), name: "event".into() }).unwrap();
        let query = page_query(&event, &PageRequest { sort: None, filters: vec![], cursor: Cursor::Start }).unwrap();
        let first = page(&event, &query, session.fetch(&query).unwrap());
        assert!(first.rows[0][2].as_ref().unwrap().truncated);
        let Some(Cursor::After(key)) = first.next else { panic!("keyset attendu") };
        let query = page_query(&event, &PageRequest { sort: None, filters: vec![], cursor: Cursor::After(key) }).unwrap();
        assert_eq!(page(&event, &query, session.fetch(&query).unwrap()).rows.len(), 51);

        // Lecture seule tenue par le serveur, pour la même session que celle du parcours.
        let mut client = connect(Engine::Postgres, &target, Tls::Disabled, true, BROWSE_TIMEOUT).unwrap();
        let refused = client.execute("INSERT INTO app.event VALUES (current_date, 99, 'x')", &[]).unwrap_err();
        assert!(refused.to_string().contains("read-only") || format!("{refused:?}").contains("25006"));
    }

    /// Contre le même Postgres jetable que `browses_a_real_server_read_only`, sans schéma requis.
    #[test]
    #[ignore = "demande un Postgres local jetable"]
    fn runs_console_statements_and_cancels_a_long_one() {
        let target = Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_PG_TEST_PORT").unwrap().parse().unwrap(),
            database: None,
            user: Some("postgres".into()),
            password: Some(std::env::var("ROBIADE_PG_TEST_PASSWORD").unwrap()),
            ssl_mode: None,
        };
        let mut session = Postgres.open(Engine::Postgres, &target, Tls::Disabled, true).unwrap();

        // Un SELECT ne lit pas plus que demandé, une de plus pour savoir qu'il y a une suite.
        let read = session.execute("SELECT g, NULL::text AS n, '' AS e FROM generate_series(1, 5000) g", StatementKind::Query, 1000).unwrap();
        assert_eq!(read.rows.len(), 1001);
        assert_eq!((read.columns[0].name.as_str(), read.columns[0].data_type.as_str()), ("g", "int4"));
        assert_eq!((read.rows[0][1].clone(), read.rows[0][2].clone()), (None, Some(String::new())));
        let show = session.execute("SHOW transaction_read_only", StatementKind::Read, 1000).unwrap();
        assert_eq!(show.rows[0][0].as_deref(), Some("on"));

        // Deux instructions dans un même envoi : refusé par le serveur avant toute exécution.
        assert!(session.execute("SELECT 1; SELECT 2", StatementKind::Query, 10).is_err());
        // Une écriture, ou une tentative de lever le verrou, se heurte au serveur.
        let refused = session.execute("CREATE TABLE robiade_x (a int)", StatementKind::Write, 10).unwrap_err();
        assert!(refused.to_string().contains("read-only"), "{refused}");
        // Le serveur accepte qu'on lève la lecture seule de la session : c'est pourquoi le domaine
        // le refuse avant envoi, et l'application relit `read_only()` après chaque instruction.
        session.execute("SET default_transaction_read_only = off", StatementKind::Write, 10).unwrap();
        assert!(!session.read_only().unwrap());
        let mut session = Postgres.open(Engine::Postgres, &target, Tls::Disabled, true).unwrap();

        // Annulée depuis un autre fil, la requête s'arrête et la session reste utilisable.
        let cancel = session.canceller();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            cancel().unwrap();
        });
        let started = std::time::Instant::now();
        let cancelled = session.execute("SELECT pg_sleep(20)", StatementKind::Query, 10).unwrap_err();
        canceller.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(cancelled.to_string().contains("cancel"), "{cancelled}");
        assert_eq!(session.execute("SELECT 1", StatementKind::Query, 10).unwrap().rows.len(), 1);

        // Sur une session qui peut écrire : le nombre de lignes touchées.
        let mut writable = Postgres.open(Engine::Postgres, &target, Tls::Disabled, false).unwrap();
        writable.execute("CREATE TEMP TABLE t (a int)", StatementKind::Write, 10).unwrap();
        let inserted = writable.execute("INSERT INTO t VALUES (1), (2)", StatementKind::Write, 10).unwrap();
        assert_eq!(inserted.affected, Some(2));
    }

    #[test]
    fn asks_for_read_only_at_session_start() {
        assert_eq!(options(true, TIMEOUT), "-c statement_timeout=10000 -c default_transaction_read_only=on");
        assert!(!options(false, BROWSE_TIMEOUT).contains("read_only"));
    }
}

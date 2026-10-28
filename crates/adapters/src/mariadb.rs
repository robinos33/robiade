//! Sessions MariaDB. La lecture seule est posée à l'ouverture (`SET SESSION TRANSACTION READ
//! ONLY`), avant toute requête, avec un `sql_mode` qui lit les chaînes comme la console du domaine.

use std::time::Duration;

use mysql::consts::ColumnType;
use mysql::prelude::Queryable;
use mysql::{Conn, Opts, OptsBuilder, Params, SslOpts, Value};
use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};
use robiade_core::db_browse::{
    Canceller, Column, DatabaseBrowser, DbSession, PageQuery, Relation, RelationKind, TableRef, TableShape,
};
use robiade_core::db_console::{Executed, StatementKind};

pub struct MariaDb;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Délai d'une requête de parcours ou de console, comme pour Postgres.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Jeu de caractères binaire : la valeur n'est pas du texte.
const BINARY_CHARSET: u16 = 63;
/// Bases du serveur lui-même, pas de l'application.
const SYSTEM_DATABASES: &[&str] = &["information_schema", "performance_schema", "mysql", "sys"];
/// Modes qui changent la lecture d'une chaîne : `"…"` devenu identifiant, `\` qui n'échappe plus.
/// Les modes composés (`ANSI`, `ORACLE`…) incluent `ANSI_QUOTES`.
const MISREAD_MODES: &[&str] = &["ANSI_QUOTES", "NO_BACKSLASH_ESCAPES", "ANSI", "DB2", "MAXDB", "MSSQL", "ORACLE", "POSTGRESQL"];

fn unreachable(e: mysql::Error) -> DbError {
    // Le message du serveur, sans le code d'erreur numérique qui le précède.
    let message = match &e {
        mysql::Error::MySqlError(server) => server.message.clone(),
        _ => e.to_string(),
    };
    DbError::Unreachable(message)
}

/// Paramètres de session envoyés à chaque connexion, avant toute requête : texte en utf8mb4,
/// requête trop longue coupée par le serveur, et lecture seule si demandée.
fn init(read_only: bool, timeout: Duration) -> Vec<String> {
    let mut init = vec!["SET NAMES utf8mb4".to_owned(), format!("SET SESSION max_statement_time = {}", timeout.as_secs())];
    if read_only {
        init.push("SET SESSION TRANSACTION READ ONLY".to_owned());
    }
    init
}

fn options(target: &Target, tls: Option<SslOpts>, read_only: bool, timeout: Duration) -> Opts {
    OptsBuilder::new()
        .ip_or_hostname(Some(&target.host))
        .tcp_port(target.port)
        .user(target.user.as_ref())
        .pass(target.password.as_ref())
        .db_name(target.database.as_ref())
        // Sinon, vers localhost, le client bascule de lui-même sur le socket Unix du serveur.
        .prefer_socket(false)
        .tcp_connect_timeout(Some(TIMEOUT))
        // Certificat et nom d'hôte vérifiés : le client n'est jamais mis en mode « dangereux ».
        .ssl_opts(tls)
        .init(init(read_only, timeout))
        .into()
}

/// `Preferred` (dev seulement) : TLS d'abord, en clair si le serveur ne le propose pas ou que son
/// certificat ne se vérifie pas, comme un MariaDB local et son certificat autosigné.
fn connect(engine: Engine, target: &Target, tls: Tls, read_only: bool, timeout: Duration) -> Result<(Conn, Opts), DbError> {
    if engine != Engine::MariaDb {
        return Err(DbError::Unsupported("ce moteur"));
    }
    let attempt = |ssl: Option<SslOpts>| {
        let opts = options(target, ssl, read_only, timeout);
        Conn::new(opts.clone()).map(|conn| (conn, opts))
    };
    let (mut conn, opts) = match tls {
        Tls::Required => attempt(Some(SslOpts::default())),
        Tls::Preferred => attempt(Some(SslOpts::default())).or_else(|_| attempt(None)),
        Tls::Disabled => attempt(None),
    }
    .map_err(unreachable)?;
    readable_sql_mode(&mut conn)?;
    Ok((conn, opts))
}

/// Retire de `sql_mode` ce qui ferait lire une chaîne autrement que la console, puis le vérifie.
fn readable_sql_mode(conn: &mut Conn) -> Result<(), DbError> {
    let mode: String = conn.query_first("SELECT @@SESSION.sql_mode").map_err(unreachable)?.unwrap_or_default();
    let kept: Vec<&str> = mode.split(',').filter(|m| !m.is_empty() && !MISREAD_MODES.contains(m)).collect();
    if kept.len() != mode.split(',').filter(|m| !m.is_empty()).count() {
        conn.exec_drop("SET SESSION sql_mode = ?", (kept.join(","),)).map_err(unreachable)?;
    }
    let mode: String = conn.query_first("SELECT @@SESSION.sql_mode").map_err(unreachable)?.unwrap_or_default();
    match mode.split(',').find(|m| MISREAD_MODES.contains(m)) {
        Some(m) => Err(DbError::Unreachable(format!("sql_mode {m} toujours actif : la console lirait mal le texte"))),
        None => Ok(()),
    }
}

fn read_only(conn: &mut Conn) -> Result<bool, DbError> {
    let flag: Option<i64> = conn.query_first("SELECT @@SESSION.tx_read_only").map_err(unreachable)?;
    Ok(flag == Some(1))
}

impl DatabaseProbe for MariaDb {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Probe, DbError> {
        let (mut conn, _) = connect(engine, target, tls, read_only, TIMEOUT)?;
        let row: Option<(String, String, Option<String>, i64)> = conn
            .query_first("SELECT VERSION(), CURRENT_USER(), DATABASE(), @@SESSION.tx_read_only")
            .map_err(unreachable)?;
        let (server_version, user, database, flag) = row.ok_or_else(|| DbError::Unreachable("réponse vide".into()))?;
        Ok(Probe { server_version, user, database: database.unwrap_or_default(), read_only: flag == 1 })
    }
}

impl DatabaseBrowser for MariaDb {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError> {
        let (conn, opts) = connect(engine, target, tls, read_only, BROWSE_TIMEOUT)?;
        Ok(Box::new(MariaSession { conn, opts }))
    }
}

/// Une session de parcours. Elle se ferme quand elle est lâchée ; `opts` sert à en ouvrir une
/// seconde, le temps d'interrompre la première.
struct MariaSession {
    conn: Conn,
    opts: Opts,
}

/// `TABLE_TYPE` → nature de la relation ; une séquence n'a pas de lignes à parcourir.
fn relation_kind(table_type: &str) -> Option<RelationKind> {
    match table_type {
        "BASE TABLE" | "SYSTEM VERSIONED" => Some(RelationKind::Table),
        "VIEW" => Some(RelationKind::View),
        _ => None,
    }
}

/// Nom du type d'une colonne de résultat, tel qu'un `COLUMN_TYPE` le dirait sans ses arguments.
fn type_name(column: &mysql::Column) -> String {
    let binary = column.character_set() == BINARY_CHARSET;
    let name = match column.column_type() {
        ColumnType::MYSQL_TYPE_TINY => "tinyint",
        ColumnType::MYSQL_TYPE_SHORT => "smallint",
        ColumnType::MYSQL_TYPE_INT24 => "mediumint",
        ColumnType::MYSQL_TYPE_LONG => "int",
        ColumnType::MYSQL_TYPE_LONGLONG => "bigint",
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "decimal",
        ColumnType::MYSQL_TYPE_FLOAT => "float",
        ColumnType::MYSQL_TYPE_DOUBLE => "double",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "datetime",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "timestamp",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "time",
        ColumnType::MYSQL_TYPE_YEAR => "year",
        ColumnType::MYSQL_TYPE_BIT => "bit",
        ColumnType::MYSQL_TYPE_JSON => "json",
        ColumnType::MYSQL_TYPE_GEOMETRY => "geometry",
        ColumnType::MYSQL_TYPE_NULL => "null",
        ColumnType::MYSQL_TYPE_STRING if binary => "binary",
        ColumnType::MYSQL_TYPE_STRING => "char",
        ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR if binary => "varbinary",
        ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR => "varchar",
        _ if binary => "blob",
        _ => "text",
    };
    name.to_owned()
}

/// Une valeur en texte, `None` pour NULL. Un binaire est résumé à sa taille.
fn text(value: Value, column: &mysql::Column) -> Option<String> {
    let date_only = matches!(column.column_type(), ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE);
    match value {
        Value::NULL => None,
        Value::Bytes(bytes) if column.character_set() == BINARY_CHARSET && !is_textual(column) => {
            Some(format!("<{} octets>", bytes.len()))
        }
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Value::Int(n) => Some(n.to_string()),
        Value::UInt(n) => Some(n.to_string()),
        Value::Float(n) => Some(n.to_string()),
        Value::Double(n) => Some(n.to_string()),
        Value::Date(y, m, d, ..) if date_only => Some(format!("{y:04}-{m:02}-{d:02}")),
        Value::Date(y, m, d, h, i, s, us) => {
            let fraction = if us > 0 { format!(".{us:06}") } else { String::new() };
            Some(format!("{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}{fraction}"))
        }
        Value::Time(negative, days, h, i, s, us) => {
            let sign = if negative { "-" } else { "" };
            let fraction = if us > 0 { format!(".{us:06}") } else { String::new() };
            Some(format!("{sign}{:02}:{i:02}:{s:02}{fraction}", days * 24 + u32::from(h)))
        }
    }
}

/// Types dont le jeu de caractères est binaire sans que la valeur soit opaque : nombres, dates,
/// rendus en texte par le protocole binaire.
fn is_textual(column: &mysql::Column) -> bool {
    !matches!(
        column.column_type(),
        ColumnType::MYSQL_TYPE_STRING
            | ColumnType::MYSQL_TYPE_VAR_STRING
            | ColumnType::MYSQL_TYPE_VARCHAR
            | ColumnType::MYSQL_TYPE_BLOB
            | ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_GEOMETRY
            | ColumnType::MYSQL_TYPE_BIT
    )
}

impl MariaSession {
    /// Une instruction préparée : le serveur refuse d'en préparer plusieurs à la fois, ce qui double
    /// le découpage du domaine. Lit au plus `max_rows` lignes ; le reste est lu et jeté.
    fn run(&mut self, statement: &str, max_rows: usize) -> Result<Executed, DbError> {
        let prepared = self.conn.prep(statement).map_err(unreachable)?;
        let mut result = self.conn.exec_iter(&prepared, ()).map_err(unreachable)?;
        let columns: Vec<mysql::Column> = result.columns().as_ref().to_vec();
        let mut out = Executed {
            columns: columns.iter().map(|c| Column { name: c.name_str().into_owned(), data_type: type_name(c), primary_key: None }).collect(),
            rows: Vec::new(),
            affected: None,
        };
        if columns.is_empty() {
            out.affected = Some(result.affected_rows());
            return Ok(out);
        }
        for row in result.by_ref() {
            let row = row.map_err(unreachable)?;
            if out.rows.len() <= max_rows {
                out.rows.push(row.unwrap().into_iter().zip(&columns).map(|(v, c)| text(v, c)).collect());
            }
        }
        Ok(out)
    }
}

impl DbSession for MariaSession {
    fn read_only(&mut self) -> Result<bool, DbError> {
        read_only(&mut self.conn)
    }

    fn current_database(&mut self) -> Result<String, DbError> {
        let database: Option<Option<String>> = self.conn.query_first("SELECT DATABASE()").map_err(unreachable)?;
        Ok(database.flatten().unwrap_or_default())
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        // Seules celles sur lesquelles le compte a un droit sont listées par le serveur.
        let names: Vec<String> = self.conn.query("SHOW DATABASES").map_err(unreachable)?;
        let mut names: Vec<String> = names.into_iter().filter(|n| !SYSTEM_DATABASES.contains(&n.as_str())).collect();
        names.sort();
        Ok(names)
    }

    fn relations(&mut self) -> Result<Vec<Relation>, DbError> {
        // `TABLE_ROWS` est l'estimation d'InnoDB, NULL pour une vue.
        let rows: Vec<(String, String, String, Option<u64>)> = self
            .conn
            .query(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, TABLE_ROWS FROM information_schema.TABLES
                 WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME",
            )
            .map_err(unreachable)?;
        Ok(rows
            .into_iter()
            .filter_map(|(schema, name, table_type, estimate)| {
                Some(Relation {
                    table: TableRef { schema, name },
                    kind: relation_kind(&table_type)?,
                    estimated_rows: estimate.and_then(|n| i64::try_from(n).ok()),
                })
            })
            .collect())
    }

    fn describe(&mut self, table: &TableRef) -> Result<TableShape, DbError> {
        // `BINARY` : le catalogue compare sans tenir compte de la casse, deux tables `User` et
        // `user` y répondraient ensemble.
        let rows: Vec<(String, String, Option<u32>)> = self
            .conn
            .exec(
                "SELECT c.COLUMN_NAME, c.COLUMN_TYPE, s.SEQ_IN_INDEX
                 FROM information_schema.COLUMNS c
                 LEFT JOIN information_schema.STATISTICS s
                   ON s.TABLE_SCHEMA = c.TABLE_SCHEMA AND s.TABLE_NAME = c.TABLE_NAME
                  AND s.COLUMN_NAME = c.COLUMN_NAME AND s.INDEX_NAME = 'PRIMARY'
                 WHERE BINARY c.TABLE_SCHEMA = ? AND BINARY c.TABLE_NAME = ?
                 ORDER BY c.ORDINAL_POSITION",
                (&table.schema, &table.name),
            )
            .map_err(unreachable)?;
        if rows.is_empty() {
            return Err(DbError::UnknownColumn(format!("{}.{}", table.schema, table.name)));
        }
        let columns = rows
            .into_iter()
            .map(|(name, data_type, rank)| Column { name, data_type, primary_key: rank.and_then(|r| u16::try_from(r).ok()) })
            .collect();
        Ok(TableShape { table: table.clone(), columns })
    }

    fn fetch(&mut self, query: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let params = Params::Positional(query.params.iter().map(|p| Value::from(p.as_str())).collect());
        let result = self.conn.exec_iter(query.sql.as_str(), params).map_err(unreachable)?;
        let columns: Vec<mysql::Column> = result.columns().as_ref().to_vec();
        result
            .map(|row| Ok(row.map_err(unreachable)?.unwrap().into_iter().zip(&columns).map(|(v, c)| text(v, c)).collect()))
            .collect()
    }

    fn execute(&mut self, statement: &str, kind: StatementKind, max_rows: usize) -> Result<Executed, DbError> {
        if kind != StatementKind::Query {
            return self.run(statement, max_rows);
        }
        // Le serveur s'arrête de lui-même après `max_rows + 1` lignes d'un SELECT sans LIMIT : un
        // SELECT sur une grosse table n'est pas lu en entier pour être jeté.
        self.conn.query_drop(format!("SET SESSION sql_select_limit = {}", max_rows + 1)).map_err(unreachable)?;
        let executed = self.run(statement, max_rows);
        let reset = self.conn.query_drop("SET SESSION sql_select_limit = DEFAULT").map_err(unreachable);
        let executed = executed?;
        reset?;
        Ok(executed)
    }

    fn begin_write(&mut self, statement: &str) -> Result<u64, DbError> {
        // Seule cette transaction, ouverte ici et pas par le texte saisi, écrit : la session garde
        // sa lecture seule pour tout le reste.
        self.conn.query_drop("START TRANSACTION READ WRITE").map_err(unreachable)?;
        // Préparée dans la transaction : MariaDB refuse de préparer une écriture dans une session en
        // lecture seule. Préparer n'exécute rien, et le serveur refuse plusieurs instructions.
        let affected = self.conn.prep(statement).and_then(|prepared| self.conn.exec_iter(&prepared, ())).and_then(|mut result| {
            // `RETURNING` rend les lignes touchées au lieu de les compter.
            if result.columns().as_ref().is_empty() {
                Ok(result.affected_rows())
            } else {
                result.by_ref().try_fold(0, |n, row| row.map(|_| n + 1))
            }
        });
        match affected {
            Ok(n) => Ok(n),
            Err(e) => {
                let _ = self.conn.query_drop("ROLLBACK");
                Err(unreachable(e))
            }
        }
    }

    fn finish_write(&mut self, commit: bool) -> Result<(), DbError> {
        self.conn.query_drop(if commit { "COMMIT" } else { "ROLLBACK" }).map_err(unreachable)
    }

    fn canceller(&self) -> Canceller {
        let (opts, id) = (self.opts.clone(), self.conn.connection_id());
        Box::new(move || {
            // Une seconde connexion, mêmes identifiants et même chiffrement, le temps du KILL.
            let mut other = Conn::new(opts.clone()).map_err(unreachable)?;
            other.query_drop(format!("KILL QUERY {id}")).map_err(unreachable)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misread_modes_cover_the_compound_ones() {
        // `ANSI`, `ORACLE`… valent ANSI_QUOTES : les garder ferait lire "…" comme un identifiant.
        for mode in ["ANSI", "ORACLE", "MSSQL", "ANSI_QUOTES", "NO_BACKSLASH_ESCAPES"] {
            assert!(MISREAD_MODES.contains(&mode), "{mode}");
        }
    }

    /// Contre un MariaDB jetable : `ROBIADE_MARIA_TEST_PORT` et `ROBIADE_MARIA_TEST_PASSWORD`
    /// (utilisateur `root`, sur localhost), avec la base `app` décrite dans le message du commit qui
    /// l'ajoute.
    fn target() -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_MARIA_TEST_PORT").unwrap().parse().unwrap(),
            database: Some("app".into()),
            user: Some("root".into()),
            password: Some(std::env::var("ROBIADE_MARIA_TEST_PASSWORD").unwrap()),
            ssl_mode: None,
        }
    }

    #[test]
    #[ignore = "demande un MariaDB local jetable"]
    fn browses_a_real_server_read_only() {
        use robiade_core::databases::Dialect;
        use robiade_core::db_browse::{page, page_query, Cursor, Filter, FilterOp, PageRequest};

        let mut session = MariaDb.open(Engine::MariaDb, &target(), Tls::Disabled, true).unwrap();
        assert!(session.read_only().unwrap());
        assert_eq!(session.current_database().unwrap(), "app");
        assert!(session.databases().unwrap().contains(&"app".to_owned()));
        let relations = session.relations().unwrap();
        let user = relations.iter().find(|r| r.table.name == "user").unwrap();
        assert!(user.kind == RelationKind::Table && user.estimated_rows.is_some());
        assert!(relations.iter().any(|r| r.table.name == "recent" && r.kind == RelationKind::View));

        let shape = session.describe(&user.table).unwrap();
        assert_eq!((shape.columns[0].data_type.as_str(), shape.columns[0].primary_key), ("int(10) unsigned", Some(1)));
        let (mut cursor, mut seen) = (Cursor::Start, 0);
        loop {
            let query = page_query(Dialect::MariaDb, &shape, &PageRequest { sort: None, filters: vec![], cursor }).unwrap();
            let p = page(&shape, &query, session.fetch(&query).unwrap());
            seen += p.rows.len();
            match p.next {
                Some(next) => cursor = next,
                None => break,
            }
        }
        assert_eq!(seen, 453);

        let filters = vec![Filter { column: "email".into(), op: FilterOp::Contains("50%_OFF".into()) }];
        let query = page_query(Dialect::MariaDb, &shape, &PageRequest { sort: None, filters, cursor: Cursor::Start }).unwrap();
        let rows = session.fetch(&query).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1].as_deref(), Some("50%_off"));
        let third = page_query(Dialect::MariaDb, &shape, &PageRequest {
            sort: None,
            filters: vec![Filter { column: "id".into(), op: FilterOp::Equals("3".into()) }],
            cursor: Cursor::Start,
        })
        .unwrap();
        assert_eq!(session.fetch(&third).unwrap()[0][3].as_deref(), Some("<2 octets>"));

        // Clé composée date + entier, valeurs longues tronquées côté serveur.
        let event = session.describe(&TableRef { schema: "app".into(), name: "event".into() }).unwrap();
        let (mut cursor, mut seen) = (Cursor::Start, 0);
        loop {
            let query = page_query(Dialect::MariaDb, &event, &PageRequest { sort: None, filters: vec![], cursor }).unwrap();
            let p = page(&event, &query, session.fetch(&query).unwrap());
            assert!(p.rows.iter().all(|r| r[2].as_ref().unwrap().truncated));
            seen += p.rows.len();
            match p.next {
                Some(next) => cursor = next,
                None => break,
            }
        }
        assert_eq!(seen, 251);
    }

    #[test]
    #[ignore = "demande un MariaDB local jetable"]
    fn console_reads_bounded_and_writes_only_in_its_own_transaction() {
        let mut session = MariaDb.open(Engine::MariaDb, &target(), Tls::Disabled, true).unwrap();
        let read = session.execute("SELECT id, created_at, avatar FROM `user`", StatementKind::Query, 100).unwrap();
        assert_eq!(read.rows.len(), 101);
        assert_eq!(read.columns[1].data_type, "datetime");
        assert!(read.rows[0][1].as_ref().unwrap().contains(' '));
        // La limite de lignes ne survit pas à l'instruction.
        let all = session.execute("SELECT COUNT(*) FROM (SELECT id FROM `user`) u", StatementKind::Query, 1000).unwrap();
        assert_eq!(all.rows[0][0].as_deref(), Some("453"));
        assert_eq!(session.relations().unwrap().len(), 3);

        // La session refuse d'écrire ; seule la transaction ouverte par Robiade le peut.
        let refused = session.execute("UPDATE `user` SET email = 'x' WHERE id = 1", StatementKind::Write, 10);
        assert!(matches!(refused, Err(DbError::Unreachable(m)) if m.contains("READ ONLY")));
        assert!(session.begin_write("UPDATE `user` SET email = 'x' WHERE id = 1; DELETE FROM `user`").is_err());
        assert_eq!(session.begin_write("UPDATE `user` SET email = 'x' WHERE id IN (1, 2)").unwrap(), 2);
        session.finish_write(false).unwrap();
        assert!(session.read_only().unwrap());
        let email = session.execute("SELECT email FROM `user` WHERE id = 1", StatementKind::Query, 10).unwrap();
        assert_eq!(email.rows[0][0].as_deref(), Some("u1@ex.fr"));
    }

    #[test]
    #[ignore = "demande un MariaDB local jetable"]
    fn required_tls_refuses_a_certificate_it_cannot_verify() {
        // Le conteneur présente un certificat autosigné : exigé, TLS échoue ; préféré, repli en clair.
        assert!(MariaDb.probe(Engine::MariaDb, &target(), Tls::Required, true).is_err());
        assert!(MariaDb.probe(Engine::MariaDb, &target(), Tls::Preferred, true).unwrap().read_only);
    }

    #[test]
    #[ignore = "demande un MariaDB local jetable"]
    fn cancels_a_running_statement_from_another_thread() {
        let mut session = MariaDb.open(Engine::MariaDb, &target(), Tls::Disabled, true).unwrap();
        let cancel = session.canceller();
        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel().unwrap();
        });
        let _ = session.execute("SELECT SLEEP(10)", StatementKind::Query, 10);
        waiter.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    #[ignore = "demande un MariaDB local jetable"]
    fn drops_the_sql_modes_that_change_how_text_is_read() {
        let (mut admin, _) = connect(Engine::MariaDb, &target(), Tls::Disabled, false, TIMEOUT).unwrap();
        let before: String = admin.query_first("SELECT @@GLOBAL.sql_mode").unwrap().unwrap();
        admin.query_drop("SET GLOBAL sql_mode = 'ANSI,NO_BACKSLASH_ESCAPES,STRICT_TRANS_TABLES'").unwrap();
        let opened = MariaDb.open(Engine::MariaDb, &target(), Tls::Disabled, true);
        admin.exec_drop("SET GLOBAL sql_mode = ?", (before,)).unwrap();
        let mut session = opened.unwrap();
        let read = session.execute("SELECT \"a\\\"b\", @@SESSION.sql_mode", StatementKind::Query, 10).unwrap();
        assert_eq!(read.rows[0][0].as_deref(), Some("a\"b"));
        let mode = read.rows[0][1].clone().unwrap();
        assert!(mode.contains("STRICT_TRANS_TABLES") && !mode.contains("ANSI") && !mode.contains("BACKSLASH"), "{mode}");
    }
}

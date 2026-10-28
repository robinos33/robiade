//! Mongo par le client officiel, en API synchrone. Le serveur n'a pas de lecture seule de session :
//! sur une connexion en lecture seule, la console n'envoie que la liste blanche du domaine, et le
//! test rapporte si le compte lui-même pourrait écrire.

use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mongodb::bson::{doc, Bson, Document};
use mongodb::error::ErrorKind;
use mongodb::options::{
    AuthMechanism, ClientOptions, Credential, ReadPreference, ReadPreferenceOptions, SelectionCriteria, ServerAddress,
    Tls as MongoTls, TlsOptions,
};
use mongodb::sync::Client;
use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};
use robiade_core::db_browse::{
    Canceller, Column, DatabaseBrowser, DbSession, PageQuery, Relation, RelationKind, TableRef, TableShape,
};
use robiade_core::db_console::{Executed, StatementKind};
use robiade_core::db_mongo;
use serde_json::{Map, Value};

pub struct Mongo;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Tentative TLS d'une connexion `Preferred`, vers la machine locale : le client réessaierait sinon
/// jusqu'à `TIMEOUT` avant le repli en clair.
const LOCAL_TLS_TRY: Duration = Duration::from_secs(2);
/// Délai d'une commande de parcours ou de console, passé au serveur (`maxTimeMS`).
const BROWSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Documents lus pour deviner les champs d'une collection, qui n'a pas de schéma.
const SAMPLE: i32 = 100;
/// Bases du serveur lui-même.
const SYSTEM_DATABASES: &[&str] = &["admin", "local", "config"];
/// Actions d'un privilège qui modifient des données ou leur structure.
const WRITE_ACTIONS: &[&str] = &[
    "insert", "update", "remove", "createCollection", "dropCollection", "dropDatabase", "createIndex", "dropIndex",
    "renameCollectionSameDB", "collMod", "convertToCapped", "bypassDocumentValidation",
];
/// Commandes dont le serveur borne la durée (`maxTimeMS`) et qui portent le `comment` qui permet
/// de les retrouver pour les annuler.
const TRACKED: &[&str] = &["find", "aggregate", "count", "distinct"];

fn unreachable(e: mongodb::error::Error) -> DbError {
    let message = match e.kind.as_ref() {
        ErrorKind::Command(command) => command.message.clone(),
        _ => e.to_string(),
    };
    DbError::Unreachable(message)
}

fn read_preference(mode: &str) -> Option<SelectionCriteria> {
    let options = || Some(ReadPreferenceOptions::default());
    let preference = match mode {
        "primary" => ReadPreference::Primary,
        "primaryPreferred" => ReadPreference::PrimaryPreferred { options: options() },
        "secondary" => ReadPreference::Secondary { options: options() },
        "secondaryPreferred" => ReadPreference::SecondaryPreferred { options: options() },
        "nearest" => ReadPreference::Nearest { options: options() },
        _ => return None,
    };
    Some(SelectionCriteria::ReadPreference(preference))
}

/// Les options du client, depuis la cible et ses paramètres de la liste blanche du domaine.
fn options(target: &Target, tls: bool) -> Result<ClientOptions, DbError> {
    let invalid = |what: &str| DbError::Unreachable(format!("paramètre Mongo invalide : {what}"));
    let option = |key: &str| target.options.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let hosts = if target.host.contains(',') {
        target.host.split(',').map(|seed| ServerAddress::parse(seed).map_err(|_| invalid(seed))).collect::<Result<_, _>>()?
    } else {
        vec![ServerAddress::Tcp { host: target.host.clone(), port: Some(target.port) }]
    };
    let mut options = ClientOptions::default();
    options.hosts = hosts;
    options.app_name = Some("robiade".into());
    options.connect_timeout = Some(TIMEOUT);
    options.server_selection_timeout = Some(TIMEOUT);
    // Certificat et nom d'hôte vérifiés : les options qui l'affaibliraient sont refusées par le domaine.
    options.tls = Some(if tls { MongoTls::Enabled(TlsOptions::default()) } else { MongoTls::Disabled });
    options.repl_set_name = option("replicaSet").map(str::to_owned);
    options.direct_connection = match option("directConnection") {
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(other) => return Err(invalid(other)),
        None => None,
    };
    if let Some(mode) = option("readPreference") {
        options.selection_criteria = Some(read_preference(mode).ok_or_else(|| invalid(mode))?);
    }
    if let Some(user) = &target.user {
        let mut credential = Credential::default();
        credential.username = Some(user.clone());
        credential.password = target.password.clone();
        // Comme dans une URL Mongo : `authSource`, sinon la base de l'URL, sinon `admin`.
        credential.source = option("authSource").map(str::to_owned).or_else(|| target.database.clone());
        if let Some(mechanism) = option("authMechanism") {
            credential.mechanism = Some(AuthMechanism::from_str(mechanism).map_err(|_| invalid(mechanism))?);
        }
        options.credential = Some(credential);
    }
    Ok(options)
}

/// Un client qui a répondu à un `ping` : le client Mongo ne se connecte qu'à la première commande.
/// `Preferred` (dev vers la machine locale) : TLS d'abord, brièvement, en clair s'il n'aboutit pas.
fn connect(engine: Engine, target: &Target, tls: Tls) -> Result<Client, DbError> {
    if engine != Engine::Mongo {
        return Err(DbError::Unsupported("ce moteur"));
    }
    let attempt = |tls: bool, selection: Duration| -> Result<Client, DbError> {
        let mut options = options(target, tls)?;
        options.server_selection_timeout = Some(selection);
        let client = Client::with_options(options).map_err(unreachable)?;
        client.database("admin").run_command(doc! { "ping": 1 }).run().map_err(unreachable)?;
        Ok(client)
    };
    match tls {
        Tls::Required => attempt(true, TIMEOUT),
        Tls::Preferred => attempt(true, LOCAL_TLS_TRY).or_else(|_| attempt(false, TIMEOUT)),
        Tls::Disabled => attempt(false, TIMEOUT),
    }
}

/// Le compte peut-il écrire ? Lu dans ses privilèges ; sans authentification, tout est permis.
fn account(client: &Client) -> Result<(String, bool), DbError> {
    let status = client
        .database("admin")
        .run_command(doc! { "connectionStatus": 1, "showPrivileges": true })
        .run()
        .map_err(unreachable)?;
    let info = status.get_document("authInfo").ok();
    let user = info
        .and_then(|i| i.get_array("authenticatedUsers").ok())
        .and_then(|users| users.first())
        .and_then(Bson::as_document)
        .and_then(|u| u.get_str("user").ok())
        .map(str::to_owned);
    let Some(user) = user else { return Ok(("anonyme".into(), false)) };
    let writes = info
        .and_then(|i| i.get_array("authenticatedUserPrivileges").ok())
        .into_iter()
        .flatten()
        .filter_map(Bson::as_document)
        .filter_map(|p| p.get_array("actions").ok())
        .flatten()
        .any(|a| a.as_str().is_some_and(|a| WRITE_ACTIONS.contains(&a)));
    Ok((user, !writes))
}

impl DatabaseProbe for Mongo {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, _read_only: bool) -> Result<Probe, DbError> {
        let client = connect(engine, target, tls)?;
        let build = client.database("admin").run_command(doc! { "buildInfo": 1 }).run().map_err(unreachable)?;
        let (user, read_only) = account(&client)?;
        Ok(Probe {
            server_version: build.get_str("version").unwrap_or_default().to_owned(),
            user,
            database: target.database.clone().unwrap_or_default(),
            read_only,
        })
    }
}

impl DatabaseBrowser for Mongo {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError> {
        let client = connect(engine, target, tls)?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let mut session =
            MongoSession { client, database: String::new(), read_only, comment: format!("robiade-{}-{nanos}", std::process::id()) };
        // Sans base dans la connexion, la première de l'application.
        session.database = match &target.database {
            Some(database) => database.clone(),
            None => session.databases()?.into_iter().next().unwrap_or_default(),
        };
        Ok(Box::new(session))
    }
}

/// Une session sur une base. Le client garde ses connexions ouvertes jusqu'à ce qu'il soit lâché.
struct MongoSession {
    client: Client,
    database: String,
    /// Demandée par la connexion, tenue par la liste blanche de la console.
    read_only: bool,
    /// Porté par chaque commande suivie, pour la retrouver et l'annuler.
    comment: String,
}

/// Une valeur en texte, `None` pour null : chaîne telle quelle, ObjectId en hexadécimal, date en
/// ISO 8601, nombres en clair, le reste en JSON étendu relâché.
fn cell(value: Option<&Bson>) -> Option<String> {
    match value? {
        Bson::Null | Bson::Undefined => None,
        Bson::String(s) => Some(s.clone()),
        Bson::ObjectId(id) => Some(id.to_hex()),
        Bson::DateTime(at) => Some(at.try_to_rfc3339_string().unwrap_or_else(|_| at.timestamp_millis().to_string())),
        Bson::Int32(n) => Some(n.to_string()),
        Bson::Int64(n) => Some(n.to_string()),
        Bson::Double(n) => Some(n.to_string()),
        Bson::Boolean(b) => Some(b.to_string()),
        Bson::Decimal128(d) => Some(d.to_string()),
        other => Some(other.clone().into_relaxed_extjson().to_string()),
    }
}

/// Le type d'une valeur, tel que la grille et les filtres du domaine le nomment.
fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::ObjectId(_) => "objectId",
        Bson::String(_) => "string",
        Bson::Int32(_) => "int",
        Bson::Int64(_) => "long",
        Bson::Double(_) => "double",
        Bson::Decimal128(_) => "decimal",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Binary(_) => "binData",
        Bson::Timestamp(_) => "timestamp",
        Bson::Null | Bson::Undefined => "null",
        _ => "other",
    }
}

fn column(name: &str) -> Column {
    Column { name: name.to_owned(), data_type: String::new(), primary_key: None }
}

/// Champs d'une suite de documents, `_id` d'abord, puis dans l'ordre où ils apparaissent.
fn fields<'a>(documents: impl Iterator<Item = &'a Document>) -> Vec<String> {
    let mut names = vec!["_id".to_owned()];
    for document in documents {
        for key in document.keys() {
            if !names.contains(key) {
                names.push(key.clone());
            }
        }
    }
    names
}

/// Les documents d'une réponse à curseur ; le curseur, s'il reste ouvert, est fermé.
fn first_batch(response: &Document) -> Option<Vec<Document>> {
    let batch = response.get_document("cursor").ok()?.get_array("firstBatch").ok()?;
    Some(batch.iter().filter_map(Bson::as_document).cloned().collect())
}

/// Une réponse en grille : les documents d'un curseur (ou de `listDatabases`) aplatis sur leurs
/// champs de premier niveau, toute autre réponse en une ligne.
fn executed(response: Document, max_rows: usize) -> Executed {
    let listed = response.get_array("databases").ok().map(|d| d.iter().filter_map(Bson::as_document).cloned().collect());
    if let Some(mut documents) = first_batch(&response).or(listed) {
        documents.truncate(max_rows + 1);
        let names = fields(documents.iter());
        // `_id` n'est une colonne que si un document en a un.
        let names: Vec<String> = names.into_iter().filter(|n| n != "_id" || documents.iter().any(|d| d.contains_key("_id"))).collect();
        return Executed {
            columns: names.iter().map(|n| column(n)).collect(),
            rows: documents.iter().map(|d| names.iter().map(|n| cell(d.get(n))).collect()).collect(),
            affected: None,
        };
    }
    let shown: Vec<(&String, &Bson)> =
        response.iter().filter(|(k, _)| !matches!(k.as_str(), "ok" | "$clusterTime" | "operationTime")).collect();
    Executed {
        columns: shown.iter().map(|(k, _)| column(k)).collect(),
        rows: vec![shown.iter().map(|(_, v)| cell(Some(v))).collect()],
        affected: None,
    }
}

impl MongoSession {
    /// Envoie une commande analysée par le domaine. Une lecture suivie porte le délai du serveur et
    /// le commentaire de la session ; un `find` revient en un seul lot, un `aggregate` est borné au
    /// même nombre de documents et son curseur fermé.
    fn run(&self, document: Map<String, Value>, max_rows: usize) -> Result<Document, DbError> {
        let name = document.keys().next().cloned().unwrap_or_default().to_ascii_lowercase();
        let mut command = Document::try_from(document).map_err(|e| DbError::Unreachable(format!("JSON étendu invalide : {e}")))?;
        let batch = i64::try_from(max_rows + 1).unwrap_or(i64::MAX);
        if TRACKED.contains(&name.as_str()) {
            command.entry("maxTimeMS".into()).or_insert(Bson::Int64(BROWSE_TIMEOUT.as_millis() as i64));
            command.insert("comment", self.comment.clone());
        }
        match name.as_str() {
            "find" => {
                let asked = command.get_i64("batchSize").ok().or_else(|| command.get_i32("batchSize").ok().map(i64::from));
                command.insert("batchSize", asked.map_or(batch, |n| n.min(batch)));
                command.insert("singleBatch", true);
            }
            "aggregate" => {
                command.insert("cursor", doc! { "batchSize": batch });
            }
            _ => {}
        }
        // `listDatabases` ne se lance que depuis `admin`.
        let database = if name == "listdatabases" { "admin" } else { self.database.as_str() };
        let response = self.client.database(database).run_command(command).run().map_err(unreachable)?;
        if let Ok(cursor) = response.get_document("cursor") {
            if let (Ok(id), Ok(ns)) = (cursor.get_i64("id"), cursor.get_str("ns")) {
                if id != 0 {
                    let collection = ns.split_once('.').map_or(ns, |(_, c)| c);
                    let _ = self.client.database(database).run_command(doc! { "killCursors": collection, "cursors": [id] }).run();
                }
            }
        }
        Ok(response)
    }
}

impl DbSession for MongoSession {
    fn read_only(&mut self) -> Result<bool, DbError> {
        Ok(self.read_only)
    }

    fn current_database(&mut self) -> Result<String, DbError> {
        Ok(self.database.clone())
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        let listed = self
            .client
            .database("admin")
            .run_command(doc! { "listDatabases": 1, "nameOnly": true, "authorizedDatabases": true })
            .run()
            .map_err(unreachable)?;
        let mut names: Vec<String> = listed
            .get_array("databases")
            .map(|d| d.iter().filter_map(|d| d.as_document()?.get_str("name").ok().map(str::to_owned)).collect())
            .unwrap_or_default();
        names.retain(|n| !SYSTEM_DATABASES.contains(&n.as_str()));
        names.sort();
        Ok(names)
    }

    fn relations(&mut self) -> Result<Vec<Relation>, DbError> {
        let listed = self
            .client
            .database(&self.database)
            .run_command(doc! { "listCollections": 1, "nameOnly": true, "authorizedCollections": true })
            .run()
            .map_err(unreachable)?;
        let mut relations: Vec<Relation> = first_batch(&listed)
            .unwrap_or_default()
            .iter()
            .filter_map(|c| {
                let name = c.get_str("name").ok()?;
                (!name.starts_with("system.")).then(|| Relation {
                    table: TableRef { schema: String::new(), name: name.to_owned() },
                    kind: if c.get_str("type") == Ok("view") { RelationKind::View } else { RelationKind::Table },
                    estimated_rows: None,
                })
            })
            .collect();
        relations.sort_by(|a, b| a.table.name.cmp(&b.table.name));
        Ok(relations)
    }

    /// Une collection n'a pas de schéma : ses champs sont ceux d'un échantillon de documents, et le
    /// type d'un champ celui de sa première valeur non nulle.
    fn describe(&mut self, table: &TableRef) -> Result<TableShape, DbError> {
        let mut sample = Map::new();
        sample.insert("aggregate".into(), Value::String(table.name.clone()));
        sample.insert("pipeline".into(), serde_json::json!([{ "$sample": { "size": SAMPLE } }]));
        let response = self.run(sample, SAMPLE as usize)?;
        let documents = first_batch(&response).unwrap_or_default();
        let columns = fields(documents.iter())
            .into_iter()
            .map(|name| {
                let found = documents.iter().filter_map(|d| d.get(&name)).find(|v| !matches!(v, Bson::Null));
                let data_type = found.map_or(if name == "_id" { "objectId" } else { "null" }, type_name);
                Column { primary_key: (name == "_id").then_some(1), data_type: data_type.to_owned(), name }
            })
            .collect();
        Ok(TableShape { table: table.clone(), columns })
    }

    fn fetch(&mut self, query: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let command = db_mongo::parse(&query.sql)?;
        // Les colonnes de la page sont `_id`, puis les champs de la projection, dans l'ordre.
        let mut names = vec!["_id".to_owned()];
        if let Some(Value::Object(projection)) = command.document.get("projection") {
            names.extend(projection.keys().cloned());
        }
        let response = self.run(command.document, query_limit(&query.sql))?;
        Ok(first_batch(&response).unwrap_or_default().iter().map(|d| names.iter().map(|n| cell(d.get(n))).collect()).collect())
    }

    fn execute(&mut self, statement: &str, _kind: StatementKind, max_rows: usize) -> Result<Executed, DbError> {
        let command = db_mongo::parse(statement)?;
        Ok(executed(self.run(command.document, max_rows)?, max_rows))
    }

    fn canceller(&self) -> Canceller {
        let (client, comment) = (self.client.clone(), self.comment.clone());
        Box::new(move || {
            // Ses propres opérations : ni privilège `inprog` ni `killop` nécessaires.
            let admin = client.database("admin");
            let pipeline = vec![doc! { "$currentOp": { "allUsers": false } }, doc! { "$match": { "command.comment": &comment } }];
            let found = admin
                .run_command(doc! { "aggregate": 1, "pipeline": pipeline, "cursor": {} })
                .run()
                .map_err(unreachable)?;
            for op in first_batch(&found).unwrap_or_default() {
                if let Some(opid) = op.get("opid") {
                    admin.run_command(doc! { "killOp": 1, "op": opid.clone() }).run().map_err(unreachable)?;
                }
            }
            Ok(())
        })
    }

    fn begin_write(&mut self, _statement: &str) -> Result<u64, DbError> {
        Err(DbError::Unsupported("modification de prod sur Mongo : seules les lectures passent"))
    }

    fn finish_write(&mut self, _commit: bool) -> Result<(), DbError> {
        Err(DbError::Unsupported("modification de prod sur Mongo : seules les lectures passent"))
    }
}

/// Le `limit` d'une commande de page : tout le lot doit revenir.
fn query_limit(text: &str) -> usize {
    serde_json::from_str::<Value>(text).ok().and_then(|v| v.get("limit")?.as_u64()).map_or(1, |n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::oid::ObjectId;

    #[test]
    fn shows_values_as_the_grid_compares_them() {
        let id = ObjectId::parse_str("65f1a2b3c4d5e6f708091a2b").unwrap();
        assert_eq!(cell(Some(&Bson::ObjectId(id))).as_deref(), Some("65f1a2b3c4d5e6f708091a2b"));
        assert_eq!(cell(Some(&Bson::Null)), None);
        assert_eq!(cell(None), None);
        assert_eq!(cell(Some(&Bson::Document(doc! { "a": 1 }))).as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(type_name(&Bson::Int64(1)), "long");
    }

    #[test]
    fn flattens_a_cursor_and_keeps_other_answers_on_one_row() {
        let response = doc! { "cursor": { "id": 0_i64, "ns": "app.users", "firstBatch": [
            { "_id": 1, "email": "a@b" },
            { "_id": 2, "age": 7 },
        ] }, "ok": 1 };
        let e = executed(response, 10);
        let names: Vec<&str> = e.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["_id", "email", "age"]);
        assert_eq!(e.rows[1], [Some("2".into()), None, Some("7".into())]);
        let count = executed(doc! { "n": 3, "ok": 1.0 }, 10);
        assert_eq!((count.columns.len(), count.rows[0][0].as_deref()), (1, Some("3")));
        let listed = executed(doc! { "databases": [{ "name": "app" }], "ok": 1 }, 10);
        assert_eq!(listed.rows, [vec![Some("app".into())]]);
    }

    /// Contre un Mongo jetable : `ROBIADE_MONGO_TEST_PORT` et `ROBIADE_MONGO_TEST_PASSWORD`
    /// (utilisateur `root`), avec la base `app` et l'utilisateur `lecteur` décrits dans le message
    /// du commit qui l'ajoute.
    fn target(user: &str, password: &str, database: Option<&str>) -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_MONGO_TEST_PORT").unwrap().parse().unwrap(),
            database: database.map(str::to_owned),
            user: Some(user.into()),
            password: Some(password.into()),
            ssl_mode: None,
            options: if user == "root" { vec![("authSource".into(), "admin".into())] } else { vec![] },
        }
    }

    fn root() -> Target {
        target("root", &std::env::var("ROBIADE_MONGO_TEST_PASSWORD").unwrap(), Some("app"))
    }

    fn reader() -> Target {
        target("lecteur", "lecteurpw", Some("app"))
    }

    #[test]
    #[ignore = "demande un Mongo local jetable"]
    fn probes_what_the_account_may_do() {
        let admin = Mongo.probe(Engine::Mongo, &root(), Tls::Disabled, true).unwrap();
        assert_eq!((admin.user.as_str(), admin.read_only), ("root", false));
        assert!(admin.server_version.starts_with('7'));
        let lecteur = Mongo.probe(Engine::Mongo, &reader(), Tls::Disabled, true).unwrap();
        assert_eq!((lecteur.user.as_str(), lecteur.read_only, lecteur.database.as_str()), ("lecteur", true, "app"));
        assert!(Mongo.probe(Engine::Mongo, &target("lecteur", "faux", Some("app")), Tls::Disabled, true).is_err());
        // Le serveur ne parle pas TLS : exigé, échec ; préféré (localhost), repli en clair.
        assert!(Mongo.probe(Engine::Mongo, &reader(), Tls::Required, true).is_err());
        assert!(Mongo.probe(Engine::Mongo, &reader(), Tls::Preferred, true).is_ok());
    }

    #[test]
    #[ignore = "demande un Mongo local jetable"]
    fn browses_a_collection_page_by_page() {
        use robiade_core::databases::Language;
        use robiade_core::db_browse::{page, page_query, Cursor, Filter, FilterOp, PageRequest, Sort};

        let mut session = Mongo.open(Engine::Mongo, &reader(), Tls::Disabled, true).unwrap();
        assert_eq!(session.current_database().unwrap(), "app");
        assert_eq!(session.databases().unwrap(), ["app"]);
        let relations = session.relations().unwrap();
        let kinds: Vec<(&str, RelationKind)> = relations.iter().map(|r| (r.table.name.as_str(), r.kind)).collect();
        assert_eq!(kinds, [("adults", RelationKind::View), ("users", RelationKind::Table)]);
        let shape = session.describe(&relations[1].table).unwrap();
        let columns: Vec<(&str, &str)> = shape.columns.iter().map(|c| (c.name.as_str(), c.data_type.as_str())).collect();
        assert_eq!(&columns[..3], [("_id", "objectId"), ("email", "string"), ("age", "int")]);
        assert!(columns.contains(&("profile", "object")));

        let count = |session: &mut Box<dyn DbSession>, filters: Vec<Filter>, sort: Option<Sort>| {
            let (mut cursor, mut seen, mut first) = (Cursor::Start, 0, None);
            loop {
                let request = PageRequest { sort: sort.clone(), filters: filters.clone(), cursor };
                let query = page_query(Language::Mongo, &shape, &request).unwrap();
                let p = page(&shape, &query, session.fetch(&query).unwrap());
                first = first.or_else(|| p.rows.first().cloned());
                seen += p.rows.len();
                match p.next {
                    Some(next) => cursor = next,
                    None => break (seen, first),
                }
            }
        };
        assert_eq!(count(&mut session, vec![], None).0, 451);
        let filter = |column: &str, op| vec![Filter { column: column.into(), op }];
        assert_eq!(count(&mut session, filter("email", FilterOp::Contains("50%_".into())), None).0, 1);
        assert_eq!(count(&mut session, filter("age", FilterOp::Equals("30".into())), None).0, 6);
        assert_eq!(count(&mut session, filter("profile", FilterOp::IsNull), None).0, 113);
        let (_, oldest) = count(&mut session, vec![], Some(Sort { column: "age".into(), descending: true }));
        let age = shape.columns.iter().position(|c| c.name == "age").unwrap();
        assert_eq!(oldest.unwrap()[age].as_ref().unwrap().text, "89");
        // Un `_id` affiché se retrouve tel quel.
        let (_, first) = count(&mut session, vec![], None);
        let id = first.unwrap()[0].clone().unwrap().text;
        assert_eq!(count(&mut session, filter("_id", FilterOp::Equals(id)), None).0, 1);
    }

    #[test]
    #[ignore = "demande un Mongo local jetable"]
    fn console_answers_in_rows_and_the_server_keeps_its_rights() {
        let mut session = Mongo.open(Engine::Mongo, &root(), Tls::Disabled, false).unwrap();
        let found = session.execute(r#"{"find": "users", "filter": {"age": {"$gte": 80}}}"#, StatementKind::Query, 5).unwrap();
        assert_eq!(found.rows.len(), 6);
        let grouped = session
            .execute(r#"{"aggregate": "users", "pipeline": [{"$group": {"_id": null, "n": {"$sum": 1}}}], "cursor": {}}"#, StatementKind::Query, 10)
            .unwrap();
        assert_eq!(grouped.rows[0][1].as_deref(), Some("451"));
        let counted = session.execute(r#"{"count": "users", "query": {"_id": {"$exists": true}}}"#, StatementKind::Query, 10).unwrap();
        assert_eq!(counted.rows[0][0].as_deref(), Some("451"));
        let listed = session.execute(r#"{"listDatabases": 1, "nameOnly": true}"#, StatementKind::Query, 10).unwrap();
        assert!(listed.rows.iter().any(|r| r[0].as_deref() == Some("app")));
        let inserted = session.execute(r#"{"insert": "brouillon", "documents": [{"a": 1}]}"#, StatementKind::Write, 10).unwrap();
        assert_eq!(inserted.rows[0][0].as_deref(), Some("1"));
        session.execute(r#"{"drop": "brouillon"}"#, StatementKind::Write, 10).unwrap();

        let mut reading = Mongo.open(Engine::Mongo, &reader(), Tls::Disabled, true).unwrap();
        let refused = reading.execute(r#"{"insert": "brouillon", "documents": [{"a": 1}]}"#, StatementKind::Write, 10);
        assert!(matches!(refused, Err(DbError::Unreachable(m)) if m.contains("not authorized")));
        assert!(reading.begin_write(r#"{"insert": "x", "documents": []}"#).is_err());
    }

    #[test]
    #[ignore = "demande un Mongo local jetable"]
    fn cancels_a_running_command_from_another_thread() {
        let mut session = Mongo.open(Engine::Mongo, &root(), Tls::Disabled, true).unwrap();
        let cancel = session.canceller();
        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            cancel().unwrap();
        });
        // 451 documents à 100 ms : 45 s sans annulation.
        let slow = r#"{"find": "users", "filter": {"$where": "sleep(100) || true"}}"#;
        let outcome = session.execute(slow, StatementKind::Query, 10);
        waiter.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(matches!(outcome, Err(DbError::Unreachable(ref m)) if m.contains("interrupted")), "{outcome:?}");
    }
}

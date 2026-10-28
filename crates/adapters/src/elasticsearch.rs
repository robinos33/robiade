//! Elasticsearch par son API REST. Le serveur n'a pas de lecture seule de session : sur une
//! connexion en lecture seule, la console n'envoie que la liste blanche du domaine, et le test
//! rapporte si le compte lui-même pourrait écrire.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};
use robiade_core::db_browse::{
    Canceller, Column, DatabaseBrowser, DbSession, PageQuery, Relation, RelationKind, TableRef, TableShape,
};
use robiade_core::db_console::{Executed, StatementKind};
use robiade_core::db_elastic::{self, id_column, Method};
use serde_json::{Map, Value};
use ureq::tls::{RootCerts, TlsConfig};

pub struct Elasticsearch;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Délai d'une requête de parcours ou de console, comme pour les bases SQL.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Réponse lue au plus : au-delà, la requête échoue plutôt que de remplir la mémoire.
const BODY_MAX: u64 = 32 * 1024 * 1024;
/// Privilèges qui permettent d'écrire : le compte est en lecture seule s'il n'en a aucun.
const WRITE_CHECK: &str = r#"{"cluster":["all","manage"],"index":[{"names":["*"],"privileges":["all","write","index","create","delete","delete_index","manage"]}]}"#;

/// Un client HTTP vers un cluster : identifiants en en-tête, et un `X-Opaque-Id` qui permet de
/// retrouver ses tâches pour les annuler.
#[derive(Clone)]
struct Client {
    http: ureq::Agent,
    base: String,
    auth: Option<String>,
    opaque: String,
}

fn unreachable(e: ureq::Error) -> DbError {
    DbError::Unreachable(e.to_string())
}

impl Client {
    fn new(target: &Target, https: bool, timeout: Duration) -> Self {
        let http = ureq::Agent::config_builder()
            .https_only(https)
            // Aucune redirection : les identifiants la suivraient vers un autre hôte.
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .user_agent("robiade")
            .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
            .build()
            .into();
        let host = if target.host.contains(':') { format!("[{}]", target.host) } else { target.host.clone() };
        let base = format!("{}://{host}:{}", if https { "https" } else { "http" }, target.port);
        // Un utilisateur : authentification de base. Un mot de passe seul : une clé d'API encodée.
        let auth = match (&target.user, &target.password) {
            (Some(user), password) => {
                let pair = format!("{user}:{}", password.as_deref().unwrap_or_default());
                Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair)))
            }
            (None, Some(key)) => Some(format!("ApiKey {key}")),
            (None, None) => None,
        };
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Self { http, base, auth, opaque: format!("robiade-{}-{nanos}", std::process::id()) }
    }

    /// Envoie une requête et rend son statut et son corps, tous statuts compris.
    fn send(&self, method: Method, path: &str, body: &str) -> Result<(u16, String), DbError> {
        let url = format!("{}{path}", self.base);
        macro_rules! headers {
            ($request:expr) => {{
                let request = $request.header("X-Opaque-Id", &self.opaque).header("Accept", "application/json");
                match &self.auth {
                    Some(auth) => request.header("Authorization", auth),
                    None => request,
                }
            }};
        }
        let content = "application/json";
        let sent = match method {
            Method::Head => headers!(self.http.head(&url)).call(),
            Method::Get if body.is_empty() => headers!(self.http.get(&url)).call(),
            Method::Delete if body.is_empty() => headers!(self.http.delete(&url)).call(),
            Method::Get => headers!(self.http.get(&url)).force_send_body().content_type(content).send(body),
            Method::Delete => headers!(self.http.delete(&url)).force_send_body().content_type(content).send(body),
            Method::Post => headers!(self.http.post(&url)).content_type(content).send(body),
            Method::Put => headers!(self.http.put(&url)).content_type(content).send(body),
        };
        let mut response = sent.map_err(unreachable)?;
        let status = response.status().as_u16();
        let text = if method == Method::Head {
            String::new()
        } else {
            response.body_mut().with_config().limit(BODY_MAX).read_to_string().map_err(unreachable)?
        };
        Ok((status, text))
    }

    /// Une requête qui doit réussir, réponse en JSON.
    fn json(&self, method: Method, path: &str, body: &str) -> Result<Value, DbError> {
        let (status, text) = self.send(method, path, body)?;
        if status >= 400 {
            return Err(failure(status, &text));
        }
        serde_json::from_str(&text).map_err(|e| DbError::Unreachable(format!("réponse illisible : {e}")))
    }
}

/// Le message d'erreur d'Elasticsearch, plutôt que son enveloppe.
fn failure(status: u16, text: &str) -> DbError {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    let error = parsed.as_ref().and_then(|v| v.get("error"));
    let reason = error
        .and_then(|e| e.pointer("/root_cause/0/reason").or_else(|| e.get("reason")))
        .and_then(Value::as_str)
        .or_else(|| error.and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| text.chars().take(300).collect());
    let reason = if reason.is_empty() { "sans message".to_owned() } else { reason };
    DbError::Unreachable(format!("HTTP {status} : {reason}"))
}

/// Une recherche répond 200 même quand ses shards ont échoué (annulation, script en erreur) ou
/// qu'elle a dépassé son délai : des résultats vides ou partiels passeraient pour complets.
fn complete(found: &Value) -> Result<(), DbError> {
    let failed = found.pointer("/_shards/failed").and_then(Value::as_u64).unwrap_or(0);
    if failed > 0 {
        let reason = found.pointer("/_shards/failures/0/reason/reason").and_then(Value::as_str).unwrap_or("sans raison");
        return Err(DbError::Unreachable(format!("{failed} shard(s) en échec, résultat incomplet : {reason}")));
    }
    if found.get("timed_out").and_then(Value::as_bool) == Some(true) {
        return Err(DbError::Unreachable("délai dépassé côté serveur, résultat incomplet".into()));
    }
    Ok(())
}

/// `Preferred` (dev seulement) : HTTPS d'abord, HTTP si le serveur ne le parle pas ou que son
/// certificat ne se vérifie pas. Le premier `GET /` dit aussi si les identifiants passent.
fn connect(engine: Engine, target: &Target, tls: Tls, timeout: Duration) -> Result<(Client, Value), DbError> {
    if engine != Engine::Elasticsearch {
        return Err(DbError::Unsupported("ce moteur"));
    }
    let attempt = |https: bool| {
        let client = Client::new(target, https, timeout);
        client.json(Method::Get, "/", "").map(|root| (client, root))
    };
    match tls {
        Tls::Required => attempt(true),
        Tls::Preferred => attempt(true).or_else(|_| attempt(false)),
        Tls::Disabled => attempt(false),
    }
}

fn cluster_name(root: &Value) -> String {
    root.get("cluster_name").and_then(Value::as_str).unwrap_or_default().to_owned()
}

impl DatabaseProbe for Elasticsearch {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, _read_only: bool) -> Result<Probe, DbError> {
        let (client, root) = connect(engine, target, tls, TIMEOUT)?;
        // Sécurité désactivée (un cluster de dev) : pas d'utilisateur, et tout est permis.
        let user = client
            .json(Method::Get, "/_security/_authenticate", "")
            .ok()
            .and_then(|me| me.get("username").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| "anonyme".to_owned());
        let read_only = client
            .json(Method::Post, "/_security/user/_has_privileges", WRITE_CHECK)
            .ok()
            .is_some_and(|granted| !grants_any(&granted));
        Ok(Probe {
            server_version: root.pointer("/version/number").and_then(Value::as_str).unwrap_or_default().to_owned(),
            user,
            database: cluster_name(&root),
            read_only,
        })
    }
}

/// Un `true` quelque part dans la réponse de `_has_privileges`, hors son résumé.
fn grants_any(granted: &Value) -> bool {
    fn any_true(v: &Value) -> bool {
        match v {
            Value::Bool(b) => *b,
            Value::Object(map) => map.values().any(any_true),
            _ => false,
        }
    }
    ["cluster", "index"].iter().any(|k| granted.get(k).is_some_and(any_true))
}

impl DatabaseBrowser for Elasticsearch {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError> {
        let (client, root) = connect(engine, target, tls, BROWSE_TIMEOUT)?;
        Ok(Box::new(EsSession { cluster: cluster_name(&root), client, read_only }))
    }
}

/// Une session sur un cluster : rien n'y reste ouvert entre deux requêtes.
struct EsSession {
    client: Client,
    cluster: String,
    /// Demandée par la connexion, tenue par la liste blanche de la console.
    read_only: bool,
}

/// Une valeur de document en texte : une chaîne telle quelle, le reste en JSON. NULL ou absent :
/// `None`.
fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Colonnes d'une suite d'objets : leurs clés, dans l'ordre où elles apparaissent.
fn keys<'a>(objects: impl Iterator<Item = &'a Map<String, Value>>) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for object in objects {
        for key in object.keys() {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
    }
    keys
}

fn column(name: &str) -> Column {
    Column { name: name.to_owned(), data_type: String::new(), primary_key: None }
}

/// Une réponse en grille : les documents d'une recherche aplatis sur leurs champs de premier
/// niveau, un tableau d'objets ligne par ligne, et toute autre réponse en une ligne.
fn executed(value: Value, max_rows: usize) -> Executed {
    let documents = value.pointer("/hits/hits").or_else(|| value.get("docs")).and_then(Value::as_array);
    if let Some(documents) = documents {
        let documents: Vec<&Value> = documents.iter().take(max_rows + 1).collect();
        let sources: Vec<&Map<String, Value>> =
            documents.iter().filter_map(|d| d.get("_source").and_then(Value::as_object)).collect();
        let fields = keys(sources.into_iter());
        let mut columns = vec![column("_index"), column("_id")];
        columns.extend(fields.iter().map(|f| column(f)));
        let rows = documents
            .iter()
            .map(|d| {
                let source = d.get("_source");
                let mut row = vec![text(d.get("_index")), text(d.get("_id"))];
                row.extend(fields.iter().map(|f| text(source.and_then(|s| s.get(f)))));
                row
            })
            .collect();
        return Executed { columns, rows, affected: None };
    }
    // `_sql` : colonnes nommées, lignes en tableaux.
    if let (Some(names), Some(rows)) = (value.get("columns").and_then(Value::as_array), value.get("rows").and_then(Value::as_array)) {
        let columns = names.iter().map(|c| column(c.get("name").and_then(Value::as_str).unwrap_or_default())).collect();
        let rows = rows
            .iter()
            .take(max_rows + 1)
            .map(|r| r.as_array().map(|cells| cells.iter().map(|c| text(Some(c))).collect()).unwrap_or_default())
            .collect();
        return Executed { columns, rows, affected: None };
    }
    let objects: Vec<Map<String, Value>> = match value {
        Value::Array(items) => items.into_iter().take(max_rows + 1).filter_map(|i| i.as_object().cloned()).collect(),
        Value::Object(map) => vec![map],
        scalar => return Executed { columns: vec![column("réponse")], rows: vec![vec![text(Some(&scalar))]], affected: None },
    };
    let names = keys(objects.iter());
    Executed {
        columns: names.iter().map(|n| column(n)).collect(),
        rows: objects.iter().map(|o| names.iter().map(|n| text(o.get(n))).collect()).collect(),
        affected: None,
    }
}

/// Une réponse qui n'est pas du JSON (`_cat` sans `format=json`) : une ligne par ligne de texte.
fn plain(text: &str, max_rows: usize) -> Executed {
    Executed {
        columns: vec![column("réponse")],
        rows: text.lines().take(max_rows + 1).map(|l| vec![Some(l.to_owned())]).collect(),
        affected: None,
    }
}

impl DbSession for EsSession {
    fn read_only(&mut self) -> Result<bool, DbError> {
        Ok(self.read_only)
    }

    fn current_database(&mut self) -> Result<String, DbError> {
        Ok(self.cluster.clone())
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(vec![self.cluster.clone()])
    }

    fn relations(&mut self) -> Result<Vec<Relation>, DbError> {
        // Les index cachés (`.security`, `.kibana`…) appartiennent au cluster, pas à l'application.
        let indices = self.client.json(Method::Get, "/_cat/indices?format=json&h=index,docs.count&s=index", "")?;
        Ok(indices
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|i| {
                let name = i.get("index")?.as_str()?;
                (!name.starts_with('.')).then(|| Relation {
                    table: TableRef { schema: String::new(), name: name.to_owned() },
                    kind: RelationKind::Table,
                    estimated_rows: i.get("docs.count").and_then(Value::as_str).and_then(|n| n.parse().ok()),
                })
            })
            .collect())
    }

    fn describe(&mut self, table: &TableRef) -> Result<TableShape, DbError> {
        // La requête de page passe par le même chemin : un nom refusé ici le serait là aussi.
        let page = db_elastic::parse(&format!("GET /{}/_mapping", encode(&table.name)))?;
        let mapping = self.client.json(page.method, &page.path, "")?;
        let properties = mapping
            .as_object()
            .and_then(|indices| indices.values().next())
            .and_then(|index| index.pointer("/mappings/properties"))
            .and_then(Value::as_object);
        let mut columns = vec![id_column()];
        for (name, field) in properties.into_iter().flatten() {
            let data_type = field.get("type").and_then(Value::as_str).unwrap_or("object");
            columns.push(Column { name: name.clone(), data_type: data_type.to_owned(), primary_key: None });
        }
        Ok(TableShape { table: table.clone(), columns })
    }

    fn fetch(&mut self, query: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let request = db_elastic::parse(&query.sql)?;
        // Les colonnes de la page sont `_id`, puis les champs demandés dans `_source`, dans l'ordre.
        let body: Value = serde_json::from_str(&request.body).map_err(|e| DbError::Unreachable(e.to_string()))?;
        let fields: Vec<&str> = body.get("_source").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let found = self.client.json(request.method, &request.path, &request.body)?;
        complete(&found)?;
        let hits = found.pointer("/hits/hits").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        Ok(hits
            .iter()
            .map(|hit| {
                let mut row = vec![text(hit.get("_id"))];
                row.extend(fields.iter().map(|f| text(hit.get("_source").and_then(|s| s.get(*f)))));
                row
            })
            .collect())
    }

    fn execute(&mut self, statement: &str, _kind: StatementKind, max_rows: usize) -> Result<Executed, DbError> {
        let request = db_elastic::parse(statement)?;
        let (status, text) = self.client.send(request.method, &request.path, &request.body)?;
        if status >= 400 {
            return Err(failure(status, &text));
        }
        if request.method == Method::Head {
            return Ok(Executed { columns: vec![column("statut")], rows: vec![vec![Some(status.to_string())]], affected: None });
        }
        Ok(match serde_json::from_str::<Value>(&text) {
            Ok(value) => {
                complete(&value)?;
                executed(value, max_rows)
            }
            Err(_) => plain(&text, max_rows),
        })
    }

    fn canceller(&self) -> Canceller {
        let client = self.client.clone();
        Box::new(move || {
            // Les tâches en cours qui portent l'identifiant de cette session, annulées une à une.
            let tasks = client.json(Method::Get, "/_tasks?detailed=true", "").map_err(|e| match e {
                DbError::Unreachable(m) if m.starts_with("HTTP 403") => {
                    DbError::Unreachable("annulation refusée : le compte n'a pas le droit de lister les tâches".into())
                }
                e => e,
            })?;
            let mine = tasks
                .get("nodes")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|nodes| nodes.values())
                .filter_map(|node| node.get("tasks").and_then(Value::as_object))
                .flat_map(|tasks| tasks.iter())
                .filter(|(_, task)| task.pointer("/headers/X-Opaque-Id").and_then(Value::as_str) == Some(&client.opaque));
            for (id, _) in mine {
                client.json(Method::Post, &format!("/_tasks/{}/_cancel", encode(id)), "")?;
            }
            Ok(())
        })
    }

    fn begin_write(&mut self, _statement: &str) -> Result<u64, DbError> {
        Err(DbError::Unsupported("modification de prod sur Elasticsearch, sans transaction pour l'annuler"))
    }

    fn finish_write(&mut self, _commit: bool) -> Result<(), DbError> {
        Err(DbError::Unsupported("modification de prod sur Elasticsearch, sans transaction pour l'annuler"))
    }
}

/// Segment de chemin encodé, `:` compris (identifiant de tâche `nœud:numéro`).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_search_hits_on_their_first_level_fields() {
        let response = serde_json::json!({"hits": {"hits": [
            {"_index": "logs", "_id": "1", "_source": {"level": "error", "user": {"id": 7}}},
            {"_index": "logs", "_id": "2", "_source": {"level": null, "count": 3}},
        ]}});
        let e = executed(response, 10);
        let names: Vec<&str> = e.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["_index", "_id", "level", "user", "count"]);
        assert_eq!(e.rows[0], [Some("logs".into()), Some("1".into()), Some("error".into()), Some(r#"{"id":7}"#.into()), None]);
        assert_eq!(e.rows[1][2], None);
        assert_eq!(e.rows[1][4].as_deref(), Some("3"));
    }

    #[test]
    fn other_answers_become_rows_too() {
        let cat = executed(serde_json::json!([{"index": "a", "docs.count": "3"}, {"index": "b"}]), 10);
        assert_eq!(cat.rows, [vec![Some("a".into()), Some("3".into())], vec![Some("b".into()), None]]);
        let sql = executed(serde_json::json!({"columns": [{"name": "n"}], "rows": [[1], [2]]}), 1);
        assert_eq!(sql.rows.len(), 2);
        let count = executed(serde_json::json!({"count": 12, "_shards": {"total": 1}}), 10);
        assert_eq!(count.rows, [vec![Some("12".into()), Some(r#"{"total":1}"#.into())]]);
        assert_eq!(plain("a\nb\n", 10).rows.len(), 2);
    }

    #[test]
    fn reports_the_server_reason() {
        let body = r#"{"error":{"root_cause":[{"type":"index_not_found_exception","reason":"no such index [x]"}]},"status":404}"#;
        assert_eq!(failure(404, body), DbError::Unreachable("HTTP 404 : no such index [x]".into()));
        assert_eq!(failure(502, ""), DbError::Unreachable("HTTP 502 : sans message".into()));
        let granted = serde_json::json!({"has_all_requested": false, "cluster": {"all": false}, "index": {"*": {"write": true}}});
        assert!(grants_any(&granted));
        assert!(!grants_any(&serde_json::json!({"has_all_requested": false, "cluster": {"all": false}, "index": {"*": {"write": false}}})));
    }

    /// Contre un Elasticsearch jetable en HTTP : `ROBIADE_ES_TEST_PORT` et `ROBIADE_ES_TEST_PASSWORD`
    /// (utilisateur `elastic`), avec l'index `logs` et l'utilisateur `lecteur` décrits dans le
    /// message du commit qui l'ajoute.
    fn target(user: &str, password: Option<String>) -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_ES_TEST_PORT").unwrap().parse().unwrap(),
            database: None,
            user: (!user.is_empty()).then(|| user.to_owned()),
            password,
            ssl_mode: None,
        }
    }

    fn admin() -> Target {
        target("elastic", Some(std::env::var("ROBIADE_ES_TEST_PASSWORD").unwrap()))
    }

    #[test]
    #[ignore = "demande un Elasticsearch local jetable"]
    fn probes_what_the_account_may_do() {
        let es = Elasticsearch;
        let superuser = es.probe(Engine::Elasticsearch, &admin(), Tls::Disabled, true).unwrap();
        assert_eq!((superuser.user.as_str(), superuser.read_only), ("elastic", false));
        assert!(superuser.server_version.starts_with('8'));
        let reader = es.probe(Engine::Elasticsearch, &target("lecteur", Some("lecteurpw".into())), Tls::Disabled, true).unwrap();
        assert_eq!((reader.user.as_str(), reader.read_only), ("lecteur", true));
        assert!(es.probe(Engine::Elasticsearch, &target("lecteur", Some("faux".into())), Tls::Disabled, true).is_err());
        // Clé d'API : un mot de passe sans utilisateur.
        let (client, _) = connect(Engine::Elasticsearch, &admin(), Tls::Disabled, TIMEOUT).unwrap();
        let key = client.json(Method::Post, "/_security/api_key", r#"{"name":"robiade-test","expiration":"1h"}"#).unwrap();
        let encoded = key.get("encoded").and_then(Value::as_str).unwrap().to_owned();
        assert_eq!(es.probe(Engine::Elasticsearch, &target("", Some(encoded)), Tls::Disabled, true).unwrap().user, "elastic");
        // Un serveur en HTTP : exigé, TLS échoue ; préféré, repli en clair.
        assert!(es.probe(Engine::Elasticsearch, &admin(), Tls::Required, true).is_err());
        assert!(es.probe(Engine::Elasticsearch, &admin(), Tls::Preferred, true).is_ok());
    }

    #[test]
    #[ignore = "demande un Elasticsearch local jetable"]
    fn browses_an_index_page_by_page() {
        use robiade_core::databases::Language;
        use robiade_core::db_browse::{page, page_query, Cursor, Filter, FilterOp, PageRequest, Sort};

        let mut session = Elasticsearch.open(Engine::Elasticsearch, &target("lecteur", Some("lecteurpw".into())), Tls::Disabled, true).unwrap();
        assert_eq!(session.databases().unwrap(), ["docker-cluster"]);
        let relations = session.relations().unwrap();
        let logs = relations.iter().find(|r| r.table.name == "logs").unwrap();
        assert_eq!(logs.estimated_rows, Some(450));
        assert!(relations.iter().all(|r| !r.table.name.starts_with('.')));
        let shape = session.describe(&logs.table).unwrap();
        let names: Vec<&str> = shape.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["_id", "at", "level", "message", "user"]);
        assert_eq!(shape.columns[4].data_type, "object");

        let count = |session: &mut Box<dyn DbSession>, filters: Vec<Filter>, sort: Option<Sort>| {
            let (mut cursor, mut seen, mut first) = (Cursor::Start, 0, None);
            loop {
                let request = PageRequest { sort: sort.clone(), filters: filters.clone(), cursor };
                let query = page_query(Language::Elastic, &shape, &request).unwrap();
                let p = page(&shape, &query, session.fetch(&query).unwrap());
                first = first.or_else(|| p.rows.first().cloned());
                seen += p.rows.len();
                match p.next {
                    Some(next) => cursor = next,
                    None => break (seen, first),
                }
            }
        };
        assert_eq!(count(&mut session, vec![], None).0, 450);
        let filter = |column: &str, op| vec![Filter { column: column.into(), op }];
        assert_eq!(count(&mut session, filter("level", FilterOp::Contains("ERR".into())), None).0, 150);
        assert_eq!(count(&mut session, filter("message", FilterOp::Equals("disk full".into())), None).0, 90);
        assert_eq!(count(&mut session, filter("level", FilterOp::IsNull), None).0, 1);
        let (_, first) = count(&mut session, filter("_id", FilterOp::Equals("42".into())), None);
        let first = first.unwrap();
        assert_eq!(first[0].as_ref().unwrap().text, "42");
        assert_eq!(first[4].as_ref().unwrap().text, r#"{"id":42}"#);
        let sort = Some(Sort { column: "at".into(), descending: true });
        let (_, latest) = count(&mut session, vec![], sort);
        assert!(latest.unwrap()[1].as_ref().unwrap().text.starts_with("2026-10-28"));
    }

    #[test]
    #[ignore = "demande un Elasticsearch local jetable"]
    fn console_answers_in_rows() {
        let mut session = Elasticsearch.open(Engine::Elasticsearch, &admin(), Tls::Disabled, true).unwrap();
        let found = session.execute("GET /logs/_search\n{\"size\": 3, \"sort\": [\"_doc\"]}", StatementKind::Query, 1000).unwrap();
        assert_eq!(found.rows.len(), 3);
        assert_eq!(found.columns[1].name, "_id");
        let count = session.execute("GET /logs/_count", StatementKind::Query, 1000).unwrap();
        assert_eq!(count.rows[0][0].as_deref(), Some("450"));
        let cat = session.execute("GET /_cat/indices/logs?h=index", StatementKind::Query, 1000).unwrap();
        assert_eq!(cat.rows, [vec![Some("logs".into())]]);
        let sql = session.execute("POST /_sql?format=json\n{\"query\": \"SELECT COUNT(*) FROM logs\"}", StatementKind::Query, 1000).unwrap();
        assert_eq!(sql.rows[0][0].as_deref(), Some("450"));
        let missing = session.execute("GET /absent/_search", StatementKind::Query, 1000);
        assert!(matches!(missing, Err(DbError::Unreachable(m)) if m.contains("no such index")));
        // Rien ne tourne : l'annulation n'a rien à faire, mais sait lister les tâches.
        session.canceller()().unwrap();
        assert!(session.begin_write("POST /logs/_doc\n{}").is_err());
    }

    #[test]
    #[ignore = "demande un Elasticsearch local jetable"]
    fn cancels_a_running_search_from_another_thread() {
        let mut session = Elasticsearch.open(Engine::Elasticsearch, &admin(), Tls::Disabled, true).unwrap();
        // Une soixantaine de scripts par document : plusieurs secondes sur l'index de test.
        let source = "long x = params.k; for (int i = 0; i < 900000; i++) { x += i; } return x > 0;";
        let should: Vec<Value> = (0..60).map(|k| serde_json::json!({"script": {"script": {"source": source, "params": {"k": k}}}})).collect();
        let slow = format!("POST /logs/_search?request_cache=false\n{}", serde_json::json!({"size": 0, "query": {"bool": {"should": should}}}));
        let cancel = session.canceller();
        let waiter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            cancel().unwrap();
        });
        let outcome = session.execute(&slow, StatementKind::Query, 10);
        waiter.join().unwrap();
        // Elasticsearch ne vérifie l'annulation qu'entre deux étapes de la recherche : ce script,
        // tout en calcul sur 450 documents, va au bout. C'est l'issue qui compte ici, pas le délai.
        assert!(matches!(outcome, Err(DbError::Unreachable(ref m)) if m.contains("cancel")), "{outcome:?}");
    }
}

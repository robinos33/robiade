//! Elasticsearch : la console au format de la Dev Tools de Kibana, et les pages d'un index.
//!
//! Une requête est une ligne `MÉTHODE /chemin?paramètres`, suivie d'un corps JSON facultatif sur les
//! lignes suivantes, jusqu'à la prochaine ligne de méthode ; les lignes `#` et `//` sont des
//! commentaires. Elasticsearch n'a pas de lecture seule de session : sur une connexion en lecture
//! seule, et toujours en prod, la console n'envoie que les requêtes d'une liste blanche de
//! lectures, jugées sur la méthode et l'endpoint, pas sur le corps. Le chemin est jugé décodé :
//! `%5Fdelete_by_query` est `_delete_by_query` pour le serveur, donc ici aussi.

use crate::databases::{percent_decode, DbError};
use crate::db_browse::{Column, Cursor, FilterOp, PageQuery, PageRequest, TableShape, FILTERS_MAX, PAGE_SIZE};
use crate::db_console::{Admission, Policy, StatementKind};

/// Fenêtre de pagination par défaut d'un index (`index.max_result_window`) : au-delà, `from`
/// n'est plus accepté.
pub const MAX_WINDOW: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
}

impl Method {
    fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "DELETE" => Some(Self::Delete),
            "HEAD" => Some(Self::Head),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

/// Une requête de la console, envoyée telle qu'elle a été analysée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// Chemin puis paramètres, `/` initial compris, tels que saisis.
    pub path: String,
    /// Corps sans ses commentaires ; vide s'il n'y en a pas.
    pub body: String,
}

/// Une requête dans le texte de l'éditeur : de sa ligne de méthode à sa dernière ligne utile.
struct Block {
    start: usize,
    end: usize,
}

fn is_comment(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('#') || line.starts_with("//")
}

/// Une ligne JSON ne commence jamais par un mot nu : `GET` en tête de ligne ouvre une requête.
fn is_method_line(line: &str) -> bool {
    line.split_whitespace().next().and_then(Method::parse).is_some()
}

fn blocks(text: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut offset = 0;
    let mut open = false;
    for line in text.split_inclusive('\n') {
        let start = offset + (line.len() - line.trim_start().len());
        let end = offset + line.trim_end().len();
        offset += line.len();
        if line.trim().is_empty() || is_comment(line) {
            continue;
        }
        match blocks.last_mut() {
            Some(block) if open && !is_method_line(line) => block.end = end,
            _ => {
                blocks.push(Block { start, end });
                open = true;
            }
        }
    }
    blocks
}

/// La requête sous le curseur (en octets) ; entre deux requêtes, la précédente.
pub fn statement_at(text: &str, cursor: usize) -> Result<&str, DbError> {
    let blocks = blocks(text);
    let found = blocks.iter().rev().find(|b| b.start <= cursor).or_else(|| blocks.first()).ok_or(DbError::NoStatement)?;
    Ok(&text[found.start..found.end])
}

/// Le texte entier doit tenir en une requête : c'est le cas d'une sélection.
pub fn single_statement(text: &str) -> Result<&str, DbError> {
    match blocks(text).as_slice() {
        [] => Err(DbError::NoStatement),
        [one] => Ok(&text[one.start..one.end]),
        _ => Err(DbError::MultipleStatements),
    }
}

/// Segments du chemin, décodés. Un segment qui ne se décode pas, dont le décodage contient un `/`
/// ou un blanc, ou qui vaut `.` ou `..` (qu'un client HTTP peut normaliser avant d'envoyer),
/// rendrait l'endpoint ambigu : refusé.
fn segments(path: &str) -> Result<Vec<String>, DbError> {
    let invalid = || DbError::Invalid(crate::databases::Part::Url);
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|raw| {
            let decoded = percent_decode(raw).ok_or_else(invalid)?;
            let dots = decoded == "." || decoded == "..";
            if dots || decoded.chars().any(|c| c == '/' || c.is_whitespace() || c.is_control()) {
                return Err(invalid());
            }
            Ok(decoded)
        })
        .collect()
}

/// Analyse une requête : sa ligne de méthode, puis son corps.
pub fn parse(statement: &str) -> Result<Request, DbError> {
    let mut lines = statement.lines().filter(|l| !l.trim().is_empty() && !is_comment(l));
    let first = lines.next().ok_or(DbError::NoStatement)?;
    let mut words = first.split_whitespace();
    let method = words.next().and_then(Method::parse).ok_or(DbError::NotARequest)?;
    let path = words.next().ok_or(DbError::NotARequest)?;
    if words.next().is_some() {
        return Err(DbError::NotARequest);
    }
    let path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };
    let (route, query) = path.split_once('?').unwrap_or((&path, ""));
    if route.contains('#') || query.contains('#') || query.chars().any(char::is_control) {
        return Err(DbError::NotARequest);
    }
    segments(route)?;
    let body = lines.collect::<Vec<_>>().join("\n");
    Ok(Request { method, path, body: body.trim().to_owned() })
}

/// L'endpoint d'un chemin : son premier segment qui commence par `_`, et ce qui le suit. Un nom
/// d'index ne commence jamais par `_`.
fn endpoint(segments: &[String]) -> Option<(&str, &[String])> {
    let i = segments.iter().position(|s| s.starts_with('_'))?;
    Some((segments[i].as_str(), &segments[i + 1..]))
}

/// Liste blanche des lectures. Elasticsearch ne promet pas qu'un GET ne modifie rien
/// (`GET _refresh`, `GET _flush` le font) : chaque endpoint est nommé.
fn is_read(request: &Request) -> Result<bool, DbError> {
    use Method::*;
    let route = request.path.split('?').next().unwrap_or_default();
    let segments = segments(route)?;
    let Some((endpoint, rest)) = endpoint(&segments) else {
        // `/` (le cluster) ou `/index` (sa description).
        return Ok(matches!(request.method, Get | Head));
    };
    let read = match (request.method, endpoint) {
        (Get | Post, "_search") => matches!(rest, [] | [_] if rest.first().is_none_or(|s| s == "template" || s == "scroll")),
        (Get | Post, "_count" | "_field_caps" | "_validate" | "_explain" | "_mget" | "_msearch" | "_termvectors"
            | "_mtermvectors" | "_terms_enum" | "_analyze" | "_rank_eval" | "_knn_search") => true,
        // SQL d'Elasticsearch : SELECT, SHOW et DESCRIBE seulement, par construction.
        (Get | Post, "_sql") => matches!(rest, [] | [_] if rest.first().is_none_or(|s| s == "translate")),
        (Get | Head, "_mapping" | "_mappings" | "_settings" | "_alias" | "_aliases" | "_cat" | "_nodes" | "_stats"
            | "_doc" | "_source" | "_segments" | "_recovery" | "_resolve" | "_data_stream" | "_tasks"
            | "_index_template" | "_component_template" | "_template" | "_shard_stores") => true,
        (Get, "_cluster") => matches!(rest.first().map(String::as_str), Some("health" | "state" | "stats" | "settings")),
        (Get, "_security") => matches!(rest, [s] if s == "_authenticate"),
        _ => false,
    };
    Ok(read)
}

/// Dit si une requête peut partir : toujours une lecture de la liste blanche ; hors lecture seule,
/// le reste après confirmation ; en prod, rien d'autre que des lectures.
pub fn admit(statement: &str, policy: Policy) -> Result<Admission, DbError> {
    let request = parse(statement)?;
    if is_read(&request)? {
        return Ok(Admission::Run(StatementKind::Query));
    }
    match policy {
        Policy::Writable => Ok(Admission::Confirm),
        Policy::ReadOnly => Err(DbError::ReadOnlyGuard("requête hors de la liste des lectures Elasticsearch")),
        Policy::Prod => Err(DbError::ProdReadsOnly(format!("{} {}", request.method.as_str(), request.path))),
    }
}

/// Chaîne JSON, échappée.
fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Segment de chemin : tout ce qui n'est pas non réservé est encodé.
fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Types qu'Elasticsearch sait trier et comparer à une valeur exacte.
const EXACT: &[&str] = &[
    "keyword", "constant_keyword", "long", "integer", "short", "byte", "double", "float", "half_float",
    "scaled_float", "unsigned_long", "date", "date_nanos", "boolean", "ip", "version",
];
/// Types analysés : une valeur s'y cherche comme une phrase.
const TEXT: &[&str] = &["text", "match_only_text", "search_as_you_type"];

/// Le motif `wildcard` qui cherche `value` telle quelle.
fn wildcard(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('*', "\\*").replace('?', "\\?");
    format!("*{escaped}*")
}

/// Requête d'une page d'un index : `_id`, puis les champs de premier niveau, triés et filtrés,
/// par décalage (`from`) ; `search_after` demanderait un tri unique que `_id` ne fournit plus.
pub fn search_query(shape: &TableShape, request: &PageRequest) -> Result<PageQuery, DbError> {
    if request.filters.len() > FILTERS_MAX {
        return Err(DbError::TooManyFilters);
    }
    let offset = match request.cursor {
        Cursor::Start => 0,
        Cursor::Offset(n) => n,
        Cursor::After(_) => return Err(DbError::InvalidCursor),
    };
    if offset + PAGE_SIZE as u64 + 1 > MAX_WINDOW {
        return Err(DbError::Unsupported("au-delà de 10 000 documents : filtre ou trie pour aller plus loin"));
    }
    let (mut filter, mut must_not) = (Vec::new(), Vec::new());
    for f in &request.filters {
        let column = shape.column(&f.column)?;
        let field = json(&column.name);
        let id = column.name == "_id";
        match &f.op {
            FilterOp::IsNull if id => filter.push(r#"{"match_none":{}}"#.to_owned()),
            FilterOp::IsNotNull if id => {}
            FilterOp::IsNull => must_not.push(format!(r#"{{"exists":{{"field":{field}}}}}"#)),
            FilterOp::IsNotNull => filter.push(format!(r#"{{"exists":{{"field":{field}}}}}"#)),
            FilterOp::Equals(v) if id => filter.push(format!(r#"{{"ids":{{"values":[{}]}}}}"#, json(v))),
            FilterOp::Equals(v) if TEXT.contains(&column.data_type.as_str()) => {
                filter.push(format!(r#"{{"match_phrase":{{{field}:{}}}}}"#, json(v)));
            }
            FilterOp::Equals(v) if EXACT.contains(&column.data_type.as_str()) || column.data_type == "wildcard" => {
                filter.push(format!(r#"{{"term":{{{field}:{}}}}}"#, json(v)));
            }
            FilterOp::Equals(_) => return Err(DbError::Unsupported("égalité sur un objet")),
            FilterOp::Contains(v) if v.is_empty() => return Err(DbError::EmptyFilter),
            FilterOp::Contains(v) if TEXT.contains(&column.data_type.as_str()) => {
                filter.push(format!(r#"{{"match_phrase":{{{field}:{}}}}}"#, json(v)));
            }
            FilterOp::Contains(v) if matches!(column.data_type.as_str(), "keyword" | "constant_keyword" | "wildcard") => {
                let pattern = json(&wildcard(v));
                filter.push(format!(r#"{{"wildcard":{{{field}:{{"value":{pattern},"case_insensitive":true}}}}}}"#));
            }
            FilterOp::Contains(_) => return Err(DbError::Unsupported("« contient » hors d'un champ texte ou keyword")),
        }
    }
    let sort = match &request.sort {
        None => r#"["_doc"]"#.to_owned(),
        Some(s) => {
            let column = shape.column(&s.column)?;
            if !EXACT.contains(&column.data_type.as_str()) {
                return Err(DbError::Unsupported("tri sur un champ qui n'est ni nombre, ni date, ni keyword"));
            }
            let order = if s.descending { "desc" } else { "asc" };
            format!(r#"[{{{}:{{"order":"{order}","missing":"_last"}}}},"_doc"]"#, json(&column.name))
        }
    };
    let fields: Vec<String> = shape.columns.iter().filter(|c| c.name != "_id").map(|c| json(&c.name)).collect();
    let mut body = format!(
        r#"{{"size":{},"from":{offset},"track_total_hits":false,"_source":[{}],"sort":{sort}"#,
        PAGE_SIZE + 1,
        fields.join(",")
    );
    if !filter.is_empty() || !must_not.is_empty() {
        body.push_str(&format!(r#","query":{{"bool":{{"filter":[{}],"must_not":[{}]}}}}"#, filter.join(","), must_not.join(",")));
    }
    body.push('}');
    let path = format!("/{}/_search", encode_segment(&shape.table.name));
    Ok(PageQuery::by_offset(format!("POST {path}\n{body}"), offset))
}

/// Colonne `_id` d'un index : la seule clé d'un document.
pub fn id_column() -> Column {
    Column { name: "_id".into(), data_type: "_id".into(), primary_key: Some(1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_browse::{Filter, Sort, TableRef};

    #[test]
    fn splits_requests_on_their_method_line() {
        let text = "# lecture\nGET /idx/_search\n{\n  \"query\": {\"match_all\": {}}\n}\n\n// suite\npost _count\n{}\n\nDELETE /idx\n";
        assert_eq!(statement_at(text, 0).unwrap(), "GET /idx/_search\n{\n  \"query\": {\"match_all\": {}}\n}");
        let count = text.find("post").unwrap();
        // Sur les lignes vides et le commentaire qui suivent : encore la précédente.
        assert_eq!(statement_at(text, count - 3).unwrap(), statement_at(text, 15).unwrap());
        assert_eq!(statement_at(text, count + 2).unwrap(), "post _count\n{}");
        assert_eq!(statement_at(text, text.len()).unwrap(), "DELETE /idx");
        assert!(matches!(single_statement(text), Err(DbError::MultipleStatements)));
        assert!(matches!(single_statement("# rien\n\n"), Err(DbError::NoStatement)));

        let r = parse("get idx/_search?size=5\n# commentaire\n{\"query\": {}}").unwrap();
        assert_eq!(r, Request { method: Method::Get, path: "/idx/_search?size=5".into(), body: "{\"query\": {}}".into() });
        assert!(matches!(parse("{\"query\": {}}"), Err(DbError::NotARequest)));
        assert!(matches!(parse("GET /idx/_search de trop"), Err(DbError::NotARequest)));
    }

    #[test]
    fn reads_pass_everywhere_and_the_rest_only_where_writes_are_allowed() {
        for request in [
            "GET /",
            "GET /idx",
            "GET /idx/_search",
            "POST /idx,autre/_search?size=0\n{\"aggs\": {}}",
            "POST /_search/scroll\n{\"scroll_id\": \"x\"}",
            "GET /idx/_count",
            "GET /idx/_mapping",
            "GET _cat/indices?v&format=json",
            "GET /idx/_doc/42",
            "HEAD /idx/_doc/42",
            "POST /_sql?format=json\n{\"query\": \"SELECT 1\"}",
            "GET /_cluster/health",
            "GET /_security/_authenticate",
            "POST /idx/_mget\n{\"ids\": [\"1\"]}",
        ] {
            for policy in [Policy::ReadOnly, Policy::Prod, Policy::Writable] {
                assert_eq!(admit(request, policy).unwrap(), Admission::Run(StatementKind::Query), "{request}");
            }
        }
        for request in [
            "PUT /idx/_doc/1\n{\"a\": 1}",
            "POST /idx/_doc\n{\"a\": 1}",
            "DELETE /idx",
            "POST /idx/_update_by_query\n{}",
            "POST /idx/_delete_by_query\n{\"query\": {\"match_all\": {}}}",
            "POST /idx/%5Fdelete_by_query\n{}",
            "POST /idx/_bulk\n{\"delete\": {\"_id\": \"1\"}}",
            "GET /_refresh",
            "GET /idx/_flush",
            "POST /idx/_close",
            "DELETE /_search/scroll\n{\"scroll_id\": \"x\"}",
            "PUT /_cluster/settings\n{}",
            "GET /_cluster/reroute",
            "POST /_security/api_key\n{}",
            "POST /_sql/close\n{}",
            "POST /idx/_search/autre",
        ] {
            assert!(matches!(admit(request, Policy::Prod), Err(DbError::ProdReadsOnly(_))), "{request}");
            assert!(matches!(admit(request, Policy::ReadOnly), Err(DbError::ReadOnlyGuard(_))), "{request}");
            assert_eq!(admit(request, Policy::Writable).unwrap(), Admission::Confirm, "{request}");
        }
        // Un chemin qui ne se lit pas d'une seule façon n'est jamais envoyé.
        for request in ["GET /idx%2F_delete_by_query", "GET /idx/%zz", "GET /idx#/_search", "GET /_cat/../idx/_refresh", "GET /./_cat", "POST /_search/template/../_delete_by_query"] {
            assert!(admit(request, Policy::Writable).is_err(), "{request}");
        }
    }

    fn logs() -> TableShape {
        let field = |name: &str, data_type: &str| Column { name: name.into(), data_type: data_type.into(), primary_key: None };
        TableShape {
            table: TableRef { schema: String::new(), name: "logs-2026.10".into() },
            columns: vec![id_column(), field("level", "keyword"), field("message", "text"), field("at", "date"), field("user", "object")],
        }
    }

    #[test]
    fn a_page_is_a_search_by_offset_with_bound_values() {
        let request = PageRequest { sort: None, filters: vec![], cursor: Cursor::Start };
        let q = search_query(&logs(), &request).unwrap();
        assert_eq!(
            q.sql,
            "POST /logs-2026.10/_search\n{\"size\":201,\"from\":0,\"track_total_hits\":false,\
             \"_source\":[\"level\",\"message\",\"at\",\"user\"],\"sort\":[\"_doc\"]}"
        );
        let filters = vec![
            Filter { column: "level".into(), op: FilterOp::Contains("ERR*\"".into()) },
            Filter { column: "message".into(), op: FilterOp::Equals("disk \"full\"".into()) },
            Filter { column: "_id".into(), op: FilterOp::Equals("a1".into()) },
            Filter { column: "user".into(), op: FilterOp::IsNull },
        ];
        let sort = Some(Sort { column: "at".into(), descending: true });
        let q = search_query(&logs(), &PageRequest { sort, filters, cursor: Cursor::Offset(400) }).unwrap();
        assert!(q.sql.contains("\"from\":400"));
        assert!(q.sql.contains(r#""sort":[{"at":{"order":"desc","missing":"_last"}},"_doc"]"#), "{}", q.sql);
        assert!(q.sql.contains(
            r#""query":{"bool":{"filter":[{"wildcard":{"level":{"value":"*ERR\\*\"*","case_insensitive":true}}},{"match_phrase":{"message":"disk \"full\""}},{"ids":{"values":["a1"]}}],"must_not":[{"exists":{"field":"user"}}]}}"#
        ), "{}", q.sql);
        assert!(q.params.is_empty());
    }

    #[test]
    fn a_page_refuses_what_elasticsearch_cannot_do() {
        let request = |sort: Option<Sort>, filters: Vec<Filter>, cursor| PageRequest { sort, filters, cursor };
        let on = |column: &str| Some(Sort { column: column.into(), descending: false });
        assert!(matches!(search_query(&logs(), &request(on("message"), vec![], Cursor::Start)), Err(DbError::Unsupported(_))));
        assert!(matches!(search_query(&logs(), &request(on("nope"), vec![], Cursor::Start)), Err(DbError::UnknownColumn(_))));
        assert!(matches!(search_query(&logs(), &request(None, vec![], Cursor::Offset(9_900))), Err(DbError::Unsupported(_))));
        assert!(matches!(search_query(&logs(), &request(None, vec![], Cursor::After(vec![]))), Err(DbError::InvalidCursor)));
        let contains = vec![Filter { column: "at".into(), op: FilterOp::Contains("2026".into()) }];
        assert!(matches!(search_query(&logs(), &request(None, contains, Cursor::Start)), Err(DbError::Unsupported(_))));
        // Un nom d'index exotique reste un seul segment.
        let mut odd = logs();
        odd.table.name = "a&b/_delete_by_query".into();
        let q = search_query(&odd, &request(None, vec![], Cursor::Start)).unwrap();
        assert!(q.sql.starts_with("POST /a%26b%2F_delete_by_query/_search\n"));
    }
}

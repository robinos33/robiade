//! Mongo : la console en commandes JSON au format de `runCommand` (`{"find": "users", "filter":
//! {…}}`), et les pages d'une collection.
//!
//! Pas de syntaxe `mongosh` : ce serait embarquer un interpréteur JavaScript. Les valeurs typées
//! s'écrivent en JSON étendu (`{"$oid": "…"}`, `{"$date": "…"}`). Mongo n'a pas de lecture seule de
//! session : sur une connexion en lecture seule, et toujours en prod, la console n'envoie que les
//! commandes d'une liste blanche de lectures. Le texte est analysé une fois, ici ; c'est le
//! document analysé, pas le texte, que l'adapter envoie.

use serde_json::{json, Map, Value};

use crate::databases::DbError;
use crate::db_browse::{Column, Cursor, FilterOp, PageQuery, PageRequest, TableShape, FILTERS_MAX, PAGE_SIZE};
use crate::db_console::{Admission, Policy, StatementKind};

/// Une commande de la console : son nom (la première clé) et le document entier.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub document: Map<String, Value>,
}

/// Commandes de lecture, en minuscules. `aggregate` n'en est une que sans `$out` ni `$merge`, et
/// `explain` que si la commande expliquée en est une.
const READS: &[&str] = &[
    "find", "aggregate", "count", "distinct", "listcollections", "listindexes", "listdatabases", "collstats",
    "dbstats", "explain", "ping", "buildinfo", "connectionstatus", "hello", "ismaster", "serverstatus", "hostinfo",
];
/// Étapes d'agrégation qui écrivent dans une collection.
const WRITING_STAGES: &[&str] = &["$out", "$merge"];
/// Opérateurs qui exécutent du JavaScript sur le serveur : refusés en prod, même dans une lecture.
const SERVER_JS: &[&str] = &["$where", "$function", "$accumulator"];
/// Champs de session et de transaction : ils ouvriraient une transaction laissée en plan.
const SESSION_FIELDS: &[&str] = &["lsid", "txnnumber", "starttransaction", "autocommit"];

/// Bornes (octets) d'un objet JSON de premier niveau dans le texte.
struct Block {
    start: usize,
    end: usize,
}

/// Les objets `{…}` de premier niveau, accolades des chaînes ignorées. Un objet non refermé va
/// jusqu'à la fin du texte. Ce qui est hors des objets (blancs, commentaires `//`) est ignoré.
fn blocks(text: &str) -> Vec<Block> {
    let bytes = text.as_bytes();
    let (mut blocks, mut depth, mut start, mut i) = (Vec::new(), 0usize, 0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'"' if depth > 0 => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if depth == 0 && bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    blocks.push(Block { start, end: i + 1 });
                }
            }
            _ => {}
        }
        i += 1;
    }
    if depth > 0 {
        blocks.push(Block { start, end: text.trim_end().len() });
    }
    blocks
}

/// La commande sous le curseur (en octets) ; entre deux commandes, la précédente.
pub fn statement_at(text: &str, cursor: usize) -> Result<&str, DbError> {
    let blocks = blocks(text);
    let found = blocks.iter().rev().find(|b| b.start <= cursor).or_else(|| blocks.first()).ok_or(DbError::NoStatement)?;
    Ok(&text[found.start..found.end])
}

/// Le texte entier doit tenir en une commande : c'est le cas d'une sélection.
pub fn single_statement(text: &str) -> Result<&str, DbError> {
    match blocks(text).as_slice() {
        [] => Err(DbError::NoStatement),
        [one] => Ok(&text[one.start..one.end]),
        _ => Err(DbError::MultipleStatements),
    }
}

/// Analyse une commande : un objet JSON dont la première clé est le nom.
pub fn parse(statement: &str) -> Result<Command, DbError> {
    let Ok(Value::Object(document)) = serde_json::from_str::<Value>(statement) else {
        return Err(DbError::NotACommand);
    };
    let name = document.keys().next().ok_or(DbError::NotACommand)?.clone();
    Ok(Command { name, document })
}

/// Une clé quelque part dans la valeur, à n'importe quelle profondeur.
fn has_key(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => map
            .iter()
            .find_map(|(k, v)| if keys.contains(&k.as_str()) { Some(k.clone()) } else { has_key(v, keys) }),
        Value::Array(items) => items.iter().find_map(|v| has_key(v, keys)),
        _ => None,
    }
}

fn is_read(name: &str, document: &Map<String, Value>) -> bool {
    let name = name.to_ascii_lowercase();
    if !READS.contains(&name.as_str()) {
        return false;
    }
    match name.as_str() {
        "aggregate" => has_key(&Value::Object(document.clone()), WRITING_STAGES).is_none(),
        "explain" => match document.values().next() {
            Some(Value::Object(inner)) => inner.keys().next().is_some_and(|n| is_read(n, inner)),
            _ => false,
        },
        _ => true,
    }
}

/// Dit si une commande peut partir. Jamais de champ de session ou de transaction, ni de champ
/// `$…` de premier niveau (`$db` changerait de base) ; une lecture de la liste blanche part
/// toujours, sauf en prod si elle exécute du JavaScript ; le reste, après confirmation, et jamais
/// en prod ni sur une connexion en lecture seule.
pub fn admit(statement: &str, policy: Policy) -> Result<Admission, DbError> {
    let command = parse(statement)?;
    if command.document.keys().any(|k| SESSION_FIELDS.contains(&k.to_ascii_lowercase().as_str())) {
        return Err(DbError::TransactionControl);
    }
    if command.document.keys().any(|k| k.starts_with('$')) {
        return Err(DbError::NotACommand);
    }
    if policy == Policy::Prod {
        if let Some(operator) = has_key(&Value::Object(command.document.clone()), SERVER_JS) {
            return Err(DbError::ProdFunction(operator));
        }
    }
    if is_read(&command.name, &command.document) {
        return Ok(Admission::Run(StatementKind::Query));
    }
    match policy {
        Policy::Writable => Ok(Admission::Confirm),
        Policy::ReadOnly => Err(DbError::ReadOnlyGuard("commande hors de la liste des lectures Mongo")),
        Policy::Prod => Err(DbError::ProdReadsOnly(command.name)),
    }
}

/// Une valeur de la grille rendue dans le type de la colonne, en JSON étendu. Un texte qui ne s'y
/// convertit pas reste une chaîne : la comparaison ne trouvera rien, sans erreur.
fn typed(column: &Column, text: &str) -> Value {
    match column.data_type.as_str() {
        "objectId" if text.len() == 24 && text.bytes().all(|b| b.is_ascii_hexdigit()) => json!({ "$oid": text }),
        "int" | "long" => text.parse::<i64>().map_or_else(|_| json!(text), |n| json!(n)),
        "double" | "decimal" => text.parse::<f64>().map_or_else(|_| json!(text), |n| json!(n)),
        "bool" => match text {
            "true" => json!(true),
            "false" => json!(false),
            _ => json!(text),
        },
        "date" => json!({ "$date": text }),
        _ => json!(text),
    }
}

/// Motif d'expression régulière qui cherche `value` telle quelle.
fn literal_regex(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Requête d'une page d'une collection : la commande `find` elle-même, `_id` puis les champs de
/// premier niveau échantillonnés. Par `_id` (keyset) quand on ne trie pas sur autre chose, sinon
/// par décalage.
pub fn find_query(shape: &TableShape, request: &PageRequest) -> Result<PageQuery, DbError> {
    if request.filters.len() > FILTERS_MAX {
        return Err(DbError::TooManyFilters);
    }
    let mut conditions = Vec::new();
    for f in &request.filters {
        let column = shape.column(&f.column)?;
        let field = column.name.clone();
        let condition = match &f.op {
            FilterOp::IsNull => json!({ field: null }),
            FilterOp::IsNotNull => json!({ field: { "$ne": null } }),
            FilterOp::Equals(v) => json!({ field: typed(column, v) }),
            FilterOp::Contains(v) if v.is_empty() => return Err(DbError::EmptyFilter),
            FilterOp::Contains(v) if column.data_type == "string" => {
                json!({ field: { "$regex": literal_regex(v), "$options": "i" } })
            }
            FilterOp::Contains(_) => return Err(DbError::Unsupported("« contient » hors d'un champ texte")),
        };
        conditions.push(condition);
    }
    let id = shape.column("_id")?;
    let by_key = request.sort.as_ref().is_none_or(|s| s.column == "_id");
    let direction = if request.sort.as_ref().is_some_and(|s| s.descending) { -1 } else { 1 };
    let mut command = Map::new();
    command.insert("find".into(), json!(shape.table.name));
    let (sort, offset) = if by_key {
        match &request.cursor {
            Cursor::Start => {}
            Cursor::After(values) if values.len() == 1 => {
                let op = if direction < 0 { "$lt" } else { "$gt" };
                conditions.push(json!({ "_id": { op: typed(id, &values[0]) } }));
            }
            _ => return Err(DbError::InvalidCursor),
        }
        (json!({ "_id": direction }), 0)
    } else {
        let offset = match request.cursor {
            Cursor::Start => 0,
            Cursor::Offset(n) => n,
            Cursor::After(_) => return Err(DbError::InvalidCursor),
        };
        let column = shape.column(&request.sort.as_ref().map(|s| s.column.clone()).unwrap_or_default())?;
        // `_id` départage les égalités : sans ordre total, un document changerait de page.
        let mut sort = Map::new();
        sort.insert(column.name.clone(), json!(direction));
        sort.insert("_id".into(), json!(1));
        (Value::Object(sort), offset)
    };
    let filter = match conditions.len() {
        0 => json!({}),
        1 => conditions.remove(0),
        _ => json!({ "$and": conditions }),
    };
    let projection: Map<String, Value> =
        shape.columns.iter().filter(|c| c.name != "_id").map(|c| (c.name.clone(), json!(1))).collect();
    command.insert("filter".into(), filter);
    command.insert("projection".into(), Value::Object(projection));
    command.insert("sort".into(), sort);
    if offset > 0 {
        command.insert("skip".into(), json!(offset));
    }
    command.insert("limit".into(), json!(PAGE_SIZE + 1));
    command.insert("singleBatch".into(), json!(true));
    let text = Value::Object(command).to_string();
    Ok(if by_key { PageQuery::by_key(text) } else { PageQuery::by_offset(text, offset) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_browse::{Filter, Sort, TableRef};

    #[test]
    fn splits_top_level_objects_ignoring_braces_in_strings() {
        let text = "// commandes\n{\"find\": \"users\", \"filter\": {\"name\": \"a}b{\"}}\n\n{\"count\": \"orders\"}\n{\"find\": ";
        assert_eq!(statement_at(text, 20).unwrap(), "{\"find\": \"users\", \"filter\": {\"name\": \"a}b{\"}}");
        let count = text.find("{\"count").unwrap();
        assert_eq!(statement_at(text, count + 3).unwrap(), "{\"count\": \"orders\"}");
        // Pas refermée : jusqu'à la fin, et l'analyse dira ce qui ne va pas.
        assert_eq!(statement_at(text, text.len()).unwrap(), "{\"find\":");
        assert!(matches!(parse("{\"find\":"), Err(DbError::NotACommand)));
        assert!(matches!(single_statement("{} {}"), Err(DbError::MultipleStatements)));
        assert_eq!(parse("{\"find\": \"u\", \"limit\": 1}").unwrap().name, "find");
        assert!(matches!(parse("[1]"), Err(DbError::NotACommand)));
    }

    #[test]
    fn reads_pass_everywhere_and_the_rest_only_where_writes_are_allowed() {
        for command in [
            r#"{"find": "users", "filter": {"age": {"$gt": 30}}}"#,
            r#"{"aggregate": "orders", "pipeline": [{"$group": {"_id": "$status"}}], "cursor": {}}"#,
            r#"{"count": "users"}"#,
            r#"{"listCollections": 1}"#,
            r#"{"explain": {"find": "users"}, "verbosity": "executionStats"}"#,
            r#"{"Find": "users"}"#,
        ] {
            for policy in [Policy::ReadOnly, Policy::Prod, Policy::Writable] {
                assert_eq!(admit(command, policy).unwrap(), Admission::Run(StatementKind::Query), "{command}");
            }
        }
        for command in [
            r#"{"insert": "users", "documents": [{}]}"#,
            r#"{"delete": "users", "deletes": [{"q": {}, "limit": 0}]}"#,
            r#"{"drop": "users"}"#,
            r#"{"aggregate": "orders", "pipeline": [{"$match": {}}, {"$out": "copie"}], "cursor": {}}"#,
            r#"{"aggregate": "orders", "pipeline": [{"$merge": {"into": "x"}}], "cursor": {}}"#,
            r#"{"explain": {"delete": "users", "deletes": []}}"#,
            r#"{"findAndModify": "users", "query": {}, "remove": true}"#,
            r#"{"mapReduce": "users", "map": "", "reduce": "", "out": "x"}"#,
            r#"{"drop": "users"}"#,
        ] {
            assert!(matches!(admit(command, Policy::Prod), Err(DbError::ProdReadsOnly(_))), "{command}");
            assert!(matches!(admit(command, Policy::ReadOnly), Err(DbError::ReadOnlyGuard(_))), "{command}");
            assert_eq!(admit(command, Policy::Writable).unwrap(), Admission::Confirm, "{command}");
        }
    }

    #[test]
    fn refuses_sessions_other_databases_and_server_javascript_in_prod() {
        for command in [r#"{"find": "u", "startTransaction": true}"#, r#"{"find": "u", "lsid": {}, "txnNumber": 1}"#] {
            assert!(matches!(admit(command, Policy::Writable), Err(DbError::TransactionControl)), "{command}");
        }
        assert!(matches!(admit(r#"{"drop": "u", "$db": "admin"}"#, Policy::Writable), Err(DbError::NotACommand)));
        for command in [
            r#"{"find": "u", "filter": {"$where": "this.a > 1"}}"#,
            r#"{"aggregate": "u", "pipeline": [{"$addFields": {"x": {"$function": {"body": "", "args": [], "lang": "js"}}}}], "cursor": {}}"#,
        ] {
            assert!(matches!(admit(command, Policy::Prod), Err(DbError::ProdFunction(_))), "{command}");
            assert!(admit(command, Policy::ReadOnly).is_ok(), "{command}");
        }
    }

    fn users() -> TableShape {
        let field = |name: &str, data_type: &str| Column { name: name.into(), data_type: data_type.into(), primary_key: None };
        TableShape {
            table: TableRef { schema: String::new(), name: "users".into() },
            columns: vec![
                Column { name: "_id".into(), data_type: "objectId".into(), primary_key: Some(1) },
                field("email", "string"),
                field("age", "int"),
                field("profile", "object"),
            ],
        }
    }

    #[test]
    fn a_page_is_a_find_by_id_with_typed_values() {
        let first = find_query(&users(), &PageRequest { sort: None, filters: vec![], cursor: Cursor::Start }).unwrap();
        assert_eq!(
            first.sql,
            r#"{"find":"users","filter":{},"projection":{"email":1,"age":1,"profile":1},"sort":{"_id":1},"limit":201,"singleBatch":true}"#
        );
        let filters = vec![
            Filter { column: "email".into(), op: FilterOp::Contains("a.b+".into()) },
            Filter { column: "age".into(), op: FilterOp::Equals("42".into()) },
            Filter { column: "profile".into(), op: FilterOp::IsNull },
        ];
        let after = Cursor::After(vec!["65f1a2b3c4d5e6f708091a2b".into()]);
        let next = find_query(&users(), &PageRequest { sort: None, filters, cursor: after }).unwrap();
        assert!(next.sql.contains(
            r#""filter":{"$and":[{"email":{"$regex":"a\\.b\\+","$options":"i"}},{"age":42},{"profile":null},{"_id":{"$gt":{"$oid":"65f1a2b3c4d5e6f708091a2b"}}}]}"#
        ), "{}", next.sql);
    }

    #[test]
    fn sorting_on_a_field_pages_by_offset() {
        let sort = Some(Sort { column: "age".into(), descending: true });
        let q = find_query(&users(), &PageRequest { sort, filters: vec![], cursor: Cursor::Offset(200) }).unwrap();
        assert!(q.sql.contains(r#""sort":{"age":-1,"_id":1},"skip":200,"limit":201"#), "{}", q.sql);
        let contains = vec![Filter { column: "age".into(), op: FilterOp::Contains("4".into()) }];
        let refused = find_query(&users(), &PageRequest { sort: None, filters: contains, cursor: Cursor::Start });
        assert!(matches!(refused, Err(DbError::Unsupported(_))));
        let back = Some(Sort { column: "_id".into(), descending: true });
        let q = find_query(&users(), &PageRequest { sort: back, filters: vec![], cursor: Cursor::After(vec!["x".into()]) }).unwrap();
        assert!(q.sql.contains(r#"{"_id":{"$lt":"x"}}"#) && q.sql.contains(r#""sort":{"_id":-1}"#), "{}", q.sql);
    }
}

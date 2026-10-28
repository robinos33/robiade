//! Parcourir une base : ses tables, puis leurs lignes page par page, triées et filtrées.
//!
//! L'interface ne fournit jamais de SQL. Elle désigne une table, une colonne de tri et des filtres ;
//! ce module vérifie chaque nom contre le catalogue lu sur le serveur, les cite, et passe toutes
//! les valeurs en paramètres. Une page est bornée : on ne charge jamais une table entière.

use crate::databases::{DbError, Engine, Target, Tls};

/// Lignes par page. Une de plus est demandée pour savoir s'il en reste.
pub const PAGE_SIZE: usize = 200;
/// Au-delà, une cellule est tronquée côté serveur : 200 lignes de documents JSON tiendraient sinon
/// des centaines de mégaoctets en mémoire.
pub const CELL_MAX_CHARS: usize = 10_000;
pub const FILTERS_MAX: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    Table,
    View,
    MaterializedView,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub table: TableRef,
    pub kind: RelationKind,
    /// Estimation du planificateur (`pg_class.reltuples`), pas un comptage : absente si jamais analysée.
    pub estimated_rows: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Type tel que le serveur le nomme (`format_type`), par ex. `character varying(255)`.
    pub data_type: String,
    /// Rang dans la clé primaire, à partir de 1.
    pub primary_key: Option<u16>,
}

/// Une table telle que le catalogue la décrit : la seule source des noms admis dans une requête.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableShape {
    pub table: TableRef,
    pub columns: Vec<Column>,
}

impl TableShape {
    fn column(&self, name: &str) -> Result<&Column, DbError> {
        self.columns.iter().find(|c| c.name == name).ok_or_else(|| DbError::UnknownColumn(name.to_owned()))
    }

    /// Colonnes de la clé primaire, dans l'ordre de la clé.
    fn primary_key(&self) -> Vec<(usize, &Column)> {
        let mut key: Vec<_> = self.columns.iter().enumerate().filter(|(_, c)| c.primary_key.is_some()).collect();
        key.sort_by_key(|(_, c)| c.primary_key);
        key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterOp {
    Equals(String),
    /// Sans tenir compte de la casse ; `%` et `_` sont pris à la lettre.
    Contains(String),
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub column: String,
    pub descending: bool,
}

/// Où reprendre. `After` porte les valeurs de clé primaire de la dernière ligne vue (keyset) ;
/// `Offset`, le nombre de lignes déjà vues quand aucune clé ne permet mieux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    Start,
    After(Vec<String>),
    Offset(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    pub sort: Option<Sort>,
    pub filters: Vec<Filter>,
    pub cursor: Cursor,
}

/// Requête prête à envoyer : `params` remplit `$1…$n`, tous en texte, convertis par le serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageQuery {
    pub sql: String,
    pub params: Vec<String>,
    /// Colonnes de la clé, par rang dans le résultat, quand la page avance par keyset.
    keyset: Option<Vec<usize>>,
    offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    /// La valeur dépassait `CELL_MAX_CHARS` : seul le début est là.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub columns: Vec<Column>,
    /// `None` : NULL, distinct de la chaîne vide.
    pub rows: Vec<Vec<Option<Cell>>>,
    /// Absent : c'était la dernière page.
    pub next: Option<Cursor>,
}

/// Types dont la valeur texte se reconvertit sans perte et dont l'ordre est celui du type : une
/// comparaison s'y fait dans le type de la colonne, donc avec son index. Les autres se comparent
/// en texte, et ne servent pas de clé de pagination.
fn castable(data_type: &str) -> bool {
    const EXACT: &[&str] = &[
        "smallint", "integer", "bigint", "numeric", "uuid", "text", "boolean", "date",
        "timestamp without time zone", "timestamp with time zone",
    ];
    const PREFIXED: &[&str] = &["character varying(", "character(", "numeric("];
    EXACT.contains(&data_type)
        || data_type == "character varying"
        || PREFIXED.iter().any(|p| {
            data_type.strip_prefix(p).and_then(|rest| rest.strip_suffix(')')).is_some_and(|args| {
                args.chars().all(|c| c.is_ascii_digit() || c == ',')
            })
        })
}

/// Identifiant cité : `"` doublé, rien d'autre à échapper entre guillemets.
fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// `$n` vu dans le type de la colonne quand c'est sûr ; le type vient du catalogue et `castable`
/// n'en laisse passer que des formes connues.
fn param_as(column: &Column, n: usize) -> String {
    if castable(&column.data_type) {
        format!("CAST(${n}::text AS {})", column.data_type)
    } else {
        format!("${n}::text")
    }
}

fn compared(column: &Column) -> String {
    if castable(&column.data_type) { quote(&column.name) } else { format!("{}::text", quote(&column.name)) }
}

/// Ce que la grille affiche d'une colonne : le texte que le serveur produit pour le type, borné ;
/// un binaire est résumé à sa taille.
fn selected(column: &Column) -> String {
    let c = quote(&column.name);
    if column.data_type == "bytea" {
        format!("CASE WHEN {c} IS NULL THEN NULL ELSE '<' || octet_length({c}) || ' octets>' END")
    } else {
        format!("left({c}::text, {})", CELL_MAX_CHARS + 1)
    }
}

/// Motif `ILIKE` qui cherche `value` telle quelle.
fn contains_pattern(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// La page avance par keyset si la table a une clé primaire comparable dans son type, et que le tri
/// demandé est celui de la clé (aucun, ou sur la clé quand elle n'a qu'une colonne).
fn keyset<'a>(shape: &'a TableShape, sort: Option<&Sort>) -> Option<Vec<(usize, &'a Column)>> {
    let key = shape.primary_key();
    if key.is_empty() || !key.iter().all(|(_, c)| castable(&c.data_type)) {
        return None;
    }
    match sort {
        None => Some(key),
        Some(s) if key.len() == 1 && key[0].1.name == s.column => Some(key),
        Some(_) => None,
    }
}

/// Construit la requête d'une page. Toute colonne citée doit exister dans `shape`.
pub fn page_query(shape: &TableShape, request: &PageRequest) -> Result<PageQuery, DbError> {
    if shape.columns.is_empty() {
        return Err(DbError::UnknownColumn(shape.table.name.clone()));
    }
    if request.filters.len() > FILTERS_MAX {
        return Err(DbError::TooManyFilters);
    }
    let mut params = Vec::new();
    let mut conditions = Vec::new();
    for filter in &request.filters {
        let column = shape.column(&filter.column)?;
        let condition = match &filter.op {
            FilterOp::IsNull => format!("{} IS NULL", quote(&column.name)),
            FilterOp::IsNotNull => format!("{} IS NOT NULL", quote(&column.name)),
            FilterOp::Equals(value) => {
                params.push(value.clone());
                format!("{} = {}", compared(column), param_as(column, params.len()))
            }
            FilterOp::Contains(value) if value.is_empty() => return Err(DbError::EmptyFilter),
            FilterOp::Contains(value) => {
                params.push(contains_pattern(value));
                format!("{}::text ILIKE ${}::text ESCAPE '\\'", quote(&column.name), params.len())
            }
        };
        conditions.push(condition);
    }

    let sort = request.sort.as_ref();
    if let Some(s) = sort {
        shape.column(&s.column)?;
    }
    let descending = sort.is_some_and(|s| s.descending);
    let direction = if descending { "DESC" } else { "ASC" };
    let key = keyset(shape, sort);

    let (order, keyset_columns, offset) = match &key {
        Some(key) => {
            if let Cursor::After(values) = &request.cursor {
                if values.len() != key.len() {
                    return Err(DbError::InvalidCursor);
                }
                let mut bound = Vec::new();
                for ((_, column), value) in key.iter().zip(values) {
                    params.push(value.clone());
                    bound.push(param_as(column, params.len()));
                }
                let names: Vec<String> = key.iter().map(|(_, c)| quote(&c.name)).collect();
                let op = if descending { "<" } else { ">" };
                conditions.push(format!("({}) {op} ({})", names.join(", "), bound.join(", ")));
            } else if request.cursor != Cursor::Start {
                return Err(DbError::InvalidCursor);
            }
            let order: Vec<String> = key.iter().map(|(_, c)| format!("{} {direction}", quote(&c.name))).collect();
            (order, Some(key.iter().map(|(i, _)| *i).collect()), 0)
        }
        None => {
            let offset = match request.cursor {
                Cursor::Start => 0,
                Cursor::Offset(n) => n,
                Cursor::After(_) => return Err(DbError::InvalidCursor),
            };
            // La clé, quand il y en a une, départage les égalités : sans ordre total, une ligne
            // pourrait passer d'une page à l'autre entre deux requêtes.
            let mut order: Vec<String> = sort
                .map(|s| format!("{} {direction} NULLS LAST", quote(&s.column)))
                .into_iter()
                .collect();
            order.extend(shape.primary_key().iter().map(|(_, c)| quote(&c.name)));
            (order, None, offset)
        }
    };

    let columns: Vec<String> = shape.columns.iter().map(selected).collect();
    let mut sql = format!(
        "SELECT {} FROM {}.{}",
        columns.join(", "),
        quote(&shape.table.schema),
        quote(&shape.table.name)
    );
    if !conditions.is_empty() {
        sql.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
    }
    if !order.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    sql.push_str(&format!(" LIMIT {}", PAGE_SIZE + 1));
    if offset > 0 {
        sql.push_str(&format!(" OFFSET {offset}"));
    }
    Ok(PageQuery { sql, params, keyset: keyset_columns, offset })
}

/// Met en forme les lignes reçues pour `query` : cellules bornées, et le curseur de la suite.
pub fn page(shape: &TableShape, query: &PageQuery, mut rows: Vec<Vec<Option<String>>>) -> Page {
    let more = rows.len() > PAGE_SIZE;
    rows.truncate(PAGE_SIZE);
    let next = match (&query.keyset, rows.last()) {
        _ if !more => None,
        (Some(key), Some(last)) => {
            let values: Option<Vec<String>> = key.iter().map(|&i| last.get(i).cloned().flatten()).collect();
            // Une clé tronquée ne permettrait pas de reprendre au bon endroit : on s'arrête là.
            values.filter(|v| v.iter().all(|s| s.chars().count() <= CELL_MAX_CHARS)).map(Cursor::After)
        }
        (Some(_), None) => None,
        (None, _) => Some(Cursor::Offset(query.offset + rows.len() as u64)),
    };
    let rows = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| {
                    value.map(|text| match text.char_indices().nth(CELL_MAX_CHARS) {
                        Some((cut, _)) => Cell { text: text[..cut].to_owned(), truncated: true },
                        None => Cell { text, truncated: false },
                    })
                })
                .collect()
        })
        .collect();
    Page { columns: shape.columns.clone(), rows, next }
}

/// Une session ouverte sur une base, en lecture seule si demandé.
pub trait DbSession: Send {
    /// Lu sur le serveur, pas déduit de la demande.
    fn read_only(&mut self) -> Result<bool, DbError>;
    fn current_database(&mut self) -> Result<String, DbError>;
    /// Bases du serveur auxquelles on peut se connecter.
    fn databases(&mut self) -> Result<Vec<String>, DbError>;
    /// Tables et vues de la base courante, hors schémas système.
    fn relations(&mut self) -> Result<Vec<Relation>, DbError>;
    fn describe(&mut self, table: &TableRef) -> Result<TableShape, DbError>;
    /// Exécute une requête de page ; chaque valeur arrive en texte, `None` pour NULL.
    fn fetch(&mut self, query: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError>;
}

pub trait DatabaseBrowser: Send + Sync {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str, primary_key: Option<u16>) -> Column {
        Column { name: name.into(), data_type: data_type.into(), primary_key }
    }

    fn users() -> TableShape {
        TableShape {
            table: TableRef { schema: "public".into(), name: "user".into() },
            columns: vec![
                column("id", "integer", Some(1)),
                column("email", "character varying(180)", None),
                column("roles", "json", None),
                column("avatar", "bytea", None),
            ],
        }
    }

    fn request(sort: Option<Sort>, filters: Vec<Filter>, cursor: Cursor) -> PageRequest {
        PageRequest { sort, filters, cursor }
    }

    const SELECT: &str = "SELECT left(\"id\"::text, 10001), left(\"email\"::text, 10001), left(\"roles\"::text, 10001), \
        CASE WHEN \"avatar\" IS NULL THEN NULL ELSE '<' || octet_length(\"avatar\") || ' octets>' END \
        FROM \"public\".\"user\"";

    #[test]
    fn pages_by_primary_key_when_nothing_else_is_asked() {
        let first = page_query(&users(), &request(None, vec![], Cursor::Start)).unwrap();
        assert_eq!(first.sql, format!("{SELECT} ORDER BY \"id\" ASC LIMIT 201"));
        assert!(first.params.is_empty());

        let next = page_query(&users(), &request(None, vec![], Cursor::After(vec!["42".into()]))).unwrap();
        assert_eq!(next.sql, format!("{SELECT} WHERE (\"id\") > (CAST($1::text AS integer)) ORDER BY \"id\" ASC LIMIT 201"));
        assert_eq!(next.params, ["42"]);

        let sort = Some(Sort { column: "id".into(), descending: true });
        let back = page_query(&users(), &request(sort, vec![], Cursor::After(vec!["42".into()]))).unwrap();
        assert!(back.sql.contains("WHERE (\"id\") < (CAST($1::text AS integer)) ORDER BY \"id\" DESC"));
    }

    #[test]
    fn falls_back_to_offset_when_sorting_on_another_column() {
        let sort = Some(Sort { column: "email".into(), descending: false });
        let q = page_query(&users(), &request(sort, vec![], Cursor::Offset(400))).unwrap();
        assert_eq!(q.sql, format!("{SELECT} ORDER BY \"email\" ASC NULLS LAST, \"id\" LIMIT 201 OFFSET 400"));
        // Un curseur keyset n'a pas de sens ici.
        let sort = Some(Sort { column: "email".into(), descending: false });
        assert_eq!(page_query(&users(), &request(sort, vec![], Cursor::After(vec!["1".into()]))), Err(DbError::InvalidCursor));
    }

    #[test]
    fn filters_are_bound_never_interpolated() {
        let filters = vec![
            Filter { column: "id".into(), op: FilterOp::Equals("7".into()) },
            Filter { column: "email".into(), op: FilterOp::Contains("50%_off\\'; DROP".into()) },
            Filter { column: "roles".into(), op: FilterOp::Equals("[]".into()) },
            Filter { column: "avatar".into(), op: FilterOp::IsNull },
        ];
        let q = page_query(&users(), &request(None, filters, Cursor::Start)).unwrap();
        assert_eq!(
            q.sql,
            format!(
                "{SELECT} WHERE \"id\" = CAST($1::text AS integer) AND \"email\"::text ILIKE $2::text ESCAPE '\\' \
                 AND \"roles\"::text = $3::text AND \"avatar\" IS NULL ORDER BY \"id\" ASC LIMIT 201"
            )
        );
        assert_eq!(q.params, ["7", "%50\\%\\_off\\\\'; DROP%", "[]"]);
    }

    #[test]
    fn refuses_names_absent_from_the_catalog() {
        let injected = Filter { column: "id\" OR 1=1 --".into(), op: FilterOp::IsNull };
        assert_eq!(
            page_query(&users(), &request(None, vec![injected], Cursor::Start)),
            Err(DbError::UnknownColumn("id\" OR 1=1 --".into()))
        );
        let sort = Some(Sort { column: "nope".into(), descending: false });
        assert!(page_query(&users(), &request(sort, vec![], Cursor::Start)).is_err());
        let empty = Filter { column: "email".into(), op: FilterOp::Contains(String::new()) };
        assert_eq!(page_query(&users(), &request(None, vec![empty], Cursor::Start)), Err(DbError::EmptyFilter));
    }

    #[test]
    fn quotes_odd_identifiers() {
        let shape = TableShape {
            table: TableRef { schema: "My\"Schema".into(), name: "t".into() },
            columns: vec![column("a\"b", "text", None)],
        };
        let q = page_query(&shape, &request(None, vec![], Cursor::Start)).unwrap();
        assert_eq!(q.sql, "SELECT left(\"a\"\"b\"::text, 10001) FROM \"My\"\"Schema\".\"t\" LIMIT 201");
    }

    #[test]
    fn composite_or_uncomparable_keys() {
        let mut shape = users();
        shape.columns[1].primary_key = Some(2);
        let q = page_query(&shape, &request(None, vec![], Cursor::After(vec!["1".into(), "a@b".into()]))).unwrap();
        assert!(q.sql.contains(
            "WHERE (\"id\", \"email\") > (CAST($1::text AS integer), CAST($2::text AS character varying(180))) \
             ORDER BY \"id\" ASC, \"email\" ASC"
        ));
        // Une clé json ne s'ordonne pas comme son texte : pagination par décalage.
        let mut shape = users();
        shape.columns[0].primary_key = None;
        shape.columns[2].primary_key = Some(1);
        let q = page_query(&shape, &request(None, vec![], Cursor::Start)).unwrap();
        assert!(q.sql.ends_with("ORDER BY \"roles\" LIMIT 201"));
    }

    #[test]
    fn shapes_a_page_and_its_cursor() {
        let row = |id: &str| vec![Some(id.to_owned()), Some(String::new()), None, Some("<3 octets>".into())];
        let q = page_query(&users(), &request(None, vec![], Cursor::Start)).unwrap();
        let rows: Vec<_> = (1..=PAGE_SIZE + 1).map(|i| row(&i.to_string())).collect();
        let p = page(&users(), &q, rows);
        assert_eq!(p.rows.len(), PAGE_SIZE);
        assert_eq!(p.next, Some(Cursor::After(vec!["200".into()])));
        // NULL et chaîne vide restent distincts.
        assert_eq!(p.rows[0][1], Some(Cell { text: String::new(), truncated: false }));
        assert_eq!(p.rows[0][2], None);

        let last = page(&users(), &q, vec![row("1")]);
        assert_eq!(last.next, None);

        let long = "é".repeat(CELL_MAX_CHARS + 1);
        let cut = page(&users(), &q, vec![vec![Some("1".into()), Some(long), None, None]]);
        let cell = cut.rows[0][1].as_ref().unwrap();
        assert!(cell.truncated && cell.text.chars().count() == CELL_MAX_CHARS);

        let sort = Some(Sort { column: "email".into(), descending: false });
        let by_offset = page_query(&users(), &request(sort, vec![], Cursor::Offset(200))).unwrap();
        let rows: Vec<_> = (0..=PAGE_SIZE).map(|i| row(&i.to_string())).collect();
        assert_eq!(page(&users(), &by_offset, rows).next, Some(Cursor::Offset(400)));
    }
}

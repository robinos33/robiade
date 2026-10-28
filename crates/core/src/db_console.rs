//! Console : une instruction SQL écrite à la main, exécutée seule.
//!
//! Ce module découpe le texte de l'éditeur en instructions, choisit celle sous le curseur, la
//! classe (lecture ou écriture) et refuse sur une connexion en lecture seule celles qui pourraient
//! lever ce verrou.
//!
//! Limite à connaître : la lecture seule de session (`default_transaction_read_only` pour
//! Postgres, `SET SESSION TRANSACTION READ ONLY` pour MariaDB), posée à l'ouverture, arrête une
//! écriture accidentelle, mais le serveur accepte qu'une instruction de la session la lève. Le
//! refus ci-dessous, puis la relecture de l'état après chaque instruction (côté application), sont
//! donc ce qui tient la lecture seule d'une console ouverte avec un compte qui peut écrire. Une
//! analyse lexicale ne voit pas l'intérieur d'une fonction : seul un compte en lecture seule côté
//! serveur est un vrai verrou.
//!
//! Le découpage suit le dialecte : un texte que le serveur lirait autrement qu'ici (une chaîne qui
//! finit plus tôt, un commentaire qui n'en est pas un) cacherait une partie de l'instruction aux
//! garde-fous. Pour MariaDB, il suppose `sql_mode` sans `ANSI_QUOTES` ni `NO_BACKSLASH_ESCAPES`, ce
//! que la session impose à l'ouverture.

use crate::databases::{DbError, Dialect, Language, Stage};
use crate::db_browse::{cell, Cell, Column};
use crate::merge_actions::AuditEntry;
use crate::Millis;

/// Lignes affichées au plus ; une de plus est lue pour savoir que le résultat est tronqué.
pub const ROWS_MAX: usize = 1000;
/// Requêtes gardées dans l'historique d'une connexion.
pub const HISTORY_MAX: usize = 200;
/// Une modification de prod non validée dans ce délai est annulée : ses verrous de ligne ne
/// restent pas posés parce qu'on a oublié la fenêtre.
pub const PROD_WRITE_DELAY_SECS: u64 = 60;

/// Ce que la console permet sur une connexion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Lecture seule hors prod : le serveur refuse les écritures.
    ReadOnly,
    /// Dev ou staging sans lecture seule : chaque écriture est confirmée.
    Writable,
    /// Lecture, et INSERT, UPDATE ou DELETE validés en deux temps ; rien d'autre.
    Prod,
}

impl Policy {
    pub fn of(stage: Stage, read_only: bool) -> Self {
        match (stage, read_only) {
            (Stage::Prod, _) => Policy::Prod,
            (_, true) => Policy::ReadOnly,
            (_, false) => Policy::Writable,
        }
    }
}

/// Ce que devient une instruction admise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Part telle quelle.
    Run(StatementKind),
    /// Écriture hors prod : confirmée sur l'instruction exacte, puis envoyée.
    Confirm,
    /// INSERT, UPDATE ou DELETE de prod : confirmé en tapant le nom de la connexion, exécuté dans
    /// une transaction laissée ouverte, puis validé ou annulé une fois le nombre de lignes connu.
    ProdWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// SELECT, VALUES, TABLE, WITH … SELECT : se lit par curseur, ligne par ligne.
    Query,
    /// EXPLAIN (sans ANALYZE d'une écriture), SHOW : lecture, sans curseur possible.
    Read,
    /// Tout le reste : confirmé avant envoi sur une connexion qui peut écrire.
    Write,
}

/// Un morceau lexical hors chaînes et commentaires.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// Mot-clé ou identifiant, en majuscules ; un identifiant entre guillemets aussi.
    Word(String),
    Open,
    Close,
    /// Commentaire exécutable de MariaDB, `/*! … */` ou `/*M! … */` : le serveur en exécute le
    /// contenu.
    Executable,
    Other,
}

/// Une instruction du texte : sa position (octets, sans le `;`) et ses mots.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    /// Fin du segment, `;` compris : un curseur juste après le `;` vise encore cette instruction.
    reach: usize,
    tokens: Vec<Token>,
}

/// Découpe sur `;`, en ignorant chaînes, identifiants cités et commentaires. Postgres : '…', E'…',
/// "identifiant", `--`, `/* */` imbriqués et blocs `$tag$ … $tag$`. MariaDB : '…' et "…" où `\`
/// échappe, `identifiant`, `#`, `-- ` (suivi d'un blanc) et `/* */` non imbriqués. Les segments
/// vides sont écartés.
fn spans(text: &str, dialect: Dialect) -> Vec<Span> {
    let maria = dialect == Dialect::MariaDb;
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut tokens = Vec::new();
    // Premier et dernier octet utiles du segment en cours : ni blancs ni commentaires autour.
    let (mut first, mut last) = (None::<usize>, 0);
    let mut i = 0;
    fn close(spans: &mut Vec<Span>, tokens: &mut Vec<Token>, first: Option<usize>, last: usize, reach: usize) {
        if let Some(first) = first {
            spans.push(Span { start: first, end: last, reach, tokens: std::mem::take(tokens) });
        }
    }
    while i < bytes.len() {
        let b = bytes[i];
        let at = i;
        match b {
            b';' => {
                close(&mut spans, &mut tokens, first, last, i + 1);
                first = None;
                i += 1;
                continue;
            }
            // Pour MariaDB, `--` n'ouvre un commentaire que suivi d'un blanc : `1--1` vaut 2.
            b'-' if bytes.get(i + 1) == Some(&b'-')
                && (!maria || bytes.get(i + 2).is_none_or(|c| c.is_ascii_whitespace() || c.is_ascii_control())) =>
            {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'#' if maria => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if maria && bytes.get(i + 1) == Some(&b'*') => {
                let executable = text[i + 2..].starts_with('!') || text[i + 2..].starts_with("M!");
                i = text[i + 2..].find("*/").map_or(bytes.len(), |n| i + 2 + n + 2);
                if !executable {
                    continue;
                }
                tokens.push(Token::Executable);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut depth = 0;
                while i < bytes.len() {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            _ if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'\'' | b'"' if maria => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        q if q == b && bytes.get(i + 1) == Some(&b) => i += 2,
                        q if q == b => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                tokens.push(Token::Other);
            }
            b'`' if maria => {
                let mut name = String::new();
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'`' {
                        if bytes.get(i + 1) == Some(&b'`') {
                            name.push('`');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    let c = text[i..].chars().next().unwrap_or_default();
                    name.push(c);
                    i += c.len_utf8().max(1);
                }
                tokens.push(Token::Word(name.to_uppercase()));
            }
            b'\'' => {
                // E'…' : la barre oblique échappe. Le préfixe vient d'être lu comme un mot.
                let escapes = matches!(tokens.last(), Some(Token::Word(w)) if w == "E") && at > 0 && bytes[at - 1].eq_ignore_ascii_case(&b'e');
                if escapes {
                    tokens.pop();
                }
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if escapes => i += 2,
                        b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
                        b'\'' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                tokens.push(Token::Other);
            }
            b'"' => {
                let mut name = String::new();
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        if bytes.get(i + 1) == Some(&b'"') {
                            name.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    let c = text[i..].chars().next().unwrap_or_default();
                    name.push(c);
                    i += c.len_utf8().max(1);
                }
                tokens.push(Token::Word(name.to_uppercase()));
            }
            b'$' if !maria => {
                // `$tag$` ouvre un bloc ; `$1` est un paramètre.
                let tag_end = text[i + 1..]
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .map(|n| i + 1 + n);
                let opens = tag_end.is_some_and(|e| bytes[e] == b'$' && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
                if let (true, Some(e)) = (opens, tag_end) {
                    let tag = &text[i..=e];
                    i = text[e + 1..].find(tag).map_or(bytes.len(), |n| e + 1 + n + tag.len());
                } else {
                    i += 1;
                }
                tokens.push(Token::Other);
            }
            b'(' => {
                i += 1;
                tokens.push(Token::Open);
            }
            b')' => {
                i += 1;
                tokens.push(Token::Close);
            }
            _ => {
                let c = text[i..].chars().next().unwrap_or_default();
                if c.is_alphabetic() || c == '_' {
                    let end = text[i..]
                        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                        .map_or(bytes.len(), |n| i + n);
                    tokens.push(Token::Word(text[i..end].to_uppercase()));
                    i = end;
                } else {
                    i += c.len_utf8().max(1);
                    tokens.push(Token::Other);
                }
            }
        }
        first.get_or_insert(at);
        last = i.min(bytes.len());
    }
    close(&mut spans, &mut tokens, first, last, bytes.len());
    spans
}

/// Position en octets d'une position de l'éditeur, comptée en unités UTF-16 comme en JavaScript.
pub fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// L'instruction sous le curseur (en octets) : celle qui le contient, `;` compris ; entre deux
/// instructions, la précédente.
pub fn statement_at(text: &str, language: impl Into<Language>, cursor: usize) -> Result<&str, DbError> {
    let dialect = match language.into() {
        Language::Sql(dialect) => dialect,
        Language::Elastic => return crate::db_elastic::statement_at(text, cursor),
    };
    let spans = spans(text, dialect);
    let found = spans
        .iter()
        .find(|s| s.start <= cursor && cursor <= s.reach)
        .or_else(|| spans.iter().rev().find(|s| s.reach <= cursor))
        .or_else(|| spans.first())
        .ok_or(DbError::NoStatement)?;
    Ok(&text[found.start..found.end])
}

/// Le texte entier doit tenir en une instruction : c'est le cas d'une sélection.
pub fn single_statement(text: &str, language: impl Into<Language>) -> Result<&str, DbError> {
    let dialect = match language.into() {
        Language::Sql(dialect) => dialect,
        Language::Elastic => return crate::db_elastic::single_statement(text),
    };
    match spans(text, dialect).as_slice() {
        [] => Err(DbError::NoStatement),
        [one] => Ok(&text[one.start..one.end]),
        _ => Err(DbError::MultipleStatements),
    }
}

fn words(tokens: &[Token]) -> impl Iterator<Item = &str> {
    tokens.iter().filter_map(|t| match t {
        Token::Word(w) => Some(w.as_str()),
        _ => None,
    })
}

/// Mots à hauteur de parenthèses nulle : ceux de l'instruction principale.
fn top_words(tokens: &[Token]) -> Vec<&str> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for t in tokens {
        match t {
            Token::Open => depth += 1,
            Token::Close => depth -= 1,
            Token::Word(w) if depth == 0 => out.push(w.as_str()),
            _ => {}
        }
    }
    out
}

fn classify(tokens: &[Token]) -> StatementKind {
    let top = top_words(tokens);
    match top.first().copied() {
        // SELECT … INTO crée une table.
        Some("SELECT") if !top.contains(&"INTO") => StatementKind::Query,
        Some("VALUES" | "TABLE") => StatementKind::Query,
        // Un WITH peut porter une écriture dans l'une de ses clauses.
        Some("WITH")
            if !words(tokens).any(|w| matches!(w, "INSERT" | "UPDATE" | "DELETE" | "MERGE")) && !top.contains(&"INTO") =>
        {
            StatementKind::Query
        }
        // DESCRIBE, DESC et HELP : MariaDB.
        Some("SHOW" | "DESCRIBE" | "DESC" | "HELP") => StatementKind::Read,
        Some("EXPLAIN") => {
            // Options : liste entre parenthèses, ou mots-clés de l'ancienne syntaxe.
            let mut rest = &tokens[1..];
            let mut analyze = false;
            if rest.first() == Some(&Token::Open) {
                let close = rest.iter().position(|t| *t == Token::Close).unwrap_or(rest.len() - 1);
                analyze = words(&rest[..close]).any(|w| matches!(w, "ANALYZE" | "ANALYSE"));
                rest = &rest[(close + 1).min(rest.len())..];
            }
            while let Some(Token::Word(w)) = rest.first() {
                match w.as_str() {
                    "ANALYZE" | "ANALYSE" => analyze = true,
                    "VERBOSE" => {}
                    _ => break,
                }
                rest = &rest[1..];
            }
            // Sans ANALYZE, rien n'est exécuté ; avec, l'instruction expliquée l'est.
            if !analyze || classify(rest) != StatementKind::Write {
                StatementKind::Read
            } else {
                StatementKind::Write
            }
        }
        _ => StatementKind::Write,
    }
}

pub fn kind(statement: &str, dialect: Dialect) -> StatementKind {
    spans(statement, dialect).first().map_or(StatementKind::Write, |s| classify(&s.tokens))
}

/// Chaque instruction est envoyée seule et validée aussitôt : un BEGIN laisserait une transaction
/// ouverte sur la session, et changerait ce qu'exécute l'instruction suivante. Pour MariaDB, un
/// verrou de tables ou `autocommit` coupé la laisseraient ouverte de la même façon.
fn transaction_control(top: &[&str], dialect: Dialect) -> bool {
    match dialect {
        Dialect::Postgres => matches!(
            top,
            ["BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "ABORT" | "SAVEPOINT" | "RELEASE", ..]
                | ["PREPARE", "TRANSACTION", ..]
        ),
        Dialect::MariaDb => match top {
            ["BEGIN" | "START" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "XA" | "LOCK" | "UNLOCK", ..] => true,
            ["SET", rest @ ..] => rest.contains(&"AUTOCOMMIT"),
            _ => false,
        },
    }
}

/// Ce qu'une connexion en lecture seule refuse avant même d'envoyer : de quoi lever le verrou
/// (`SET`/`RESET` de la lecture seule, changement de rôle, `set_config`), ou exécuter du code côté
/// serveur (`DO`, `CALL`, `COPY … PROGRAM`).
fn lifts_read_only(tokens: &[Token], dialect: Dialect) -> Option<&'static str> {
    if dialect == Dialect::MariaDb {
        return mariadb_lifts_read_only(tokens);
    }
    let all: Vec<&str> = words(tokens).collect();
    let top = top_words(tokens);
    let has = |w: &str| all.contains(&w);
    let pair = |a: &str, b: &str| all.windows(2).any(|p| p[0] == a && p[1] == b);
    match top.first().copied() {
        Some("SET" | "RESET")
            if has("DEFAULT_TRANSACTION_READ_ONLY") || has("TRANSACTION_READ_ONLY") || has("CHARACTERISTICS") =>
        {
            Some("lever la lecture seule")
        }
        Some("SET") if top.get(1) == Some(&"TRANSACTION") && pair("READ", "WRITE") => Some("lever la lecture seule"),
        Some("SET" | "RESET") if has("AUTHORIZATION") || has("ROLE") => Some("changer de rôle"),
        Some("RESET") if top.get(1) == Some(&"ALL") => Some("RESET ALL"),
        Some("DISCARD") if top.get(1) == Some(&"ALL") => Some("DISCARD ALL"),
        Some("DO" | "CALL") => Some("exécuter du code côté serveur"),
        Some("COPY") if has("PROGRAM") => Some("COPY … PROGRAM"),
        _ if has("SET_CONFIG") => Some("set_config"),
        _ => None,
    }
}

/// MariaDB, connexion en lecture seule : aucun `SET`, qui lève le verrou de plus de façons qu'une
/// liste n'en retiendrait (`tx_read_only`, `SET STATEMENT … FOR`, `SET SESSION TRANSACTION`) ou
/// change la lecture du texte (`sql_mode`, jeu de caractères) ; aucun SQL dynamique, dont le texte
/// est une chaîne que l'analyse ne lit pas ; aucun commentaire exécutable.
fn mariadb_lifts_read_only(tokens: &[Token]) -> Option<&'static str> {
    if tokens.contains(&Token::Executable) {
        return Some("commentaire exécutable /*! … */");
    }
    match top_words(tokens).first().copied() {
        Some("SET") => Some("SET (lecture seule, sql_mode ou jeu de caractères de la session)"),
        Some("PREPARE" | "EXECUTE" | "DEALLOCATE") => Some("SQL dynamique (PREPARE, EXECUTE)"),
        Some("CALL") => Some("exécuter du code côté serveur"),
        Some("DO") => Some("DO, qui n'évalue une expression que pour ses effets"),
        Some("BINLOG") => Some("BINLOG"),
        _ => None,
    }
}

/// Fonctions d'administration ou à effet de bord connues, refusées en prod même dans un SELECT.
/// Repérage lexical d'un nom suivi de `(` : une fonction maison qui écrit passe quand même.
const PROD_FUNCTIONS: &[&str] = &[
    "PG_TERMINATE_BACKEND", "PG_CANCEL_BACKEND", "PG_RELOAD_CONF", "PG_ROTATE_LOGFILE", "SET_CONFIG", "NEXTVAL",
    "SETVAL", "PG_READ_FILE", "PG_READ_BINARY_FILE", "PG_SWITCH_WAL", "PG_CREATE_RESTORE_POINT", "PG_PROMOTE",
    "PG_LOGICAL_EMIT_MESSAGE", "PG_DROP_REPLICATION_SLOT", "PG_IMPORT_SYSTEM_COLLATIONS", "PG_FILE_WRITE",
    "PG_FILE_UNLINK", "PG_FILE_RENAME", "TXID_CURRENT", "PG_CURRENT_XACT_ID",
];
const PROD_FUNCTION_PREFIXES: &[&str] =
    &["LO_", "DBLINK", "PG_ADVISORY", "PG_TRY_ADVISORY", "PG_LS_", "PG_STAT_RESET", "PG_CREATE_", "PG_REPLICATION_"];

/// MariaDB : fichiers du serveur, verrous nommés qui survivent à l'instruction, séquences (leur
/// avance n'est pas annulée par un ROLLBACK), attente de réplication, et les UDF d'exécution de
/// commandes les plus répandues.
const MARIADB_PROD_FUNCTIONS: &[&str] = &[
    "LOAD_FILE", "GET_LOCK", "RELEASE_LOCK", "RELEASE_ALL_LOCKS", "BENCHMARK", "NEXTVAL", "SETVAL",
    "MASTER_POS_WAIT", "MASTER_GTID_WAIT", "SOURCE_POS_WAIT", "SYS_EXEC", "SYS_EVAL",
];

fn prod_function(tokens: &[Token], dialect: Dialect) -> Option<String> {
    let refused = |name: &str| match dialect {
        Dialect::Postgres => PROD_FUNCTIONS.contains(&name) || PROD_FUNCTION_PREFIXES.iter().any(|p| name.starts_with(p)),
        Dialect::MariaDb => MARIADB_PROD_FUNCTIONS.contains(&name),
    };
    let call = tokens.windows(2).find_map(|pair| match pair {
        [Token::Word(name), Token::Open] if refused(name) => Some(name.to_lowercase()),
        _ => None,
    });
    // MariaDB fait aussi avancer une séquence sans parenthèses.
    let next_value = || {
        let next = |w: &str| Token::Word(w.to_owned());
        let advances = tokens.windows(3).any(|t| t == [next("NEXT"), next("VALUE"), next("FOR")]);
        (dialect == Dialect::MariaDb && advances).then(|| "next value for".to_owned())
    };
    call.or_else(next_value)
}

/// Dit si une instruction peut partir, et comment : jamais de contrôle de transaction, rien qui
/// lève le verrou d'une connexion en lecture seule, et en prod, seulement des lectures, des
/// INSERT, et des UPDATE ou DELETE restreints par un WHERE.
pub fn admit(statement: &str, language: impl Into<Language>, policy: Policy) -> Result<Admission, DbError> {
    let dialect = match language.into() {
        Language::Sql(dialect) => dialect,
        Language::Elastic => return crate::db_elastic::admit(statement, policy),
    };
    let span = spans(statement, dialect).into_iter().next().ok_or(DbError::NoStatement)?;
    let top = top_words(&span.tokens);
    if transaction_control(&top, dialect) {
        return Err(DbError::TransactionControl);
    }
    if policy != Policy::Writable {
        if let Some(what) = lifts_read_only(&span.tokens, dialect) {
            return Err(DbError::ReadOnlyGuard(what));
        }
    }
    let kind = classify(&span.tokens);
    match policy {
        Policy::ReadOnly => Ok(Admission::Run(kind)),
        Policy::Writable if kind == StatementKind::Write => Ok(Admission::Confirm),
        Policy::Writable => Ok(Admission::Run(kind)),
        Policy::Prod => {
            if let Some(function) = prod_function(&span.tokens, dialect) {
                return Err(DbError::ProdFunction(function));
            }
            match (kind, top.first().copied()) {
                (StatementKind::Query | StatementKind::Read, _) => Ok(Admission::Run(kind)),
                // Un WHERE au niveau principal, pas dans une sous-requête. `WHERE true` passe :
                // c'est la confirmation, avec le nombre de lignes, qui le rattrape.
                // INSERT … SELECT et INSERT … ON CONFLICT DO UPDATE compris : un WITH qui écrit,
                // lui, commence par WITH et reste refusé.
                (StatementKind::Write, Some("INSERT")) => Ok(Admission::ProdWrite),
                (StatementKind::Write, Some("UPDATE" | "DELETE")) if top.contains(&"WHERE") => Ok(Admission::ProdWrite),
                (StatementKind::Write, Some("UPDATE" | "DELETE")) => Err(DbError::ProdWithoutWhere),
                (StatementKind::Write, _) => Err(DbError::ProdRefused),
            }
        }
    }
}

impl AuditEntry {
    /// Une modification de prod faite depuis la console : où, quoi, combien, et son issue
    /// (`commit`, `rollback`, ou ce qui a échoué).
    pub fn on_prod_write(at: Millis, target: &str, statement: &str, affected: Option<u64>, outcome: &str) -> Self {
        let rows = affected.map(|n| format!(" — {n} ligne{}", if n > 1 { "s" } else { "" })).unwrap_or_default();
        Self { at, target: target.to_owned(), action: format!("{statement}{rows}"), outcome: outcome.to_owned() }
    }
}

/// Ce que le serveur a renvoyé : colonnes, lignes en texte (`None` pour NULL), et pour une
/// instruction sans résultat, le nombre de lignes touchées.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Executed {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
    pub affected: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<Cell>>>,
    /// Il y avait plus de `ROWS_MAX` lignes.
    pub truncated: bool,
    pub affected: Option<u64>,
}

pub fn outcome(mut executed: Executed) -> Outcome {
    let truncated = executed.rows.len() > ROWS_MAX;
    executed.rows.truncate(ROWS_MAX);
    Outcome {
        columns: executed.columns,
        rows: executed.rows.into_iter().map(|row| row.into_iter().map(|v| v.map(cell)).collect()).collect(),
        truncated,
        affected: executed.affected,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub sql: String,
    pub at: i64,
}

/// Requêtes envoyées depuis la console, par connexion, sans leurs résultats.
pub trait ConsoleHistory: Send + Sync {
    /// Garde les `HISTORY_MAX` dernières.
    fn record(&self, connection: &str, sql: &str, at: i64) -> Result<(), DbError>;
    /// Les plus récentes d'abord.
    fn recent(&self, connection: &str) -> Result<Vec<HistoryEntry>, DbError>;
    fn forget(&self, connection: &str) -> Result<(), DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;
    const MARIA: Dialect = Dialect::MariaDb;

    fn all(text: &str) -> Vec<&str> {
        spans(text, Dialect::Postgres).iter().map(|s| &text[s.start..s.end]).collect()
    }

    #[test]
    fn splits_on_semicolons_outside_strings_comments_and_dollar_blocks() {
        let text = "SELECT 'a;b', \"x;y\" FROM t; -- c;d\n/* e; /* f; */ g; */ SELECT $$h;i$$, $tag$j;$$k$tag$;\nSELECT E'l\\';m'; SELECT $1";
        assert_eq!(
            all(text),
            vec![
                "SELECT 'a;b', \"x;y\" FROM t",
                "SELECT $$h;i$$, $tag$j;$$k$tag$",
                "SELECT E'l\\';m'",
                "SELECT $1"
            ]
        );
        // Que des commentaires et des `;` : rien à exécuter.
        assert!(all("-- rien ;\n ; /* ; */").is_empty());
    }

    #[test]
    fn picks_the_statement_under_the_cursor() {
        let text = "SELECT 1;\nSELECT 2;\n\n";
        assert_eq!(statement_at(text, PG, 3).unwrap(), "SELECT 1");
        // Juste après le `;` : encore la première.
        assert_eq!(statement_at(text, PG, 9).unwrap(), "SELECT 1");
        assert_eq!(statement_at(text, PG, 10).unwrap(), "SELECT 2");
        // Sur les lignes vides de la fin : la dernière écrite.
        assert_eq!(statement_at(text, PG, text.len()).unwrap(), "SELECT 2");
        assert!(matches!(statement_at("  -- seul\n", PG, 0), Err(DbError::NoStatement)));
    }

    #[test]
    fn a_selection_must_hold_a_single_statement() {
        assert_eq!(single_statement(" SELECT 1 ; ", PG).unwrap(), "SELECT 1");
        assert!(matches!(single_statement("SELECT 1; DELETE FROM t", PG), Err(DbError::MultipleStatements)));
    }

    #[test]
    fn converts_editor_positions_counted_in_utf16() {
        let text = "SELECT '😀é'; SELECT 2";
        // 😀 compte deux unités UTF-16 et quatre octets.
        let after = byte_offset(text, "SELECT '😀é';".encode_utf16().count() + 1);
        assert_eq!(&text[after..], "SELECT 2");
        assert_eq!(byte_offset(text, 10_000), text.len());
    }

    #[test]
    fn classifies_reads_and_writes() {
        use StatementKind::*;
        for (sql, expected) in [
            ("select * from t", Query),
            ("VALUES (1)", Query),
            ("TABLE t", Query),
            ("WITH a AS (SELECT 1) SELECT * FROM a", Query),
            ("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d", Write),
            ("SELECT * INTO copie FROM t", Write),
            ("SELECT (SELECT 1 INTO x)", Query),
            ("SHOW search_path", Read),
            ("EXPLAIN SELECT 1", Read),
            ("EXPLAIN DELETE FROM t", Read),
            ("EXPLAIN ANALYZE SELECT 1", Read),
            ("EXPLAIN ANALYZE DELETE FROM t", Write),
            ("EXPLAIN (ANALYZE, BUFFERS) UPDATE t SET a = 1", Write),
            ("INSERT INTO t VALUES (1)", Write),
            ("/* lecture ? */ delete from t", Write),
            ("SET search_path = public", Write),
        ] {
            assert_eq!(kind(sql, PG), expected, "{sql}");
        }
    }

    #[test]
    fn refuses_what_would_lift_read_only() {
        for sql in [
            "SET default_transaction_read_only = off",
            "set SESSION Default_Transaction_Read_Only to off",
            "SET \"transaction_read_only\" = off",
            "RESET default_transaction_read_only",
            "SET TRANSACTION READ WRITE",
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
            "SET ROLE admin",
            "SET SESSION AUTHORIZATION postgres",
            "RESET ROLE",
            "RESET ALL",
            "DISCARD ALL",
            "DO $$ BEGIN PERFORM 1; END $$",
            "CALL p()",
            "COPY t TO PROGRAM 'cat'",
            "SELECT set_config('default_transaction_read_only', 'off', false)",
            "/* rien */ set -- commentaire\n default_transaction_read_only = off",
        ] {
            assert!(matches!(admit(sql, PG, Policy::ReadOnly), Err(DbError::ReadOnlyGuard(_))), "{sql}");
            // Sur une connexion qui peut écrire, c'est la confirmation qui protège.
            assert!(admit(sql, PG, Policy::Writable).is_ok(), "{sql}");
        }
        assert_eq!(admit("SET search_path = public", PG, Policy::ReadOnly).unwrap(), Admission::Run(StatementKind::Write));
        assert_eq!(admit("SELECT 'SET ROLE admin'", PG, Policy::ReadOnly).unwrap(), Admission::Run(StatementKind::Query));
        assert_eq!(admit("DELETE FROM t", PG, Policy::Writable).unwrap(), Admission::Confirm);
    }

    #[test]
    fn transaction_control_never_runs() {
        for sql in ["BEGIN", "begin read write", "START TRANSACTION READ WRITE", "COMMIT", "ROLLBACK", "SAVEPOINT a", "END"] {
            assert!(matches!(admit(sql, PG, Policy::Writable), Err(DbError::TransactionControl)), "{sql}");
        }
    }

    #[test]
    fn production_reads_inserts_and_updates_or_deletes_with_a_where() {
        use Admission::*;
        for (sql, expected) in [
            ("SELECT * FROM t", Run(StatementKind::Query)),
            ("WITH a AS (SELECT 1) SELECT * FROM a", Run(StatementKind::Query)),
            ("EXPLAIN SELECT 1", Run(StatementKind::Read)),
            ("SHOW search_path", Run(StatementKind::Read)),
            ("VALUES (1)", Run(StatementKind::Query)),
            ("TABLE t", Run(StatementKind::Query)),
            ("UPDATE t SET a = 1 WHERE id = 3", ProdWrite),
            ("delete from t where id in (select id from u)", ProdWrite),
            ("UPDATE t SET a = 1 WHERE id = 3 RETURNING id", ProdWrite),
            ("INSERT INTO t VALUES (1)", ProdWrite),
            ("INSERT INTO t (a) SELECT b FROM u WHERE c", ProdWrite),
            ("INSERT INTO t VALUES (1) ON CONFLICT (id) DO UPDATE SET a = excluded.a", ProdWrite),
        ] {
            assert_eq!(admit(sql, PG, Policy::Prod).unwrap(), expected, "{sql}");
        }
        for sql in ["UPDATE t SET a = 1", "DELETE FROM t", "UPDATE t SET a = (SELECT b FROM u WHERE u.id = 1)", "DELETE FROM t -- WHERE id = 1"] {
            assert!(matches!(admit(sql, PG, Policy::Prod), Err(DbError::ProdWithoutWhere)), "{sql}");
        }
        for sql in [
            "WITH n AS (SELECT 1) INSERT INTO t SELECT * FROM n",
            "MERGE INTO t USING u ON t.id = u.id WHEN MATCHED THEN DELETE",
            "TRUNCATE t",
            "DROP TABLE t",
            "CREATE INDEX i ON t (a)",
            "ALTER TABLE t ADD c int",
            "COPY t FROM '/tmp/x'",
            "GRANT SELECT ON t TO x",
            "VACUUM t",
            "LOCK TABLE t",
            "EXPLAIN ANALYZE UPDATE t SET a = 1 WHERE id = 1",
            "WITH d AS (DELETE FROM t WHERE id = 1 RETURNING *) SELECT * FROM d",
            "SET search_path = public",
            "SELECT * INTO copie FROM t",
        ] {
            assert!(matches!(admit(sql, PG, Policy::Prod), Err(DbError::ProdRefused)), "{sql}");
        }
    }

    #[test]
    fn production_refuses_admin_functions_even_inside_a_select() {
        for sql in [
            "SELECT pg_terminate_backend(42)",
            "select PG_Cancel_Backend (42)",
            "SELECT pg_catalog.pg_reload_conf /* rien */ ()",
            "SELECT nextval('s')",
            "SELECT lo_import('/etc/passwd')",
            "SELECT * FROM dblink('h', 'q') AS x(a int)",
            "SELECT pg_advisory_lock(1)",
            "SELECT pg_read_file('/etc/hosts')",
            "SELECT \"pg_ls_dir\"('.')",
            "UPDATE t SET a = setval('s', 1) WHERE id = 1",
            "INSERT INTO t SELECT pg_terminate_backend(pid) FROM pg_stat_activity",
        ] {
            assert!(matches!(admit(sql, PG, Policy::Prod), Err(DbError::ProdFunction(_))), "{sql}");
        }
        // Le nom dans une chaîne, un commentaire ou sans parenthèse n'est pas un appel.
        for sql in ["SELECT 'pg_terminate_backend(1)'", "SELECT 1 -- nextval(1)", "SELECT nextval FROM t", "SELECT lo FROM t"] {
            assert!(admit(sql, PG, Policy::Prod).is_ok(), "{sql}");
        }
        // Hors prod, la console ne les filtre pas.
        assert!(admit("SELECT pg_terminate_backend(42)", PG, Policy::ReadOnly).is_ok());
    }

    #[test]
    fn splits_mariadb_text_as_the_server_reads_it() {
        let text = "SELECT 'a\\';b', \"c;\"\"d\", `e;f` FROM t; # g;h\nSELECT 1--1; SELECT 2; -- i;\n/* j; /* k; */ SELECT $$";
        let spans: Vec<&str> = spans(text, MARIA).iter().map(|s| &text[s.start..s.end]).collect();
        // `--` collé n'est pas un commentaire ; `/* */` ne s'imbrique pas ; `$$` n'est rien.
        assert_eq!(spans, ["SELECT 'a\\';b', \"c;\"\"d\", `e;f` FROM t", "SELECT 1--1", "SELECT 2", "SELECT $$"]);
        // Un commentaire exécutable est du texte de l'instruction, `;` compris.
        assert_eq!(single_statement("/*! SET a = 1; */ SELECT 1", MARIA).unwrap(), "/*! SET a = 1; */ SELECT 1");
        assert!(matches!(single_statement("/* x; */ SELECT 1; SELECT 2", MARIA), Err(DbError::MultipleStatements)));
        assert_eq!(kind("DESCRIBE t", MARIA), StatementKind::Read);
        assert_eq!(kind("SELECT * FROM t INTO OUTFILE '/tmp/x'", MARIA), StatementKind::Write);
    }

    #[test]
    fn mariadb_refuses_what_would_lift_read_only_or_hide_from_the_analysis() {
        for sql in [
            "SET SESSION TRANSACTION READ WRITE",
            "set tx_read_only = 0",
            "SET @@session.transaction_read_only = 0",
            "SET STATEMENT tx_read_only = 0 FOR INSERT INTO t VALUES (1)",
            "SET sql_mode = 'NO_BACKSLASH_ESCAPES'",
            "SET NAMES gbk",
            "SET ROLE admin",
            "EXECUTE IMMEDIATE 'SET tx_read_only = 0'",
            "PREPARE p FROM 'SET tx_read_only = 0'",
            "EXECUTE p",
            "CALL p()",
            "DO GET_LOCK('a', 1)",
            "/*!50000 SET tx_read_only = 0 */",
            "/*M!100000 SET tx_read_only = 0 */",
            "SELECT 1 /*! , SLEEP(10) */",
        ] {
            assert!(matches!(admit(sql, MARIA, Policy::ReadOnly), Err(DbError::ReadOnlyGuard(_))), "{sql}");
            assert!(matches!(admit(sql, MARIA, Policy::Prod), Err(DbError::ReadOnlyGuard(_))), "{sql}");
            assert!(admit(sql, MARIA, Policy::Writable).is_ok(), "{sql}");
        }
        // Le nom dans une chaîne, un identifiant cité ou un commentaire n'est pas un SET.
        assert_eq!(admit("SELECT 'SET tx_read_only = 0'", MARIA, Policy::ReadOnly).unwrap(), Admission::Run(StatementKind::Query));
        assert_eq!(admit("SELECT `set` FROM t # SET x", MARIA, Policy::ReadOnly).unwrap(), Admission::Run(StatementKind::Query));
    }

    #[test]
    fn mariadb_transaction_control_never_runs() {
        for sql in [
            "START TRANSACTION READ WRITE",
            "BEGIN NOT ATOMIC SELECT 1",
            "COMMIT",
            "XA START 'x'",
            "LOCK TABLES t WRITE",
            "UNLOCK TABLES",
            "SET autocommit = 0",
            "set @@session.autocommit=0",
        ] {
            assert!(matches!(admit(sql, MARIA, Policy::Writable), Err(DbError::TransactionControl)), "{sql}");
        }
        // `LOCK` n'est un contrôle de transaction que pour MariaDB.
        assert!(matches!(admit("LOCK TABLE t", PG, Policy::Writable), Ok(Admission::Confirm)));
    }

    #[test]
    fn mariadb_production_reads_inserts_and_updates_or_deletes_with_a_where() {
        use Admission::*;
        for (sql, expected) in [
            ("SELECT * FROM `t` WHERE a = 'x'", Run(StatementKind::Query)),
            ("SHOW TABLES", Run(StatementKind::Read)),
            ("DESCRIBE t", Run(StatementKind::Read)),
            ("EXPLAIN UPDATE t SET a = 1", Run(StatementKind::Read)),
            ("UPDATE `t` SET a = 1 WHERE id = 3", ProdWrite),
            ("UPDATE t JOIN u ON t.id = u.id SET t.a = u.a WHERE u.b = 1", ProdWrite),
            ("DELETE FROM t WHERE id = 1", ProdWrite),
            ("INSERT INTO t VALUES (1) ON DUPLICATE KEY UPDATE a = 2", ProdWrite),
        ] {
            assert_eq!(admit(sql, MARIA, Policy::Prod).unwrap(), expected, "{sql}");
        }
        for sql in ["UPDATE t SET a = 1 ORDER BY id LIMIT 10", "DELETE FROM t # WHERE id = 1", "DELETE FROM t -- WHERE id = 1"] {
            assert!(matches!(admit(sql, MARIA, Policy::Prod), Err(DbError::ProdWithoutWhere)), "{sql}");
        }
        for sql in [
            "REPLACE INTO t VALUES (1)",
            "TRUNCATE t",
            "LOAD DATA INFILE '/tmp/x' INTO TABLE t",
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
            "ANALYZE TABLE t",
            "OPTIMIZE TABLE t",
            "KILL 42",
            "HANDLER t OPEN",
            "USE autre",
        ] {
            assert!(matches!(admit(sql, MARIA, Policy::Prod), Err(DbError::ProdRefused)), "{sql}");
        }
    }

    #[test]
    fn mariadb_production_refuses_admin_functions_even_inside_a_select() {
        for sql in [
            "SELECT LOAD_FILE('/etc/passwd')",
            "SELECT get_lock('a', 10)",
            "select benchmark(1e9, md5('a'))",
            "SELECT NEXT VALUE FOR s",
            "INSERT INTO t VALUES (NEXTVAL(s))",
            "SELECT `sys_exec`('id')",
            "UPDATE t SET a = RELEASE_ALL_LOCKS() WHERE id = 1",
        ] {
            assert!(matches!(admit(sql, MARIA, Policy::Prod), Err(DbError::ProdFunction(_))), "{sql}");
        }
        for sql in ["SELECT 'load_file(1)'", "SELECT 1 # get_lock(1)", "SELECT next_value FROM t", "SELECT pg_terminate_backend(1)"] {
            assert!(admit(sql, MARIA, Policy::Prod).is_ok(), "{sql}");
        }
    }

    #[test]
    fn audits_a_production_write() {
        let entry = AuditEntry::on_prod_write(7, "Boutique prod · app@h:5432/boutique", "DELETE FROM t WHERE id = 1", Some(1), "commit");
        assert_eq!(entry.action, "DELETE FROM t WHERE id = 1 — 1 ligne");
        assert_eq!((entry.target.as_str(), entry.outcome.as_str()), ("Boutique prod · app@h:5432/boutique", "commit"));
    }

    #[test]
    fn caps_the_rows_shown() {
        let executed = Executed {
            columns: vec![],
            rows: (0..=ROWS_MAX).map(|i| vec![Some(i.to_string()), None]).collect(),
            affected: None,
        };
        let shown = outcome(executed);
        assert!(shown.truncated);
        assert_eq!(shown.rows.len(), ROWS_MAX);
        assert_eq!(shown.rows[0][1], None);
    }
}

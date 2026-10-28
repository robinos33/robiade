//! Console : une instruction SQL écrite à la main, exécutée seule.
//!
//! Ce module découpe le texte de l'éditeur en instructions, choisit celle sous le curseur, la
//! classe (lecture ou écriture) et refuse sur une connexion en lecture seule celles qui pourraient
//! lever ce verrou. Ce refus est un garde-fou contre la fausse manipulation, pas une protection :
//! le vrai verrou est le serveur (`default_transaction_read_only`, posé à l'ouverture), et une
//! analyse lexicale ne connaît ni les fonctions ni les droits du compte.

use crate::databases::DbError;
use crate::db_browse::{cell, Cell, Column};

/// Lignes affichées au plus ; une de plus est lue pour savoir que le résultat est tronqué.
pub const ROWS_MAX: usize = 1000;
/// Requêtes gardées dans l'historique d'une connexion.
pub const HISTORY_MAX: usize = 200;

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

/// Découpe sur `;`, en ignorant chaînes ('…', E'…'), identifiants entre guillemets, commentaires
/// (`--`, `/* */` imbriqués) et blocs `$tag$ … $tag$`. Les segments vides sont écartés.
fn spans(text: &str) -> Vec<Span> {
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
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
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
            b'$' => {
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
pub fn statement_at(text: &str, cursor: usize) -> Result<&str, DbError> {
    let spans = spans(text);
    let found = spans
        .iter()
        .find(|s| s.start <= cursor && cursor <= s.reach)
        .or_else(|| spans.iter().rev().find(|s| s.reach <= cursor))
        .or_else(|| spans.first())
        .ok_or(DbError::NoStatement)?;
    Ok(&text[found.start..found.end])
}

/// Le texte entier doit tenir en une instruction : c'est le cas d'une sélection.
pub fn single_statement(text: &str) -> Result<&str, DbError> {
    match spans(text).as_slice() {
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
        Some("SHOW") => StatementKind::Read,
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

pub fn kind(statement: &str) -> StatementKind {
    spans(statement).first().map_or(StatementKind::Write, |s| classify(&s.tokens))
}

/// Chaque instruction est envoyée seule et validée aussitôt : un BEGIN laisserait une transaction
/// ouverte sur la session, et changerait ce qu'exécute l'instruction suivante.
fn transaction_control(top: &[&str]) -> bool {
    matches!(
        top,
        ["BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "ABORT" | "SAVEPOINT" | "RELEASE", ..]
            | ["PREPARE", "TRANSACTION", ..]
    )
}

/// Ce qu'une connexion en lecture seule refuse avant même d'envoyer : de quoi lever le verrou
/// (`SET`/`RESET` de la lecture seule, changement de rôle, `set_config`), ou exécuter du code côté
/// serveur (`DO`, `CALL`, `COPY … PROGRAM`). Le serveur reste le verrou : ceci évite seulement
/// qu'une fausse manipulation le teste.
fn lifts_read_only(tokens: &[Token]) -> Option<&'static str> {
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

/// Vérifie qu'une instruction peut partir : jamais de contrôle de transaction, et rien qui lève
/// le verrou sur une connexion en lecture seule.
pub fn check(statement: &str, read_only: bool) -> Result<StatementKind, DbError> {
    let span = spans(statement).into_iter().next().ok_or(DbError::NoStatement)?;
    if transaction_control(&top_words(&span.tokens)) {
        return Err(DbError::TransactionControl);
    }
    if read_only {
        if let Some(what) = lifts_read_only(&span.tokens) {
            return Err(DbError::ReadOnlyGuard(what));
        }
    }
    Ok(classify(&span.tokens))
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

    fn all(text: &str) -> Vec<&str> {
        spans(text).iter().map(|s| &text[s.start..s.end]).collect()
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
        assert_eq!(statement_at(text, 3).unwrap(), "SELECT 1");
        // Juste après le `;` : encore la première.
        assert_eq!(statement_at(text, 9).unwrap(), "SELECT 1");
        assert_eq!(statement_at(text, 10).unwrap(), "SELECT 2");
        // Sur les lignes vides de la fin : la dernière écrite.
        assert_eq!(statement_at(text, text.len()).unwrap(), "SELECT 2");
        assert!(matches!(statement_at("  -- seul\n", 0), Err(DbError::NoStatement)));
    }

    #[test]
    fn a_selection_must_hold_a_single_statement() {
        assert_eq!(single_statement(" SELECT 1 ; ").unwrap(), "SELECT 1");
        assert!(matches!(single_statement("SELECT 1; DELETE FROM t"), Err(DbError::MultipleStatements)));
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
            assert_eq!(kind(sql), expected, "{sql}");
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
            assert!(matches!(check(sql, true), Err(DbError::ReadOnlyGuard(_))), "{sql}");
            // Sur une connexion qui peut écrire, c'est la confirmation qui protège.
            assert!(check(sql, false).is_ok(), "{sql}");
        }
        assert_eq!(check("SET search_path = public", true).unwrap(), StatementKind::Write);
        assert_eq!(check("SELECT 'SET ROLE admin'", true).unwrap(), StatementKind::Query);
    }

    #[test]
    fn transaction_control_never_runs() {
        for sql in ["BEGIN", "begin read write", "START TRANSACTION READ WRITE", "COMMIT", "ROLLBACK", "SAVEPOINT a", "END"] {
            assert!(matches!(check(sql, false), Err(DbError::TransactionControl)), "{sql}");
        }
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

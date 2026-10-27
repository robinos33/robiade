//! Recherche plein texte dans l'historique : index FTS5 dans `cache.db`.
//!
//! L'index est un cache : il se reconstruit entièrement à partir des transcripts, et le supprimer
//! ne fait rien perdre. Seuls les transcripts modifiés depuis la dernière passe sont relus.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use rayon::prelude::*;
use robiade_core::history::{HistoryError, SearchHit, SessionSearch, HIT_END, HIT_START};
use rusqlite::{params, Connection};

use crate::claude_code::read_entries;

pub struct SearchIndex {
    conn: Mutex<Connection>,
}

fn unreadable(e: impl std::fmt::Display) -> HistoryError {
    HistoryError::Unreadable(e.to_string())
}

/// Taille et date de modification : un transcript ne fait que grossir, cela suffit à voir qu'il a changé.
fn fingerprint(path: &Path) -> Option<(i64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    Some((meta.len() as i64, modified))
}

/// Chaque mot devient une chaîne FTS5 littérale : la saisie ne peut pas injecter d'opérateur.
/// Le dernier mot est cherché en préfixe, pour trouver pendant la frappe.
fn fts_query(query: &str) -> Option<String> {
    let words: Vec<String> = query.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', "\"\""))).collect();
    let last = words.last()?.clone();
    Some(format!("{} {last}*", words[..words.len() - 1].join(" ")).trim_start().to_owned())
}

impl SearchIndex {
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        let conn = Connection::open(path).map_err(unreadable)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS indexed (
                 path TEXT PRIMARY KEY,
                 len INTEGER NOT NULL,
                 modified INTEGER NOT NULL
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS entry USING fts5(
                 session_id UNINDEXED,
                 path UNINDEXED,
                 text,
                 tokenize = 'unicode61 remove_diacritics 2'
             );",
        )
        .map_err(unreadable)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Met l'index à jour : relit les transcripts modifiés, oublie ceux qui ont disparu.
    /// Renvoie le nombre de transcripts relus.
    pub fn sync(&self, files: &[PathBuf]) -> Result<usize, HistoryError> {
        let known: HashMap<String, (i64, i64)> = {
            let conn = self.conn.lock().map_err(unreadable)?;
            let mut stmt = conn.prepare("SELECT path, len, modified FROM indexed").map_err(unreadable)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))
                .map_err(unreadable)?;
            rows.collect::<Result<_, _>>().map_err(unreadable)?
        };

        // Lecture en parallèle, hors verrou ; écriture ensuite, en une transaction.
        let changed: Vec<(String, String, (i64, i64), Vec<String>)> = files
            .par_iter()
            .filter_map(|path| {
                let print = fingerprint(path)?;
                let key = path.to_string_lossy().into_owned();
                if known.get(&key) == Some(&print) {
                    return None;
                }
                let session_id = path.file_stem()?.to_string_lossy().into_owned();
                let texts = read_entries(path).ok()?.into_iter().map(|e| e.text).collect();
                Some((key, session_id, print, texts))
            })
            .collect();
        let present: std::collections::HashSet<String> =
            files.iter().map(|p| p.to_string_lossy().into_owned()).collect();

        let mut conn = self.conn.lock().map_err(unreadable)?;
        let tx = conn.transaction().map_err(unreadable)?;
        for gone in known.keys().filter(|k| !present.contains(*k)) {
            tx.execute("DELETE FROM entry WHERE path = ?1", [gone]).map_err(unreadable)?;
            tx.execute("DELETE FROM indexed WHERE path = ?1", [gone]).map_err(unreadable)?;
        }
        for (path, session_id, (len, modified), texts) in &changed {
            tx.execute("DELETE FROM entry WHERE path = ?1", [path]).map_err(unreadable)?;
            let mut insert =
                tx.prepare_cached("INSERT INTO entry (session_id, path, text) VALUES (?1, ?2, ?3)").map_err(unreadable)?;
            for text in texts {
                insert.execute(params![session_id, path, text]).map_err(unreadable)?;
            }
            tx.execute(
                "INSERT INTO indexed (path, len, modified) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET len = ?2, modified = ?3",
                params![path, len, modified],
            )
            .map_err(unreadable)?;
        }
        tx.commit().map_err(unreadable)?;
        Ok(changed.len())
    }
}

impl SessionSearch for SearchIndex {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, HistoryError> {
        let Some(fts) = fts_query(query) else { return Ok(vec![]) };
        let conn = self.conn.lock().map_err(unreadable)?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT session_id, snippet(entry, 2, ?2, ?3, '…', 14) FROM entry
                 WHERE entry MATCH ?1 ORDER BY bm25(entry) LIMIT 2000",
            )
            .map_err(unreadable)?;
        let rows = stmt
            .query_map(params![fts, HIT_START.to_string(), HIT_END.to_string()], |r| {
                Ok(SearchHit { session_id: r.get(0)?, snippet: r.get(1)? })
            })
            .map_err(unreadable)?;
        // Une session = un résultat : on garde son extrait le plus pertinent.
        let mut seen = std::collections::HashSet::new();
        let mut out = vec![];
        for hit in rows {
            let hit = hit.map_err(unreadable)?;
            if seen.insert(hit.session_id.clone()) {
                out.push(hit);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn quotes_every_word_and_prefixes_the_last() {
        assert_eq!(fts_query("  rebase train ").as_deref(), Some(r#""rebase" "train"*"#));
        assert_eq!(fts_query(r#"a"b OR"#).as_deref(), Some(r#""a""b" "OR"*"#));
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn finds_sessions_ignoring_accents_and_forgets_deleted_transcripts() {
        let dir = std::env::temp_dir().join(format!("robiade-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("s1.jsonl");
        let mut f = std::fs::File::create(&transcript).unwrap();
        writeln!(f, r#"{{"type":"user","timestamp":"2026-10-27T10:00:00Z","message":{{"content":"Corrige l'équipe de boutique"}}}}"#).unwrap();
        drop(f);

        let index = SearchIndex::open(&dir.join("cache.db")).unwrap();
        assert_eq!(index.sync(std::slice::from_ref(&transcript)).unwrap(), 1);
        assert_eq!(index.sync(std::slice::from_ref(&transcript)).unwrap(), 0, "rien n'a changé");

        let hits = index.search("equipe bout", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s1");
        assert!(hits[0].snippet.contains(&format!("{HIT_START}équipe{HIT_END}")));

        index.sync(&[]).unwrap();
        assert!(index.search("equipe", 10).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

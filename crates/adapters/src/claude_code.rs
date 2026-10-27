//! Lecture seule des transcripts `~/.claude/projects/<cwd encodé>/<session>.jsonl`.
//!
//! Format interne et non documenté de Claude Code : le parsing est tolérant (champ absent ou
//! inconnu = ignoré, ligne illisible = sautée) et ne doit jamais faire échouer la liste entière.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use rayon::prelude::*;
use robiade_core::history::{
    EntryKind, HistoryError, SessionSummary, TokenUsage, TranscriptEntry, TranscriptReader,
};
use robiade_core::Millis;
use serde_json::Value;

const TITLE_MAX_CHARS: usize = 140;

pub struct TranscriptStore {
    root: PathBuf,
    /// Résumés déjà calculés, invalidés par la taille et la date de modification du fichier.
    cache: Mutex<HashMap<PathBuf, (u64, SystemTime, SessionSummary)>>,
}

impl TranscriptStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root, cache: Mutex::new(HashMap::new()) }
    }

    fn transcript_files(&self) -> Vec<PathBuf> {
        let Ok(projects) = std::fs::read_dir(&self.root) else { return vec![] };
        projects
            .flatten()
            .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
            .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect()
    }

    fn find(&self, session_id: &str) -> Option<PathBuf> {
        let file_name = format!("{session_id}.jsonl");
        self.transcript_files().into_iter().find(|p| p.file_name().is_some_and(|n| n == file_name.as_str()))
    }
}

fn timestamp(v: &Value) -> Option<Millis> {
    let raw = v.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw).ok().map(|t| t.timestamp_millis())
}

fn usage(v: &Value) -> TokenUsage {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    TokenUsage {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_creation: n("cache_creation_input_tokens"),
    }
}

/// Texte saisi par l'humain : exclut les résultats d'outils et les messages injectés (`<...>`).
fn user_text(message: &Value) -> Option<String> {
    let text = match message.get("content")? {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<')).then(|| text.to_owned())
}

fn truncate(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or_default();
    match line.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_owned(),
    }
}

fn summarize(path: &Path) -> Option<SessionSummary> {
    let reader = BufReader::new(File::open(path).ok()?);
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    let mut s = SessionSummary {
        session_id,
        project_path: String::new(),
        title: String::new(),
        git_branch: None,
        started_at: Millis::MAX,
        last_activity_at: 0,
        user_messages: 0,
        assistant_messages: 0,
        tokens: TokenUsage::default(),
        tools: vec![],
        models: vec![],
        merge_request_url: None,
    };
    let mut custom_title = None;
    let mut first_prompt = None;
    let mut seen_messages = HashSet::new();
    let mut tools: HashMap<String, u32> = HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(at) = timestamp(&v) {
            s.started_at = s.started_at.min(at);
            s.last_activity_at = s.last_activity_at.max(at);
        }
        if s.project_path.is_empty() {
            if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
                s.project_path = cwd.to_owned();
            }
        }
        if let Some(branch) = v.get("gitBranch").and_then(Value::as_str).filter(|b| !b.is_empty()) {
            s.git_branch = Some(branch.to_owned());
        }
        let sidechain = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
        match v.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                custom_title = v.get("customTitle").and_then(Value::as_str).map(str::to_owned);
            }
            Some("pr-link") => {
                s.merge_request_url = v.get("prUrl").and_then(Value::as_str).map(str::to_owned);
            }
            Some("user") if !sidechain => {
                if let Some(text) = v.get("message").and_then(user_text) {
                    s.user_messages += 1;
                    first_prompt.get_or_insert(text);
                }
            }
            Some("assistant") => {
                let Some(message) = v.get("message") else { continue };
                // Une réponse est écrite en plusieurs lignes portant le même id et le même usage.
                // On ne compte l'usage qu'une fois, mais on lit les blocs de chaque ligne.
                let id = message.get("id").and_then(Value::as_str).unwrap_or_default();
                if id.is_empty() || seen_messages.insert(id.to_owned()) {
                    s.assistant_messages += 1;
                    if let Some(u) = message.get("usage") {
                        s.tokens.add(&usage(u));
                    }
                }
                if let Some(model) = message.get("model").and_then(Value::as_str) {
                    if !model.starts_with('<') && !s.models.iter().any(|m| m == model) {
                        s.models.push(model.to_owned());
                    }
                }
                for block in message.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let Some(name) = block.get("name").and_then(Value::as_str) {
                            *tools.entry(name.to_owned()).or_default() += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if s.last_activity_at == 0 {
        return None;
    }
    s.title = truncate(
        custom_title.or(first_prompt).as_deref().unwrap_or("(session sans message)"),
        TITLE_MAX_CHARS,
    );
    s.tools = tools.into_iter().collect();
    s.tools.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Some(s)
}

impl TranscriptReader for TranscriptStore {
    fn summaries(&self) -> Result<Vec<SessionSummary>, HistoryError> {
        let files = self.transcript_files();
        let cached = self.cache.lock().map_err(|e| HistoryError::Unreadable(e.to_string()))?.clone();
        let fresh: Vec<(PathBuf, u64, SystemTime, SessionSummary)> = files
            .par_iter()
            .filter_map(|path| {
                let meta = std::fs::metadata(path).ok()?;
                let (len, modified) = (meta.len(), meta.modified().ok()?);
                if let Some((l, m, s)) = cached.get(path) {
                    if *l == len && *m == modified {
                        return Some((path.clone(), len, modified, s.clone()));
                    }
                }
                Some((path.clone(), len, modified, summarize(path)?))
            })
            .collect();

        let mut cache = self.cache.lock().map_err(|e| HistoryError::Unreadable(e.to_string()))?;
        cache.clear();
        let mut out = Vec::with_capacity(fresh.len());
        for (path, len, modified, summary) in fresh {
            out.push(summary.clone());
            cache.insert(path, (len, modified, summary));
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.last_activity_at));
        Ok(out)
    }

    fn entries(&self, session_id: &str) -> Result<Vec<TranscriptEntry>, HistoryError> {
        let path = self.find(session_id).ok_or(HistoryError::NotFound)?;
        let file = File::open(&path).map_err(|e| HistoryError::Unreadable(e.to_string()))?;
        let mut out = vec![];
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let at = timestamp(&v).unwrap_or(0);
            let Some(message) = v.get("message") else { continue };
            match v.get("type").and_then(Value::as_str) {
                Some("user") => {
                    if let Some(text) = user_text(message) {
                        out.push(TranscriptEntry { kind: EntryKind::User, at, text });
                    }
                }
                Some("assistant") => {
                    for block in message.get("content").and_then(Value::as_array).into_iter().flatten() {
                        match block.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
                                if !text.trim().is_empty() {
                                    out.push(TranscriptEntry { kind: EntryKind::Assistant, at, text: text.to_owned() });
                                }
                            }
                            Some("tool_use") => {
                                let name = block.get("name").and_then(Value::as_str).unwrap_or("?");
                                let hint = tool_hint(block.get("input"));
                                out.push(TranscriptEntry { kind: EntryKind::ToolUse, at, text: format!("{name} {hint}") });
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

/// Résumé d'une ligne de l'entrée d'un outil, pour le fil de lecture.
fn tool_hint(input: Option<&Value>) -> String {
    let Some(input) = input else { return String::new() };
    ["description", "command", "file_path", "pattern", "url", "prompt"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .map(|s| truncate(s, 120))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn summarizes_a_transcript_and_survives_garbage() {
        let dir = std::env::temp_dir().join(format!("robiade-test-{}", std::process::id()));
        let project = dir.join("-tmp-p");
        std::fs::create_dir_all(&project).unwrap();
        let mut f = File::create(project.join("abc.jsonl")).unwrap();
        let lines = [
            r#"{"type":"user","cwd":"/tmp/p","gitBranch":"main","timestamp":"2026-10-27T10:00:00Z","message":{"role":"user","content":"Corrige le bug du login\nAvec détails"}}"#,
            "pas du json",
            r#"{"type":"assistant","timestamp":"2026-10-27T10:00:05Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":5},"content":[{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-27T10:00:06Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5},"content":[{"type":"text","text":"Fait."}]}}"#,
            r#"{"type":"user","timestamp":"2026-10-27T10:00:07Z","message":{"content":[{"type":"tool_result","content":"x"}]}}"#,
            r#"{"type":"pr-link","prUrl":"https://gitlab.example.com/a/b/-/merge_requests/1","timestamp":"2026-10-27T10:01:00Z"}"#,
        ];
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        let store = TranscriptStore::new(dir.clone());
        let all = store.summaries().unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!(s.title, "Corrige le bug du login");
        assert_eq!(s.project_path, "/tmp/p");
        assert_eq!(s.user_messages, 1);
        assert_eq!(s.assistant_messages, 1);
        assert_eq!(s.tokens.output, 5);
        assert_eq!(s.tools, vec![("Bash".to_owned(), 1)]);
        assert!(s.merge_request_url.is_some());
        let entries = store.entries("abc").unwrap();
        assert_eq!(entries.iter().filter(|e| e.kind == EntryKind::ToolUse).count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

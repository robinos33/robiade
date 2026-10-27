//! Historique des sessions : lecture seule des transcripts Claude Code.
//!
//! Le port n'expose aucune écriture : l'historique ne peut pas être modifié depuis Robiade.

use crate::Millis;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_creation += other.cache_creation;
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub project_path: String,
    pub title: String,
    pub git_branch: Option<String>,
    pub started_at: Millis,
    pub last_activity_at: Millis,
    pub user_messages: u32,
    pub assistant_messages: u32,
    pub tokens: TokenUsage,
    /// Appels d'outils par nom, triés par nombre décroissant.
    pub tools: Vec<(String, u32)>,
    pub models: Vec<String>,
    pub merge_request_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    User,
    Assistant,
    ToolUse,
}

#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub kind: EntryKind,
    pub at: Millis,
    /// Texte brut, non fiable : à rendre comme texte, jamais comme HTML.
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("session introuvable")]
    NotFound,
    #[error("lecture de l'historique impossible : {0}")]
    Unreadable(String),
}

pub trait TranscriptReader: Send + Sync {
    fn summaries(&self) -> Result<Vec<SessionSummary>, HistoryError>;
    fn entries(&self, session_id: &str) -> Result<Vec<TranscriptEntry>, HistoryError>;
}

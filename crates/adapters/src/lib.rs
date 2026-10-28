//! Implémentations des ports de `robiade-core` : fichiers Claude Code, SQLite, hooks, PTY, Git, GitLab, Docker.

pub mod backup;
pub mod claude_code;
pub mod docker;
pub mod git;
pub mod gitlab;
pub mod hooks;
pub mod keychain;
pub mod pty;
pub mod shell_env;
pub mod sqlite_audit;
pub mod sqlite_connections;
pub mod sqlite_agents;
pub mod sqlite_search;
pub mod sqlite_tasks;
pub mod workspace;

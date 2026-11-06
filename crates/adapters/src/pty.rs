//! Agents Claude Code dans un pseudo-terminal.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub struct LaunchSpec<'a> {
    pub claude: &'a str,
    pub cwd: &'a Path,
    pub path_env: &'a str,
    pub args: Vec<String>,
    pub env: Vec<(&'static str, String)>,
    pub cols: u16,
    pub rows: u16,
}

pub struct AgentProcess {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

impl AgentProcess {
    /// Lance le binaire directement, sans shell : aucun argument n'est interprété.
    pub fn spawn(
        spec: LaunchSpec,
        on_output: impl Fn(Vec<u8>) + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: spec.rows, cols: spec.cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(spec.claude);
        // Un Robiade lancé depuis Claude Code ferait de l'agent un sous-agent sans transcript (robiade#16).
        // Lancé depuis iTerm, il ferait croire à Claude Code qu'il tourne dans iTerm.
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|n| is_leaked_claude_var(n) || is_parent_terminal_var(n)) {
                cmd.env_remove(&name);
            }
        }
        cmd.args(&spec.args);
        cmd.cwd(spec.cwd);
        cmd.env("PATH", spec.path_env);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // Sans OSC 8, Claude Code affiche les URL en clair : xterm les reconnaît, et ⌘ décide seul
        // de leur soulignement (robiade#11). Sinon tout dépendrait de l'environnement hérité.
        cmd.env("FORCE_HYPERLINK", "0");
        // Lancé depuis le Dock, Robiade n'hérite d'aucune locale : `pbcopy`, par lequel Claude Code
        // copie, ne lirait pas l'UTF-8 et casserait les accents (robiade#48).
        if lacks_locale(|name| std::env::var_os(name)) {
            cmd.env("LC_CTYPE", "UTF-8");
        }
        for (k, v) in spec.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let child = Arc::new(Mutex::new(child));

        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_output(buf[..n].to_vec()),
                }
            }
        });
        let waiter = child.clone();
        std::thread::spawn(move || {
            loop {
                let exited = waiter.lock().map(|mut c| matches!(c.try_wait(), Ok(Some(_)) | Err(_))).unwrap_or(true);
                if exited {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            on_exit();
        });

        Ok(Self { master: Mutex::new(pair.master), writer: Mutex::new(writer), child })
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut w = self.writer.lock().map_err(|e| e.to_string())?;
        w.write_all(data).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self.master.lock().map_err(|e| e.to_string())?;
        master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
    }

    pub fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

/// Variable héritée de Claude Code, à ne pas transmettre à un agent.
fn is_leaked_claude_var(name: &str) -> bool {
    name == "CLAUDECODE" || name.starts_with("CLAUDE_")
}

/// Variable qui décrit le terminal d'où Robiade a été lancé : le PTY d'un agent n'en est pas un.
/// Celles d'iTerm désignent sa session : le hook global `cc-status` d'iTerm y notifierait
/// l'agent, en double de la notification de Robiade (robiade#9).
fn is_parent_terminal_var(name: &str) -> bool {
    matches!(name, "TERM_PROGRAM" | "TERM_PROGRAM_VERSION" | "LC_TERMINAL" | "LC_TERMINAL_VERSION")
        || name.starts_with("ITERM_")
        || name.starts_with("IT2_")
}

/// Aucune locale définie : l'encodage des outils retombe sur ASCII. Une locale héritée, même autre
/// qu'UTF-8, est un choix de l'environnement : on la laisse.
fn lacks_locale(var: impl Fn(&str) -> Option<OsString>) -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"].into_iter().all(|name| var(name).is_none_or(|v| v.is_empty()))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{is_leaked_claude_var, is_parent_terminal_var, lacks_locale};

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.into())
    }

    #[test]
    fn gives_a_utf8_locale_only_when_none_is_inherited() {
        assert!(lacks_locale(env(&[])));
        assert!(lacks_locale(env(&[("LANG", ""), ("PATH", "/usr/bin")])));
        assert!(!lacks_locale(env(&[("LANG", "fr_FR.UTF-8")])));
        assert!(!lacks_locale(env(&[("LC_CTYPE", "UTF-8")])));
        assert!(!lacks_locale(env(&[("LC_ALL", "C")])));
    }

    #[test]
    fn strips_inherited_claude_code_variables_and_keeps_the_others() {
        assert!(is_leaked_claude_var("CLAUDECODE"));
        assert!(is_leaked_claude_var("CLAUDE_CODE_CHILD_SESSION"));
        assert!(is_leaked_claude_var("CLAUDE_CONFIG_DIR"));
        assert!(!is_leaked_claude_var("ROBIADE_AGENT_ID"));
        assert!(!is_leaked_claude_var("PATH"));
        assert!(!is_leaked_claude_var("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn strips_the_identity_of_the_terminal_robiade_was_launched_from() {
        assert!(is_parent_terminal_var("TERM_PROGRAM"));
        assert!(is_parent_terminal_var("LC_TERMINAL"));
        assert!(is_parent_terminal_var("ITERM_SESSION_ID"));
        assert!(is_parent_terminal_var("ITERM_PROFILE"));
        assert!(is_parent_terminal_var("IT2_SOCKET"));
        assert!(!is_parent_terminal_var("TERM"));
        assert!(!is_parent_terminal_var("COLORTERM"));
    }
}

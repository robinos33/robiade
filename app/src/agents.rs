//! Registre des agents vivants : relie le domaine (`Agent`), les PTY et les événements vers l'UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use robiade_adapters::hooks;
use robiade_adapters::pty::{AgentProcess, LaunchSpec};
use robiade_adapters::shell_env::LoginEnv;
use robiade_core::agents::{Agent, AgentEvent, AgentId, PermissionMode};
use tauri::{AppHandle, Emitter};

use crate::dto::{AgentDto, AgentOutput};
use crate::now;

struct Live {
    agent: Agent,
    process: Option<Arc<AgentProcess>>,
}

pub struct AgentRegistry {
    app: AppHandle,
    login: LoginEnv,
    socket: PathBuf,
    live: Mutex<HashMap<AgentId, Live>>,
}

/// Échappement pour la commande de hook, exécutée par Claude Code via un shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl AgentRegistry {
    pub fn new(app: AppHandle, login: LoginEnv, socket: PathBuf) -> Self {
        Self { app, login, socket, live: Mutex::new(HashMap::new()) }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn claude_path(&self) -> Option<&str> {
        self.login.claude.as_deref()
    }

    pub fn spawn(self: &Arc<Self>, cwd: &Path, resume: Option<AgentId>, cols: u16, rows: u16) -> Result<AgentDto, String> {
        let claude = self.login.claude.as_deref().ok_or("binaire `claude` introuvable dans le PATH du shell de login")?;
        let id = AgentId::parse(&uuid::Uuid::new_v4().to_string()).map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let relay = format!("{} hook", shell_quote(&exe.to_string_lossy()));

        let mut args = match &resume {
            Some(session) => vec!["--resume".to_owned(), session.as_str().to_owned()],
            None => vec!["--session-id".to_owned(), id.as_str().to_owned()],
        };
        args.extend(["--settings".to_owned(), hooks::settings_json(&relay)]);

        let output_target = self.clone();
        let output_id = id.clone();
        let exit_target = self.clone();
        let exit_id = id.clone();
        let process = AgentProcess::spawn(
            LaunchSpec {
                claude,
                cwd,
                path_env: &self.login.path,
                args,
                env: vec![
                    (hooks::ENV_SOCKET, self.socket.to_string_lossy().into_owned()),
                    (hooks::ENV_AGENT_ID, id.as_str().to_owned()),
                ],
                cols,
                rows,
            },
            move |bytes| {
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                let _ = output_target.app.emit("agent:output", AgentOutput { id: output_id.as_str().to_owned(), data });
            },
            move || exit_target.apply(&exit_id, AgentEvent::ProcessExited),
        )?;

        let agent = Agent::start(id.clone(), cwd.to_string_lossy().into_owned(), resume.map(|r| r.as_str().to_owned()), now());
        let dto = AgentDto::from_agent(&agent, now());
        self.live.lock().map_err(|e| e.to_string())?.insert(id, Live { agent, process: Some(Arc::new(process)) });
        self.publish();
        Ok(dto)
    }

    pub fn apply(&self, id: &AgentId, event: AgentEvent) {
        self.observe(id, event, None);
    }

    pub fn observe(&self, id: &AgentId, event: AgentEvent, mode: Option<PermissionMode>) {
        let ended = matches!(event, AgentEvent::ProcessExited);
        {
            let Ok(mut live) = self.live.lock() else { return };
            // Un événement d'un agent inconnu (session hors Robiade, agent déjà retiré) est ignoré.
            let Some(entry) = live.get_mut(id) else { return };
            if let Some(mode) = mode {
                entry.agent.observe_permission_mode(mode);
            }
            entry.agent.apply(event, now());
            if ended {
                entry.process = None;
            }
        }
        self.publish();
    }

    pub fn mark_seen(&self, id: &AgentId) {
        if let Ok(mut live) = self.live.lock() {
            if let Some(entry) = live.get_mut(id) {
                entry.agent.mark_seen(now());
            }
        }
        self.publish();
    }

    pub fn process(&self, id: &AgentId) -> Result<Arc<AgentProcess>, String> {
        let live = self.live.lock().map_err(|e| e.to_string())?;
        live.get(id).and_then(|l| l.process.clone()).ok_or_else(|| "agent inconnu ou terminé".to_owned())
    }

    pub fn list(&self) -> Vec<AgentDto> {
        let at = now();
        let Ok(live) = self.live.lock() else { return vec![] };
        let mut out: Vec<AgentDto> = live.values().map(|l| AgentDto::from_agent(&l.agent, at)).collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.started_at));
        out
    }

    pub fn kill_all(&self) {
        if let Ok(live) = self.live.lock() {
            live.values().filter_map(|l| l.process.as_ref()).for_each(|p| p.kill());
        }
    }

    /// Diffuse la liste complète : quelques agents, pas besoin de deltas.
    fn publish(&self) {
        let agents = self.list();
        let attention = agents.iter().filter(|a| a.needs_attention).count();
        let _ = self.app.emit("agents:changed", &agents);
        if let Some(window) = tauri::Manager::get_webview_window(&self.app, "main") {
            let _ = window.set_badge_count((attention > 0).then_some(attention as i64));
        }
    }
}

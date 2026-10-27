//! Registre des agents vivants : relie le domaine (`Agent`), les PTY et les événements vers l'UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use robiade_adapters::hooks::{self, Observation};
use robiade_adapters::pty::{AgentProcess, LaunchSpec};
use robiade_adapters::shell_env::LoginEnv;
use robiade_adapters::workspace::WorkspaceResolver;
use robiade_core::agents::{Agent, AgentEvent, AgentId, AgentJournal, AgentStatus, PermissionMode};
use robiade_core::assets::Asset;
use robiade_core::project::Workspace;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::dto::{AgentDto, AgentOutput};
use crate::now;

struct Live {
    agent: Agent,
    workspace: Workspace,
    process: Option<Arc<AgentProcess>>,
}

/// Les onglets inactifs depuis plus longtemps ne sont pas rouverts au démarrage.
const REOPEN_WITHIN_MS: i64 = 7 * 24 * 3600 * 1000;

pub struct AgentRegistry {
    app: AppHandle,
    workspaces: Arc<WorkspaceResolver>,
    journal: Arc<dyn AgentJournal>,
    login: LoginEnv,
    socket: PathBuf,
    live: Mutex<HashMap<AgentId, Live>>,
}

/// Échappement pour la commande de hook, exécutée par Claude Code via un shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl AgentRegistry {
    /// Rouvre les onglets récents de l'exécution précédente, terminés : ils se reprennent d'un clic.
    pub fn new(
        app: AppHandle,
        workspaces: Arc<WorkspaceResolver>,
        journal: Arc<dyn AgentJournal>,
        login: LoginEnv,
        socket: PathBuf,
    ) -> Self {
        let reopened = journal.recent(now() - REOPEN_WITHIN_MS).unwrap_or_default();
        let live = reopened
            .into_iter()
            .map(|agent| {
                let workspace = workspaces.resolve(agent.project_path());
                (agent.id().clone(), Live { agent, workspace, process: None })
            })
            .collect();
        Self { app, workspaces, journal, login, socket, live: Mutex::new(live) }
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
        let relay = shell_quote(&exe.to_string_lossy());

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

        let path = cwd.to_string_lossy().into_owned();
        let workspace = self.workspaces.resolve(&path);
        let mut agent = Agent::start(id.clone(), path, resume.as_ref().map(|r| r.as_str().to_owned()), now());
        let mut live = self.live.lock().map_err(|e| e.to_string())?;
        // Reprendre une session remplace son onglet terminé, qui lui passe son nom.
        if let Some(session) = &resume {
            let replaced: Vec<AgentId> = live
                .values()
                .filter(|l| l.process.is_none() && l.agent.session_id() == session.as_str())
                .map(|l| l.agent.id().clone())
                .collect();
            for old in replaced {
                if let Some(name) = live.remove(&old).and_then(|l| l.agent.name().map(str::to_owned)) {
                    agent.rename(&name);
                }
                let _ = self.journal.forget(&old);
            }
        }
        let _ = self.journal.save(&agent);
        let dto = AgentDto::from_agent(&agent, &workspace, now());
        live.insert(id, Live { agent, workspace, process: Some(Arc::new(process)) });
        drop(live);
        self.publish();
        Ok(dto)
    }

    pub fn apply(&self, id: &AgentId, event: AgentEvent) {
        self.on_event(id, event, None, None);
    }

    pub fn observe(&self, id: &AgentId, observation: Observation) {
        match observation {
            Observation::Event { event, permission_mode, written_file } => {
                self.on_event(id, event, permission_mode, written_file.as_deref())
            }
            Observation::Gauges(gauges) => {
                if let Ok(mut live) = self.live.lock() {
                    match live.get_mut(id) {
                        Some(entry) => entry.agent.observe_gauges(gauges),
                        None => return,
                    }
                }
                self.publish();
            }
        }
    }

    fn on_event(&self, id: &AgentId, event: AgentEvent, mode: Option<PermissionMode>, written_file: Option<&str>) {
        let ended = matches!(event, AgentEvent::ProcessExited);
        let now_waiting = {
            let Ok(mut live) = self.live.lock() else { return };
            // Un événement d'un agent inconnu (session hors Robiade, agent déjà retiré) est ignoré.
            let Some(entry) = live.get_mut(id) else { return };
            if let Some(mode) = mode {
                entry.agent.observe_permission_mode(mode);
            }
            if let Some(path) = written_file {
                entry.agent.observe_file_written(path, now());
            }
            let before = entry.agent.status();
            entry.agent.apply(event, now());
            if ended {
                entry.process = None;
            }
            let after = entry.agent.status();
            if after != before {
                let _ = self.journal.save(&entry.agent);
            }
            (after != before && after.is_waiting_on_me()).then(|| (after, entry.workspace.project_name().to_owned()))
        };
        if let Some((status, project)) = now_waiting {
            self.notify_waiting(status, &project);
        }
        self.publish();
    }

    /// Notification macOS quand un agent se met à m'attendre et que je regarde ailleurs.
    /// Le texte ne reprend rien du transcript : seulement le projet et la raison.
    fn notify_waiting(&self, status: AgentStatus, project: &str) {
        let focused = tauri::Manager::get_webview_window(&self.app, "main")
            .is_some_and(|w| w.is_focused().unwrap_or(false));
        if focused {
            return;
        }
        let reason = match status {
            AgentStatus::AwaitingPermission => "attend ta permission",
            _ => "a fini son tour",
        };
        let _ = self.app.notification().builder().title(project).body(reason).show();
    }

    pub fn rename(&self, id: &AgentId, name: &str) -> Result<(), String> {
        {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            let entry = live.get_mut(id).ok_or("agent inconnu")?;
            entry.agent.rename(name);
            self.journal.save(&entry.agent).map_err(|e| e.to_string())?;
        }
        self.publish();
        Ok(())
    }

    /// Ferme l'onglet d'un agent terminé ; sa session reste dans l'historique.
    pub fn forget(&self, id: &AgentId) -> Result<(), String> {
        {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            if live.get(id).is_some_and(|l| l.process.is_some()) {
                return Err("agent encore en cours : arrête-le d'abord".into());
            }
            live.remove(id);
            self.journal.forget(id).map_err(|e| e.to_string())?;
        }
        self.publish();
        Ok(())
    }

    /// Noms donnés aux onglets, par session Claude Code : l'historique les affiche aussi.
    pub fn names_by_session(&self) -> HashMap<String, String> {
        let Ok(live) = self.live.lock() else { return HashMap::new() };
        live.values()
            .filter_map(|l| Some((l.agent.session_id().to_owned(), l.agent.name()?.to_owned())))
            .collect()
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

    /// Un asset de l'agent : seuls les fichiers qu'il a écrits sont lisibles depuis l'interface.
    pub fn asset(&self, id: &AgentId, path: &str) -> Result<Asset, String> {
        let live = self.live.lock().map_err(|e| e.to_string())?;
        let entry = live.get(id).ok_or("agent inconnu")?;
        entry.agent.assets().get(path).cloned().ok_or_else(|| "fichier inconnu de cet agent".to_owned())
    }

    pub fn list(&self) -> Vec<AgentDto> {
        let at = now();
        let Ok(live) = self.live.lock() else { return vec![] };
        let mut out: Vec<AgentDto> = live.values().map(|l| AgentDto::from_agent(&l.agent, &l.workspace, at)).collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.started_at));
        out
    }

    /// Agents qu'une fermeture de Robiade interromprait en plein travail.
    pub fn interrupted_by_quit(&self) -> Vec<AgentDto> {
        let at = now();
        let Ok(live) = self.live.lock() else { return vec![] };
        live.values()
            .filter(|l| l.process.is_some() && l.agent.status().interrupted_by_quit())
            .map(|l| AgentDto::from_agent(&l.agent, &l.workspace, at))
            .collect()
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

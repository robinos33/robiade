//! Registre des agents vivants : relie le domaine (`Agent`), les PTY et les événements vers l'UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use robiade_adapters::hooks::{self, Observation};
use robiade_adapters::pty::{AgentProcess, LaunchSpec};
use robiade_adapters::shell_env::LoginEnv;
use robiade_adapters::workspace::WorkspaceResolver;
use robiade_core::agents::{Agent, AgentEvent, AgentId, AgentJournal, AgentStatus, Effort, PermissionMode};
use robiade_core::assets::Asset;
use robiade_core::project::Workspace;
use robiade_core::scratchpad;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::dto::{AgentDto, AgentOutput};
use crate::now;
use crate::task_cli;

struct Live {
    agent: Agent,
    workspace: Workspace,
    process: Option<Arc<AgentProcess>>,
}

/// Ce qu'un hook rapporte en plus de l'événement.
#[derive(Default)]
struct Observed {
    session: Option<String>,
    permission_mode: Option<PermissionMode>,
    effort: Option<Effort>,
    written_file: Option<String>,
    viewed_image: Option<String>,
}

/// Clé, dans les données d'une notification, de l'agent qu'un clic doit ouvrir.
pub const NOTIFIED_AGENT: &str = "agent";

/// Les onglets inactifs depuis plus longtemps ne sont pas rouverts au démarrage.
const REOPEN_WITHIN_MS: i64 = 7 * 24 * 3600 * 1000;

pub struct AgentRegistry {
    app: AppHandle,
    workspaces: Arc<WorkspaceResolver>,
    journal: Arc<dyn AgentJournal>,
    login: LoginEnv,
    socket: PathBuf,
    /// Base des tâches, donnée aux agents pour `robiade task done`.
    tasks_db: PathBuf,
    /// Racine résolue des scratchpads de Claude Code, `None` si elle ne se résout pas.
    scratchpad_root: Option<PathBuf>,
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
        tasks_db: PathBuf,
    ) -> Self {
        let reopened = journal.recent(now() - REOPEN_WITHIN_MS).unwrap_or_default();
        let live = reopened
            .into_iter()
            .map(|agent| {
                let workspace = workspaces.resolve(agent.project_path());
                (agent.id().clone(), Live { agent, workspace, process: None })
            })
            .collect();
        let scratchpad_root = robiade_adapters::scratchpad::root();
        Self { app, workspaces, journal, login, socket, tasks_db, scratchpad_root, live: Mutex::new(live) }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn claude_path(&self) -> Option<&str> {
        self.login.claude.as_deref()
    }

    /// `prompt` : premier message, déjà relu ; Claude Code le traite dès son démarrage.
    pub fn spawn(
        self: &Arc<Self>,
        cwd: &Path,
        resume: Option<AgentId>,
        prompt: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<AgentDto, String> {
        let claude = self.login.claude.as_deref().ok_or("binaire `claude` introuvable dans le PATH du shell de login")?;
        let id = AgentId::parse(&uuid::Uuid::new_v4().to_string()).map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let relay = shell_quote(&exe.to_string_lossy());

        let mut args = match &resume {
            Some(session) => vec!["--resume".to_owned(), session.as_str().to_owned()],
            None => vec!["--session-id".to_owned(), id.as_str().to_owned()],
        };
        args.extend(["--settings".to_owned(), hooks::settings_json(&relay)]);
        if let Some(prompt) = prompt {
            // Un argument qui commence par `-` serait lu comme une option de `claude`.
            if prompt.trim_start().starts_with('-') {
                return Err("le prompt ne peut pas commencer par « - »".into());
            }
            args.push(prompt);
        }

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
                    // Une reprise garde la session d'origine, à laquelle sa tâche est liée.
                    (task_cli::ENV_SESSION_ID, resume.as_ref().unwrap_or(&id).as_str().to_owned()),
                    (task_cli::ENV_DB, self.tasks_db.to_string_lossy().into_owned()),
                    (task_cli::ENV_BIN, exe.to_string_lossy().into_owned()),
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
                let _ = self.journal.close(&old);
            }
        }
        let _ = self.journal.save(&agent, now());
        let dto = AgentDto::from_agent(&agent, &workspace, now());
        live.insert(id, Live { agent, workspace, process: Some(Arc::new(process)) });
        drop(live);
        self.publish();
        Ok(dto)
    }

    pub fn apply(&self, id: &AgentId, event: AgentEvent) {
        self.on_event(id, event, Observed::default());
    }

    pub fn observe(&self, id: &AgentId, observation: Observation) {
        match observation {
            Observation::Event { event, session, permission_mode, effort, written_file, viewed_image } => {
                self.on_event(id, event, Observed { session, permission_mode, effort, written_file, viewed_image })
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

    fn on_event(&self, id: &AgentId, event: AgentEvent, observed: Observed) {
        let Observed { session, permission_mode: mode, effort, written_file, viewed_image } = observed;
        // Résolu hors du verrou : `/tmp` → `/private/tmp`, liens et `..` suivis.
        let written_resolved = written_file.as_deref().and_then(|p| Path::new(p).canonicalize().ok());
        let ended = matches!(event, AgentEvent::ProcessExited);
        let asks_reply = matches!(event, AgentEvent::WaitingForInput);
        let notification = {
            let Ok(mut live) = self.live.lock() else { return };
            // Un événement d'un agent inconnu (session hors Robiade, agent déjà retiré) est ignoré.
            let Some(entry) = live.get_mut(id) else { return };
            let session_before = entry.agent.session_id().to_owned();
            if let Some(session) = &session {
                entry.agent.observe_session(session);
            }
            if let Some(mode) = mode {
                entry.agent.observe_permission_mode(mode);
            }
            if let Some(effort) = effort {
                entry.agent.observe_effort(effort);
            }
            if let Some(path) = &written_file {
                let in_scratchpad = written_resolved
                    .as_deref()
                    .filter(|resolved| self.in_scratchpad(&entry.agent, resolved))
                    .and_then(Path::to_str);
                entry.agent.observe_file_written(path, in_scratchpad, now());
            }
            if let Some(path) = &viewed_image {
                entry.agent.observe_image_viewed(path, now());
            }
            let before = entry.agent.status();
            entry.agent.apply(event, now());
            if ended {
                entry.process = None;
            }
            let after = entry.agent.status();
            // La session change sans que le statut bouge (`/clear` sur un agent au repos).
            if after != before || entry.agent.session_id() != session_before {
                let _ = self.journal.save(&entry.agent, now());
            }
            attention_body(before, after, asks_reply)
                .map(|body| (notification_title(entry.workspace.project_name(), entry.agent.name()), body))
        };
        if let Some((title, body)) = notification {
            self.notify(id, &title, body);
        }
        self.publish();
    }

    /// `resolved` est dans le scratchpad de l'une des sessions de l'agent.
    fn in_scratchpad(&self, agent: &Agent, resolved: &Path) -> bool {
        self.scratchpad_root.as_deref().is_some_and(|root| scratchpad::holds(root, agent.sessions(), resolved))
    }

    /// Notification macOS quand un agent se met à m'attendre et que je regarde ailleurs.
    /// Elle porte l'id de l'agent : un clic dessus l'ouvre (`main::open_agent_from_notification`).
    fn notify(&self, id: &AgentId, title: &str, body: &str) {
        let focused = tauri::Manager::get_webview_window(&self.app, "main")
            .is_some_and(|w| w.is_focused().unwrap_or(false));
        if focused {
            return;
        }
        let _ = self.app.notification().builder().title(title).body(body).extra(NOTIFIED_AGENT, id.as_str()).show();
    }

    pub fn contains(&self, id: &AgentId) -> bool {
        self.live.lock().is_ok_and(|live| live.contains_key(id))
    }

    pub fn rename(&self, id: &AgentId, name: &str) -> Result<(), String> {
        {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            let entry = live.get_mut(id).ok_or("agent inconnu")?;
            entry.agent.rename(name);
            self.journal.save(&entry.agent, now()).map_err(|e| e.to_string())?;
        }
        self.publish();
        Ok(())
    }

    /// Arrête le process puis reprend la même session dans le même dossier : c'est ainsi qu'un agent
    /// passe sur une nouvelle version de Claude Code (« Restart to update »). L'onglet garde son nom.
    pub fn restart(self: &Arc<Self>, id: &AgentId, cols: u16, rows: u16) -> Result<AgentDto, String> {
        let (process, session, path) = {
            let live = self.live.lock().map_err(|e| e.to_string())?;
            let entry = live.get(id).ok_or("agent inconnu")?;
            (entry.process.clone(), entry.agent.session_id().to_owned(), entry.agent.project_path().to_owned())
        };
        let session = AgentId::parse(&session).map_err(|e| e.to_string())?;
        if let Some(process) = process {
            process.kill();
            // La reprise remplace l'onglet terminé : attendre que la sortie du process soit constatée.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while self.live.lock().map_err(|e| e.to_string())?.get(id).is_some_and(|l| l.process.is_some()) {
                if std::time::Instant::now() > deadline {
                    return Err("l'agent ne s'est pas arrêté".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        self.spawn(Path::new(&path), Some(session), None, cols, rows)
    }

    /// Ferme l'onglet d'un agent terminé ; sa session reste dans l'historique.
    pub fn forget(&self, id: &AgentId) -> Result<(), String> {
        {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            if live.get(id).is_some_and(|l| l.process.is_some()) {
                return Err("agent encore en cours : arrête-le d'abord".into());
            }
            live.remove(id);
            self.journal.close(id).map_err(|e| e.to_string())?;
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

    /// Dossier de travail d'un agent, tel qu'il a été lancé : jamais un chemin venu du front.
    pub fn project_path(&self, id: &AgentId) -> Result<String, String> {
        let live = self.live.lock().map_err(|e| e.to_string())?;
        live.get(id).map(|l| l.agent.project_path().to_owned()).ok_or_else(|| "agent inconnu".to_owned())
    }

    pub fn process(&self, id: &AgentId) -> Result<Arc<AgentProcess>, String> {
        let live = self.live.lock().map_err(|e| e.to_string())?;
        live.get(id).and_then(|l| l.process.clone()).ok_or_else(|| "agent inconnu ou terminé".to_owned())
    }

    /// `path` résolu (`..`, liens, `/tmp` → `/private/tmp`) s'il désigne un fichier existant du
    /// scratchpad de l'agent, écrit par un outil ou par une commande.
    pub fn scratchpad_file(&self, id: &AgentId, path: &str) -> Option<PathBuf> {
        let resolved = Path::new(path).canonicalize().ok()?;
        let held = {
            let live = self.live.lock().ok()?;
            self.in_scratchpad(&live.get(id)?.agent, &resolved)
        };
        (held && resolved.is_file()).then_some(resolved)
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

    /// Arrête tous les process ; les compteurs des agents vivants sont gardés avant.
    pub fn kill_all(&self) {
        if let Ok(live) = self.live.lock() {
            for l in live.values().filter(|l| l.process.is_some()) {
                let _ = self.journal.save(&l.agent, now());
            }
            live.values().filter_map(|l| l.process.as_ref()).for_each(|p| p.kill());
        }
    }

    /// Vrai si un agent tourne encore dans ce worktree.
    pub fn running_in(&self, worktree: &str) -> bool {
        let Ok(live) = self.live.lock() else { return true };
        live.values().any(|l| l.process.is_some() && l.workspace.worktree.as_deref() == Some(worktree))
    }

    /// Après la suppression d'un worktree, ses onglets terminés se rattachent à ce qui reste sur le disque.
    pub fn reattach(&self, worktree: &str) {
        if let Ok(mut live) = self.live.lock() {
            for l in live.values_mut().filter(|l| l.workspace.worktree.as_deref() == Some(worktree)) {
                l.workspace = self.workspaces.resolve(l.agent.project_path());
            }
        }
        self.publish();
    }

    /// Agents actifs depuis `since`, ceux de cette exécution comme ceux des précédentes.
    pub fn activity_since(&self, since: i64) -> Vec<Agent> {
        let mut agents: HashMap<AgentId, Agent> =
            self.journal.since(since).unwrap_or_default().into_iter().map(|a| (a.id().clone(), a)).collect();
        if let Ok(live) = self.live.lock() {
            agents.extend(live.values().map(|l| (l.agent.id().clone(), l.agent.clone())));
        }
        agents.into_values().collect()
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

/// Titre d'une notification : le projet, puis le nom de l'onglet s'il en a un.
fn notification_title(project: &str, agent: Option<&str>) -> String {
    match agent {
        Some(name) => format!("{project} · {name}"),
        None => project.to_owned(),
    }
}

/// Corps d'une notification, seulement quand l'agent se met à demander mon action.
/// Le texte ne reprend rien du transcript : seulement la raison.
/// `asks_reply` : la transition vient d'une notification d'inactivité, pas d'une fin de tour.
fn attention_body(before: AgentStatus, after: AgentStatus, asks_reply: bool) -> Option<&'static str> {
    if after == before || !after.needs_attention() {
        return None;
    }
    Some(match after {
        AgentStatus::AwaitingPermission => "attend ta permission",
        AgentStatus::Failed => "s'est arrêté sur une erreur",
        _ if asks_reply => "attend ta réponse",
        _ => "a fini son tour",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentStatus as S;

    #[test]
    fn title_names_the_project_then_the_agent() {
        assert_eq!(notification_title("robiade", Some("notifs")), "robiade · notifs");
        assert_eq!(notification_title("robiade", None), "robiade");
    }

    #[test]
    fn body_tells_why_the_agent_waits_on_me() {
        assert_eq!(attention_body(S::Running, S::AwaitingPermission, false), Some("attend ta permission"));
        assert_eq!(attention_body(S::Running, S::AwaitingInput, false), Some("a fini son tour"));
        assert_eq!(attention_body(S::Running, S::AwaitingInput, true), Some("attend ta réponse"));
        assert_eq!(attention_body(S::Running, S::Failed, false), Some("s'est arrêté sur une erreur"));
    }

    #[test]
    fn nothing_when_the_status_does_not_change_or_needs_nothing() {
        // `PermissionRequest` puis la notification `permission_prompt` : une seule notification.
        assert_eq!(attention_body(S::AwaitingPermission, S::AwaitingPermission, false), None);
        assert_eq!(attention_body(S::AwaitingInput, S::AwaitingInput, true), None);
        assert_eq!(attention_body(S::AwaitingPermission, S::Running, false), None);
        assert_eq!(attention_body(S::Running, S::Ended, false), None);
    }
}

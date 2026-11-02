// Pas de console Windows en release ; sans effet sur macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod commands;
mod containers;
mod db_sessions;
mod dto;
mod forge;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use robiade_adapters::claude_code::TranscriptStore;
use robiade_adapters::docker::DockerEngine;
use robiade_adapters::git::GitCli;
use robiade_adapters::gitlab::GitLab;
use robiade_adapters::sqlite_agents::SqliteAgentJournal;
use robiade_adapters::engines::Engines;
use robiade_adapters::sqlite_audit::SqliteAuditLog;
use robiade_adapters::sqlite_connections::SqliteConnectionRepository;
use robiade_adapters::sqlite_console_history::SqliteConsoleHistory;
use robiade_adapters::sqlite_search::SearchIndex;
use robiade_adapters::vault::VaultKv;
use robiade_adapters::workspace::WorkspaceResolver;
use robiade_adapters::sqlite_tasks::SqliteTaskRepository;
use robiade_adapters::{backup, hooks, shell_env, task_backup};
use robiade_core::containers::Containers;
use robiade_core::databases::{DatabaseProbe, Vault};
use robiade_core::db_browse::DatabaseBrowser;
use robiade_core::forge::Forge;
use robiade_core::merge_actions::AuditLog;
use robiade_core::vcs::{WorkingTree, Worktrees};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub transcripts: Arc<TranscriptStore>,
    pub search: Arc<SearchIndex>,
    pub workspaces: Arc<WorkspaceResolver>,
    pub tasks: SqliteTaskRepository,
    pub agents: Arc<agents::AgentRegistry>,
    pub vcs: Arc<dyn WorkingTree>,
    pub worktrees: Arc<dyn Worktrees>,
    pub forge: Arc<dyn Forge>,
    pub mr_states: Arc<forge::MrStates>,
    pub contributions: Arc<forge::Contributions>,
    pub audit: Arc<dyn AuditLog>,
    pub docker: Arc<dyn Containers>,
    pub log_streams: containers::LogStreams,
    pub connections: SqliteConnectionRepository,
    pub vault: Arc<dyn Vault>,
    pub databases: Arc<dyn DatabaseProbe>,
    pub browser: Arc<dyn DatabaseBrowser>,
    pub db_sessions: db_sessions::DbSessions,
    pub console_history: SqliteConsoleHistory,
    /// Fermeture confirmée dans l'interface malgré des agents au travail.
    pub quit_confirmed: AtomicBool,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Fenêtre créée en code pour lui interdire toute navigation hors de l'application :
/// un lien dans un transcript ou une description de MR ne peut pas remplacer l'interface.
fn main_window(app: &tauri::App) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title(if cfg!(debug_assertions) { "Robiade (dev)" } else { "Robiade" })
        .inner_size(1440.0, 900.0)
        .min_inner_size(1024.0, 640.0)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .on_navigation(|url| match url.scheme() {
            "tauri" => true,
            // Aperçu HTML d'un asset : iframe `srcdoc` sandboxée, sans script ni accès réseau.
            "about" => url.as_str() == "about:srcdoc",
            "http" => cfg!(debug_assertions) && url.host_str() == Some("localhost"),
            _ => false,
        })
        .build()?;
    Ok(())
}

/// Retient la fermeture si elle interromprait un agent, et demande confirmation à l'interface,
/// qui affiche les agents concernés. Renvoie `true` si la fermeture doit être annulée.
fn hold_quit(app: &tauri::AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else { return false };
    if state.quit_confirmed.load(Ordering::SeqCst) {
        return false;
    }
    let busy = state.agents.interrupted_by_quit();
    if busy.is_empty() || app.get_webview_window("main").is_none() {
        return false;
    }
    let _ = app.emit("app:quit-requested", &busy);
    true
}

/// Un `tauri dev` range ses données à part : il tourne à côté de l'application installée,
/// qui garde la base, le socket des hooks et l'index de recherche du dossier principal.
fn own_dir(dir: std::path::PathBuf) -> std::path::PathBuf {
    if cfg!(debug_assertions) { dir.join("dev") } else { dir }
}

fn main() {
    // Le même binaire sert de relais aux hooks et à la statusline de Claude Code.
    match std::env::args().nth(1).as_deref() {
        Some("hook") => return hooks::relay_from_stdin(),
        Some("statusline") => return hooks::relay_status_from_stdin(),
        _ => {}
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            main_window(app)?;
            let data_dir = own_dir(app.path().app_data_dir()?);
            std::fs::create_dir_all(&data_dir)?;
            let home = app.path().home_dir()?;

            let workspaces = Arc::new(WorkspaceResolver::default());
            let login = shell_env::resolve();
            let git = Arc::new(GitCli::with_path(login.path.clone()));
            let registry = Arc::new(agents::AgentRegistry::new(
                app.handle().clone(),
                workspaces.clone(),
                Arc::new(SqliteAgentJournal::open(&data_dir.join("robiade.db"))?),
                login,
                data_dir.join("hooks.sock"),
            ));
            let on_event = registry.clone();
            hooks::serve(registry.socket(), move |id, observation| on_event.observe(&id, observation))?;

            let transcripts = Arc::new(TranscriptStore::new(home.join(".claude").join("projects")));
            // L'index recopie le texte des transcripts : il vit dans le cache, exclu de Time Machine.
            let cache_dir = own_dir(app.path().app_cache_dir()?);
            std::fs::create_dir_all(&cache_dir)?;
            if let Err(e) = backup::exclude_from_backups(&cache_dir) {
                eprintln!("index de recherche non exclu de Time Machine : {e}");
            }
            for old in ["cache.db", "cache.db-wal", "cache.db-shm"] {
                let _ = std::fs::remove_file(data_dir.join(old));
            }
            let search = Arc::new(SearchIndex::open(&cache_dir.join("cache.db"))?);
            // Premier remplissage de l'index en tâche de fond : l'interface n'attend pas.
            let (warm_store, warm_index) = (transcripts.clone(), search.clone());
            std::thread::spawn(move || warm_index.sync(&warm_store.transcript_files()));

            // Export du jour au démarrage, puis vérifié toutes les heures : une app jamais quittée
            // en fait un par jour quand même.
            let backed_up = SqliteTaskRepository::open(&data_dir.join("robiade.db"))?;
            let backup_dir = data_dir.join("backups");
            std::thread::spawn(move || loop {
                let day = task_backup::local_day(std::time::SystemTime::now());
                if let Err(e) = task_backup::export_daily(&backed_up, &backup_dir, &day) {
                    eprintln!("export des tâches impossible : {e}");
                }
                std::thread::sleep(std::time::Duration::from_secs(3600));
            });

            let gitlab: Arc<dyn Forge> = Arc::new(GitLab::default());
            let docker: Arc<dyn Containers> = Arc::new(DockerEngine::new(home.clone())?);
            app.manage(AppState {
                transcripts,
                search,
                workspaces,
                tasks: SqliteTaskRepository::open(&data_dir.join("robiade.db"))?,
                agents: registry,
                vcs: git.clone(),
                worktrees: git,
                forge: gitlab.clone(),
                mr_states: Arc::new(forge::MrStates::new(gitlab.clone())),
                contributions: Arc::new(forge::Contributions::new(gitlab)),
                audit: Arc::new(SqliteAuditLog::open(&data_dir.join("robiade.db"))?),
                log_streams: containers::LogStreams::new(app.handle().clone(), docker.clone()),
                docker,
                connections: SqliteConnectionRepository::open(&data_dir.join("robiade.db"))?,
                vault: Arc::new(VaultKv::new(home.clone())),
                databases: Arc::new(Engines),
                browser: Arc::new(Engines),
                db_sessions: db_sessions::DbSessions::default(),
                console_history: SqliteConsoleHistory::open(&data_dir.join("robiade.db"))?,
                quit_confirmed: AtomicBool::new(false),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if hold_quit(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::environment,
            commands::list_history,
            commands::read_transcript,
            commands::search_history,
            commands::stats,
            commands::gitlab_activity,
            commands::list_tasks,
            commands::create_task,
            commands::update_task,
            commands::schedule_task,
            commands::link_task_issue,
            commands::link_task_session,
            commands::delete_task,
            commands::list_agents,
            commands::spawn_agent,
            commands::write_agent,
            commands::resize_agent,
            commands::kill_agent,
            commands::restart_agent,
            commands::mark_agent_seen,
            commands::rename_agent,
            commands::forget_agent,
            commands::read_asset,
            commands::working_changes,
            commands::working_diff,
            commands::merge_requests,
            commands::issues,
            commands::preview_mr_action,
            commands::run_mr_action,
            commands::preview_issue_action,
            commands::run_issue_action,
            commands::preview_issue_creation,
            commands::create_issue,
            commands::prepare_issue_start,
            commands::start_issue_agent,
            commands::prepare_task_start,
            commands::start_task_agent,
            commands::worktrees,
            commands::prepare_worktree_creation,
            commands::create_worktree,
            commands::prepare_worktree_removal,
            commands::remove_worktree,
            commands::reveal_in_finder,
            commands::audit_log,
            commands::containers,
            commands::prepare_container_action,
            commands::run_container_action,
            commands::follow_container_logs,
            commands::stop_container_logs,
            commands::db_connections,
            commands::save_db_connection,
            commands::delete_db_connection,
            commands::vault_paths,
            commands::vault_keys,
            commands::resolve_db_target,
            commands::test_db_connection,
            commands::open_db,
            commands::close_db,
            commands::db_page,
            commands::run_db_statement,
            commands::cancel_db_statement,
            commands::finish_db_write,
            commands::db_console_history,
            commands::merge_request_outcomes,
            commands::merge_request_states,
            commands::quit,
        ])
        .build(tauri::generate_context!())
        .expect("échec du démarrage de Robiade")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } if hold_quit(app) => api.prevent_exit(),
            tauri::RunEvent::Exit => app.state::<AppState>().agents.kill_all(),
            _ => {}
        });
}

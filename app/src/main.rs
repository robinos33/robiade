// Pas de console Windows en release ; sans effet sur macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod commands;
mod dto;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use robiade_adapters::claude_code::TranscriptStore;
use robiade_adapters::sqlite_tasks::SqliteTaskRepository;
use robiade_adapters::{hooks, shell_env};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub transcripts: Arc<TranscriptStore>,
    pub tasks: SqliteTaskRepository,
    pub agents: Arc<agents::AgentRegistry>,
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
        .title("Robiade")
        .inner_size(1440.0, 900.0)
        .min_inner_size(1024.0, 640.0)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .on_navigation(|url| match url.scheme() {
            "tauri" => true,
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

fn main() {
    // Le même binaire sert de relais aux hooks Claude Code : `robiade hook`.
    if std::env::args().nth(1).as_deref() == Some("hook") {
        hooks::relay_from_stdin();
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            main_window(app)?;
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let home = app.path().home_dir()?;

            let registry = Arc::new(agents::AgentRegistry::new(
                app.handle().clone(),
                shell_env::resolve(),
                data_dir.join("hooks.sock"),
            ));
            let on_event = registry.clone();
            hooks::serve(registry.socket(), move |id, event, mode| on_event.observe(&id, event, mode))?;

            app.manage(AppState {
                transcripts: Arc::new(TranscriptStore::new(home.join(".claude").join("projects"))),
                tasks: SqliteTaskRepository::open(&data_dir.join("robiade.db"))?,
                agents: registry,
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
            commands::stats,
            commands::list_tasks,
            commands::create_task,
            commands::update_task,
            commands::delete_task,
            commands::list_agents,
            commands::spawn_agent,
            commands::write_agent,
            commands::resize_agent,
            commands::kill_agent,
            commands::mark_agent_seen,
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

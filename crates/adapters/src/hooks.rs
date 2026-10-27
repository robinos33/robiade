//! Canal hooks Claude Code → Robiade.
//!
//! Claude Code exécute `robiade hook` à chaque événement ; ce relais lit l'entrée JSON du hook,
//! n'en garde que les champs utiles et les pousse sur un socket Unix réservé à mon utilisateur.
//! Aucun port réseau n'est ouvert.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use robiade_core::agents::{AgentEvent, AgentId, PermissionMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ENV_SOCKET: &str = "ROBIADE_HOOK_SOCKET";
pub const ENV_AGENT_ID: &str = "ROBIADE_AGENT_ID";
const MAX_MESSAGE_BYTES: u64 = 64 * 1024;

/// Ce qui transite sur le socket : jamais le prompt ni l'entrée des outils.
#[derive(Debug, Serialize, Deserialize)]
struct Relayed {
    agent_id: String,
    event: String,
    tool: Option<String>,
    notification: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
}

/// Événements déclarés dans le `--settings` passé à chaque agent.
pub const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// JSON de `--settings` : les hooks de Robiade n'existent que pour ses propres agents.
pub fn settings_json(relay_command: &str) -> String {
    let hook = serde_json::json!([{ "hooks": [{ "type": "command", "command": relay_command, "timeout": 5 }] }]);
    let hooks: serde_json::Map<String, Value> =
        HOOK_EVENTS.iter().map(|e| ((*e).to_owned(), hook.clone())).collect();
    serde_json::json!({ "hooks": hooks }).to_string()
}

/// Côté relais (`robiade hook`). Ne doit jamais bloquer ni faire échouer Claude Code :
/// toute erreur est avalée, le code de sortie reste 0.
pub fn relay_from_stdin() {
    let (Ok(socket), Ok(agent_id)) = (std::env::var(ENV_SOCKET), std::env::var(ENV_AGENT_ID)) else {
        return;
    };
    let mut input = String::new();
    if std::io::stdin().take(16 * 1024 * 1024).read_to_string(&mut input).is_err() {
        return;
    }
    let Some(message) = relayed_from(&input, agent_id) else { return };
    let Ok(mut stream) = UnixStream::connect(socket) else { return };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = serde_json::to_writer(&mut stream, &message);
    let _ = stream.flush();
}

/// Extrait de l'entrée d'un hook les seuls champs utiles à Robiade.
fn relayed_from(hook_input: &str, agent_id: String) -> Option<Relayed> {
    let v = serde_json::from_str::<Value>(hook_input).ok()?;
    let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    Some(Relayed {
        agent_id,
        event: field("hook_event_name")?,
        tool: field("tool_name"),
        notification: field("notification_type"),
        permission_mode: field("permission_mode"),
    })
}

fn translate(r: &Relayed) -> Option<AgentEvent> {
    Some(match r.event.as_str() {
        "SessionStart" => AgentEvent::SessionStarted,
        "UserPromptSubmit" => AgentEvent::PromptSubmitted,
        "PreToolUse" => AgentEvent::ToolStarted { tool: r.tool.clone().unwrap_or_default() },
        "PostToolUse" | "PostToolUseFailure" => AgentEvent::ToolFinished,
        "PermissionRequest" => AgentEvent::PermissionRequested,
        "Notification" => match r.notification.as_deref() {
            Some("permission_prompt") => AgentEvent::PermissionRequested,
            Some("idle_prompt") => AgentEvent::WaitingForInput,
            _ => return None,
        },
        "Stop" => AgentEvent::TurnFinished,
        "StopFailure" => AgentEvent::TurnFailed,
        "SessionEnd" => AgentEvent::SessionEnded,
        _ => return None,
    })
}

fn peer_is_me(stream: &UnixStream) -> bool {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: descripteur valide détenu par `stream`, pointeurs vers des variables locales.
    let ok = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0;
    // SAFETY: getuid n'a pas de précondition.
    ok && uid == unsafe { libc::getuid() }
}

/// Écoute le socket et appelle `on_event` pour chaque événement reconnu.
pub fn serve(
    socket: &Path,
    on_event: impl Fn(AgentId, AgentEvent, Option<PermissionMode>) + Send + Sync + 'static,
) -> std::io::Result<PathBuf> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    let on_event = std::sync::Arc::new(on_event);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if !peer_is_me(&stream) {
                continue;
            }
            let on_event = on_event.clone();
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut raw = String::new();
                if stream.take(MAX_MESSAGE_BYTES).read_to_string(&mut raw).is_err() {
                    return;
                }
                let Ok(relayed) = serde_json::from_str::<Relayed>(&raw) else { return };
                let (Ok(id), Some(event)) = (AgentId::parse(&relayed.agent_id), translate(&relayed)) else {
                    return;
                };
                on_event(id, event, relayed.permission_mode.as_deref().and_then(PermissionMode::parse));
            });
        }
    });
    Ok(socket.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn delivers_a_relayed_event_over_the_socket() {
        let socket = std::env::temp_dir().join(format!("robiade-hooks-{}/h.sock", std::process::id()));
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        serve(&socket, move |id, e, mode| tx.lock().unwrap().send((id, e, mode)).unwrap()).unwrap();
        let mut stream = UnixStream::connect(&socket).unwrap();
        let msg = Relayed {
            agent_id: "0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10".into(),
            event: "PreToolUse".into(),
            tool: Some("Bash".into()),
            notification: None,
            permission_mode: Some("acceptEdits".into()),
        };
        serde_json::to_writer(&mut stream, &msg).unwrap();
        drop(stream);
        let (id, event, mode) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(id.as_str(), "0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10");
        assert_eq!(event, AgentEvent::ToolStarted { tool: "Bash".into() });
        assert_eq!(mode.unwrap().as_str(), "acceptEdits");
        assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// Contrat : entrées réelles de Claude Code 2.1.311, anonymisées. À recapturer à chaque
    /// montée de version (`claude -p` avec un `--settings` dont chaque hook écrit son stdin).
    #[test]
    fn reads_real_hook_inputs() {
        let expected = [
            (AgentEvent::SessionStarted, None),
            (AgentEvent::PromptSubmitted, Some("default")),
            (AgentEvent::ToolStarted { tool: "Bash".into() }, Some("default")),
            (AgentEvent::ToolFinished, Some("default")),
            (AgentEvent::TurnFinished, Some("default")),
            (AgentEvent::SessionEnded, None),
        ];
        let inputs: Vec<&str> = include_str!("../fixtures/hooks-2.1.311.jsonl").lines().collect();
        assert_eq!(inputs.len(), expected.len());
        for (input, (event, mode)) in inputs.into_iter().zip(expected) {
            let relayed = relayed_from(input, "id".into()).unwrap();
            assert_eq!(translate(&relayed), Some(event));
            assert_eq!(relayed.permission_mode.as_deref(), mode);
        }
    }

    #[test]
    fn settings_declare_every_hook() {
        let v: Value = serde_json::from_str(&settings_json("'/x/robiade' hook")).unwrap();
        assert_eq!(v["hooks"].as_object().unwrap().len(), HOOK_EVENTS.len());
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "'/x/robiade' hook");
    }
}

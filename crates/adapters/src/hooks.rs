//! Canal hooks Claude Code → Robiade.
//!
//! Claude Code exécute `robiade hook` à chaque événement, et `robiade statusline` à chaque
//! rafraîchissement de la statusline ; ce relais lit l'entrée JSON, n'en garde que les champs
//! utiles et les pousse sur un socket Unix réservé à mon utilisateur. Aucun port réseau n'est ouvert.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use robiade_core::agents::{AgentEvent, AgentId, Effort, Gauges, PermissionMode};
use robiade_core::assets::AssetKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ENV_SOCKET: &str = "ROBIADE_HOOK_SOCKET";
pub const ENV_AGENT_ID: &str = "ROBIADE_AGENT_ID";
const MAX_MESSAGE_BYTES: u64 = 64 * 1024;

/// Ce qui transite sur le socket : jamais le prompt ni l'entrée des outils,
/// hormis le chemin d'un fichier écrit ou modifié par l'agent (pas son contenu).
#[derive(Debug, Serialize, Deserialize)]
struct Relayed {
    agent_id: String,
    /// Session Claude Code en cours : elle change à chaque `/clear`, l'agent restant le même.
    #[serde(default)]
    session_id: Option<String>,
    event: String,
    tool: Option<String>,
    notification: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    gauges: Option<RelayedGauges>,
    #[serde(default)]
    written_file: Option<String>,
    #[serde(default)]
    viewed_image: Option<String>,
}

/// Pseudo-événement porté par les relevés de la statusline.
const STATUS_LINE: &str = "StatusLine";

#[derive(Debug, Default, Serialize, Deserialize)]
struct RelayedGauges {
    model: Option<String>,
    context_percent: Option<u8>,
    context_tokens: Option<u64>,
    context_window: Option<u64>,
    five_hour_percent: Option<u8>,
    five_hour_resets_at: Option<i64>,
    week_percent: Option<u8>,
}

/// Ce que Robiade observe d'un agent par le socket.
#[derive(Debug, PartialEq, Eq)]
pub enum Observation {
    Event {
        event: AgentEvent,
        /// Session Claude Code en cours, identifiant bien formé seulement.
        session: Option<String>,
        permission_mode: Option<PermissionMode>,
        effort: Option<Effort>,
        written_file: Option<String>,
        /// Image lue par l'agent : souvent une capture qu'il vient de produire par une commande.
        viewed_image: Option<String>,
    },
    Gauges(Gauges),
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

/// JSON de `--settings` : les hooks et la statusline de Robiade n'existent que pour ses propres agents.
/// `relay` est le binaire Robiade, déjà échappé pour le shell qui exécute les commandes.
pub fn settings_json(relay: &str) -> String {
    let command = format!("{relay} hook");
    let hook = serde_json::json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]);
    let hooks: serde_json::Map<String, Value> =
        HOOK_EVENTS.iter().map(|e| ((*e).to_owned(), hook.clone())).collect();
    let status_line = serde_json::json!({ "type": "command", "command": format!("{relay} statusline"), "padding": 0 });
    serde_json::json!({ "hooks": hooks, "statusLine": status_line }).to_string()
}

/// Côté relais (`robiade hook`). Ne doit jamais bloquer ni faire échouer Claude Code :
/// toute erreur est avalée, le code de sortie reste 0.
pub fn relay_from_stdin() {
    relay_with(relayed_from);
}

/// Côté relais (`robiade statusline`). N'écrit rien : la barre est dessinée par Robiade,
/// pas dans le terminal.
pub fn relay_status_from_stdin() {
    relay_with(status_from);
}

fn relay_with(read: fn(&str, String) -> Option<Relayed>) {
    let (Ok(socket), Ok(agent_id)) = (std::env::var(ENV_SOCKET), std::env::var(ENV_AGENT_ID)) else {
        return;
    };
    let mut input = String::new();
    if std::io::stdin().take(16 * 1024 * 1024).read_to_string(&mut input).is_err() {
        return;
    }
    let Some(message) = read(&input, agent_id) else { return };
    let Ok(mut stream) = UnixStream::connect(socket) else { return };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = serde_json::to_writer(&mut stream, &message);
    let _ = stream.flush();
}

/// Extrait de l'entrée de la statusline les jauges affichées par Robiade.
fn status_from(input: &str, agent_id: String) -> Option<Relayed> {
    let v = serde_json::from_str::<Value>(input).ok()?;
    let at = |path: &str| v.pointer(path);
    let percent = |path: &str| at(path).and_then(Value::as_f64).map(|p| p.clamp(0.0, 100.0).round() as u8);
    let gauges = RelayedGauges {
        model: at("/model/display_name").and_then(Value::as_str).map(|m| m.chars().take(40).collect()),
        context_percent: percent("/context_window/used_percentage"),
        context_tokens: at("/context_window/total_input_tokens").and_then(Value::as_u64),
        context_window: at("/context_window/context_window_size").and_then(Value::as_u64),
        five_hour_percent: percent("/rate_limits/five_hour/used_percentage"),
        five_hour_resets_at: at("/rate_limits/five_hour/resets_at").and_then(Value::as_i64).map(|s| s * 1000),
        week_percent: percent("/rate_limits/seven_day/used_percentage"),
    };
    Some(Relayed {
        agent_id,
        session_id: None,
        event: STATUS_LINE.to_owned(),
        tool: None,
        notification: None,
        permission_mode: None,
        effort: None,
        gauges: Some(gauges),
        written_file: None,
        viewed_image: None,
    })
}

/// Extrait de l'entrée d'un hook les seuls champs utiles à Robiade.
fn relayed_from(hook_input: &str, agent_id: String) -> Option<Relayed> {
    let v = serde_json::from_str::<Value>(hook_input).ok()?;
    let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let event = field("hook_event_name")?;
    let tool = field("tool_name");
    let file = (event == "PostToolUse")
        .then(|| v.pointer("/tool_input/file_path").and_then(Value::as_str))
        .flatten()
        .filter(|p| p.len() <= 4096)
        .map(str::to_owned);
    let writes = matches!(tool.as_deref(), Some("Write" | "Edit" | "MultiEdit"));
    let reads_image = tool.as_deref() == Some("Read")
        && file.as_deref().and_then(AssetKind::of) == Some(AssetKind::Image);
    let (written_file, viewed_image) = match (writes, reads_image) {
        (true, _) => (file, None),
        (false, true) => (None, file),
        _ => (None, None),
    };
    Some(Relayed {
        agent_id,
        session_id: field("session_id"),
        event,
        tool,
        notification: field("notification_type"),
        permission_mode: field("permission_mode"),
        // Absent de SessionStart et UserPromptSubmit : il arrive avec le premier outil ou la fin du tour.
        effort: v.pointer("/effort/level").and_then(Value::as_str).map(str::to_owned),
        gauges: None,
        written_file,
        viewed_image,
    })
}

fn observe(r: Relayed) -> Option<Observation> {
    if r.event == STATUS_LINE {
        let g = r.gauges?;
        return Some(Observation::Gauges(Gauges {
            model: g.model,
            context_percent: g.context_percent.map(|p| p.min(100)),
            context_tokens: g.context_tokens,
            context_window: g.context_window,
            five_hour_percent: g.five_hour_percent.map(|p| p.min(100)),
            five_hour_resets_at: g.five_hour_resets_at,
            week_percent: g.week_percent.map(|p| p.min(100)),
        }));
    }
    let event = translate(&r)?;
    let permission_mode = r.permission_mode.as_deref().and_then(PermissionMode::parse);
    let effort = r.effort.as_deref().and_then(Effort::parse);
    // Un identifiant de session sert à reconnaître un chemin : rien d'autre qu'un UUID n'est gardé.
    let session = r.session_id.as_deref().and_then(|s| AgentId::parse(s).ok()).map(|s| s.as_str().to_owned());
    Some(Observation::Event {
        event,
        session,
        permission_mode,
        effort,
        written_file: r.written_file,
        viewed_image: r.viewed_image,
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

/// Écoute le socket et appelle `on_event` pour chaque observation reconnue.
pub fn serve(socket: &Path, on_event: impl Fn(AgentId, Observation) + Send + Sync + 'static) -> std::io::Result<PathBuf> {
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
                let Ok(id) = AgentId::parse(&relayed.agent_id) else { return };
                if let Some(observation) = observe(relayed) {
                    on_event(id, observation);
                }
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
        serve(&socket, move |id, o| tx.lock().unwrap().send((id, o)).unwrap()).unwrap();
        let mut stream = UnixStream::connect(&socket).unwrap();
        let msg = Relayed {
            agent_id: "0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10".into(),
            session_id: Some("9D259B93-5DC0-47C3-B1C8-3084E032DD44".into()),
            event: "PreToolUse".into(),
            tool: Some("Bash".into()),
            notification: None,
            permission_mode: Some("acceptEdits".into()),
            effort: Some("high".into()),
            gauges: None,
            written_file: None,
            viewed_image: None,
        };
        serde_json::to_writer(&mut stream, &msg).unwrap();
        drop(stream);
        let (id, observation) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(id.as_str(), "0b6f1c2e-6b1d-4a5e-9c33-2f1e8d7a4b10");
        assert_eq!(
            observation,
            Observation::Event {
                event: AgentEvent::ToolStarted { tool: "Bash".into() },
                session: Some("9d259b93-5dc0-47c3-b1c8-3084e032dd44".into()),
                permission_mode: PermissionMode::parse("acceptEdits"),
                effort: Effort::parse("high"),
                written_file: None,
                viewed_image: None,
            }
        );
        assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// Contrat : entrées réelles de Claude Code 2.1.311, anonymisées. À recapturer à chaque
    /// montée de version (`claude -p` avec un `--settings` dont chaque hook écrit son stdin).
    #[test]
    fn reads_real_hook_inputs() {
        let expected = [
            (AgentEvent::SessionStarted, None, None),
            (AgentEvent::PromptSubmitted, Some("default"), None),
            (AgentEvent::ToolStarted { tool: "Bash".into() }, Some("default"), Some("medium")),
            (AgentEvent::ToolFinished, Some("default"), Some("medium")),
            (AgentEvent::TurnFinished, Some("default"), Some("medium")),
            (AgentEvent::SessionEnded, None, None),
        ];
        let inputs: Vec<&str> = include_str!("../fixtures/hooks-2.1.311.jsonl").lines().collect();
        assert_eq!(inputs.len(), expected.len());
        for (input, (event, mode, effort)) in inputs.into_iter().zip(expected) {
            let observed = observe(relayed_from(input, "id".into()).unwrap());
            let permission_mode = mode.and_then(PermissionMode::parse);
            let effort = effort.and_then(Effort::parse);
            let session = Some("00000000-0000-4000-8000-000000000000".to_owned());
            assert_eq!(
                observed,
                Some(Observation::Event { event, session, permission_mode, effort, written_file: None, viewed_image: None })
            );
        }
    }

    #[test]
    fn relays_the_path_of_a_written_file_but_not_its_content() {
        let input = r#"{"hook_event_name":"PostToolUse","tool_name":"Write","permission_mode":"default",
            "tool_input":{"file_path":"/tmp/projet/capture.png","content":"secret"}}"#;
        let relayed = relayed_from(input, "id".into()).unwrap();
        assert!(!serde_json::to_string(&relayed).unwrap().contains("secret"));
        let Some(Observation::Event { written_file, .. }) = observe(relayed) else { panic!() };
        assert_eq!(written_file.as_deref(), Some("/tmp/projet/capture.png"));

        let edit = input.replace("\"Write\"", "\"Edit\"");
        assert!(relayed_from(&edit, "id".into()).unwrap().written_file.is_some());
        let pre = input.replace("PostToolUse", "PreToolUse");
        assert_eq!(relayed_from(&pre, "id".into()).unwrap().written_file, None);
        let read = input.replace("\"Write\"", "\"Read\"");
        assert_eq!(relayed_from(&read, "id".into()).unwrap().written_file, None);
    }

    #[test]
    fn relays_an_image_the_agent_reads_but_not_other_files() {
        let input = r#"{"hook_event_name":"PostToolUse","tool_name":"Read","permission_mode":"default",
            "tool_input":{"file_path":"/private/tmp/scratchpad/shot.png"}}"#;
        let Some(Observation::Event { written_file, viewed_image, .. }) = observe(relayed_from(input, "id".into()).unwrap())
        else {
            panic!()
        };
        assert_eq!((written_file, viewed_image.as_deref()), (None, Some("/private/tmp/scratchpad/shot.png")));

        let code = input.replace("shot.png", "Cart.php");
        assert_eq!(relayed_from(&code, "id".into()).unwrap().viewed_image, None);
        let notes = input.replace("shot.png", "SPEC.md");
        assert_eq!(relayed_from(&notes, "id".into()).unwrap().viewed_image, None);
    }

    #[test]
    fn keeps_only_a_well_formed_session_id() {
        let input = |session: &str| format!(r#"{{"hook_event_name":"SessionStart","session_id":"{session}"}}"#);
        let session = |input: String| match observe(relayed_from(&input, "id".into()).unwrap()) {
            Some(Observation::Event { session, .. }) => session,
            _ => panic!(),
        };
        assert_eq!(session(input("9D259B93-5DC0-47C3-B1C8-3084E032DD44")).as_deref(), Some("9d259b93-5dc0-47c3-b1c8-3084e032dd44"));
        assert_eq!(session(input("../../etc")), None);
        assert_eq!(session(r#"{"hook_event_name":"SessionStart"}"#.to_owned()), None);
    }

    #[test]
    fn reads_the_gauges_of_the_status_line() {
        let input = r#"{"model":{"display_name":"Opus 5.5"},"context_window":{"used_percentage":42.6,
            "total_input_tokens":85200,"context_window_size":200000},
            "rate_limits":{"five_hour":{"used_percentage":130,"resets_at":1793120000},"seven_day":{}}}"#;
        let Some(Observation::Gauges(g)) = observe(status_from(input, "id".into()).unwrap()) else { panic!() };
        assert_eq!(g.model.as_deref(), Some("Opus 5.5"));
        assert_eq!(g.context_percent, Some(43));
        assert_eq!(g.context_window, Some(200_000));
        assert_eq!(g.five_hour_percent, Some(100));
        assert_eq!(g.five_hour_resets_at, Some(1_793_120_000_000));
        assert_eq!(g.week_percent, None);
    }

    #[test]
    fn settings_declare_every_hook_and_the_status_line() {
        let v: Value = serde_json::from_str(&settings_json("'/x/robiade'")).unwrap();
        assert_eq!(v["hooks"].as_object().unwrap().len(), HOOK_EVENTS.len());
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "'/x/robiade' hook");
        assert_eq!(v["statusLine"]["command"], "'/x/robiade' statusline");
    }
}

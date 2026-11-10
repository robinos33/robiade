//! Modèles que Claude Code propose (son écran `/model`), demandés à la CLI elle-même : la réponse à
//! la requête `initialize` de son protocole de contrôle, celle que l'Agent SDK lit pour
//! `supportedModels()`. Aucun appel à l'API, aucune session enregistrée, aucun hook déclenché.
//!
//! Format sans contrat stable : la lecture est tolérante (entrée mal formée = sautée).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use robiade_core::agents::{is_cli_token, ModelOption};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_MODELS: usize = 50;
const REQUEST: &str = r#"{"type":"control_request","request_id":"robiade-models","request":{"subtype":"initialize"}}"#;

/// Lance `claude` en mode protocole, demande `initialize` et rend sa liste de modèles.
/// `cwd` : le dossier dont les réglages de projet s'appliquent.
pub fn fetch(claude: &str, path_env: &str, cwd: &Path) -> Result<Vec<ModelOption>, String> {
    let mut cmd = Command::new(claude);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ])
    .current_dir(cwd)
    .env("PATH", path_env)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    // Comme pour un agent : hérité d'une session Claude Code, `claude` se croirait sous-agent.
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(crate::pty::is_leaked_claude_var) {
            cmd.env_remove(name);
        }
    }
    let mut child = cmd.spawn().map_err(|e| format!("lancement de claude impossible : {e}"))?;
    let stdout = child.stdout.take().ok_or("sortie de claude indisponible")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let response = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
            .find(|v| v.pointer("/response/request_id").and_then(Value::as_str) == Some("robiade-models"));
        let _ = tx.send(response);
    });
    let sent = child.stdin.take().map(|mut stdin| writeln!(stdin, "{REQUEST}"));
    let response = match sent {
        Some(Ok(())) => rx.recv_timeout(TIMEOUT).ok().flatten(),
        _ => None,
    };
    let _ = child.kill();
    let _ = child.wait();
    let models = parse(&response.ok_or("claude n'a pas donné sa liste de modèles")?);
    if models.is_empty() {
        return Err("aucun modèle lisible dans la réponse de claude".into());
    }
    Ok(models)
}

/// Les modèles d'une réponse `initialize`, dans l'ordre de l'écran `/model`.
pub fn parse(response: &Value) -> Vec<ModelOption> {
    let Some(models) = response.pointer("/response/response/models").and_then(Value::as_array) else {
        return vec![];
    };
    models
        .iter()
        .filter_map(|m| {
            let value = m.get("value")?.as_str().filter(|v| is_cli_token(v))?.to_owned();
            let text = |key: &str| m.get(key).and_then(Value::as_str).map(|s| s.chars().take(120).collect::<String>());
            let efforts = m
                .get("supportedEffortLevels")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Value::as_str).filter(|e| is_cli_token(e)).map(str::to_owned).collect())
                .unwrap_or_default();
            Some(ModelOption {
                display_name: text("displayName").unwrap_or_else(|| value.clone()),
                description: text("description").unwrap_or_default(),
                value,
                efforts,
            })
        })
        .take(MAX_MODELS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_models_of_an_initialize_response_and_skips_malformed_ones() {
        let response: Value = serde_json::from_str(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"robiade-models","response":{
                "commands":[],"models":[
                {"value":"default","displayName":"Default (recommended)","description":"Opus 5.5 · Best for everyday, complex tasks",
                 "supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},
                {"value":"sonnet","displayName":"Sonnet 5.5","supportedEffortLevels":["low","--x"]},
                {"value":"claude-haiku-4-5-20251001","displayName":"Haiku 4.5","supportsThinkingOff":true},
                {"value":"--settings","displayName":"piège"},
                {"displayName":"sans valeur"}
            ]}}}"#,
        )
        .unwrap();
        let models = parse(&response);
        let values: Vec<&str> = models.iter().map(|m| m.value.as_str()).collect();
        assert_eq!(values, ["default", "sonnet", "claude-haiku-4-5-20251001"]);
        assert_eq!(models[0].display_name, "Default (recommended)");
        assert_eq!(models[0].efforts.len(), 5);
        assert_eq!(models[1].efforts, ["low"]);
        assert_eq!((models[2].description.as_str(), models[2].efforts.len()), ("", 0));
    }

    #[test]
    fn an_unexpected_response_gives_no_model() {
        assert!(parse(&serde_json::json!({"response": {"response": {}}})).is_empty());
    }

    /// Joue la vraie CLI : `cargo test -p robiade-adapters claude_models -- --ignored`.
    #[test]
    #[ignore]
    fn asks_the_installed_claude() {
        let path = std::env::var("PATH").unwrap();
        let models = fetch("claude", &path, &std::env::temp_dir()).unwrap();
        assert!(models.iter().any(|m| m.value == "default"), "{models:?}");
    }
}

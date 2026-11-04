//! Lecture des moteurs KV v2 de Vault, instance réglée uniquement, avec le jeton que la CLI
//! dépose après `vault login -method=oidc`. Robiade ne fait aucun flux de connexion lui-même.
//!
//! Les noms (chemins, clés) remontent à l'interface ; les valeurs, jamais : seule `read` en rend
//! une, à l'appelant qui ouvre une connexion.

use std::path::PathBuf;
use std::time::Duration;

use robiade_core::databases::{is_vault_path, is_vault_segment, Vault, VaultError, VaultRef};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;

use crate::keychain;

/// Bornes du parcours récursif : au-delà, le préfixe est trop large pour être une liste utile.
const MAX_PATHS: usize = 1000;
const MAX_REQUESTS: usize = 200;

pub struct VaultKv {
    http: ureq::Agent,
    api: String,
    /// `~/.vault-token`, relu à chaque appel : un `vault login` refait est pris sans redémarrer.
    token_file: PathBuf,
    /// Jeton de secours, si la CLI n'en a pas déposé : `security add-generic-password -s <service> …`.
    token_service: String,
}

impl VaultKv {
    /// `authority` : `hôte[:port]` du Vault, seul hôte vers lequel le jeton part.
    pub fn new(home: PathBuf, authority: &str, token_service: &str) -> Self {
        let http = ureq::Agent::config_builder()
            .https_only(true)
            // Aucune redirection : le jeton la suivrait vers un autre hôte.
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent("robiade")
            .build()
            .into();
        Self {
            http,
            api: format!("https://{authority}/v1"),
            token_file: home.join(".vault-token"),
            token_service: token_service.to_owned(),
        }
    }

    fn token(&self) -> Result<SecretString, VaultError> {
        let from_cli = std::fs::read_to_string(&self.token_file)
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .map(SecretString::from);
        from_cli.or_else(|| keychain::generic_password(&self.token_service)).ok_or(VaultError::NoToken)
    }

    /// `GET` sur l'API ; `path` est assemblé ici à partir de segments validés.
    fn get(&self, path: &str, what: &str) -> Result<Value, VaultError> {
        let token = self.token()?;
        let mut response = self
            .http
            .get(format!("{}/{path}", self.api))
            .header("X-Vault-Token", token.expose_secret())
            .call()
            .map_err(|e| VaultError::Network(e.to_string()))?;
        match response.status().as_u16() {
            200 => response
                .body_mut()
                .with_config()
                .limit(4 * 1024 * 1024)
                .read_json()
                .map_err(|e| VaultError::Api(e.to_string())),
            404 => Err(VaultError::NotFound(what.to_owned())),
            // Vault répond 403 aussi bien à un jeton expiré qu'à un droit manquant : on demande au
            // jeton lui-même s'il est encore valide.
            403 if self.token_is_valid(&token) => Err(VaultError::Forbidden(what.to_owned())),
            401 | 403 => Err(VaultError::TokenExpired),
            code => Err(VaultError::Api(format!("statut HTTP {code}"))),
        }
    }

    fn token_is_valid(&self, token: &SecretString) -> bool {
        self.http
            .get(format!("{}/auth/token/lookup-self", self.api))
            .header("X-Vault-Token", token.expose_secret())
            .call()
            .is_ok_and(|r| r.status() == 200)
    }

    fn list(&self, mount: &str, folder: &str) -> Result<Vec<String>, VaultError> {
        let listed: Listed = serde_json::from_value(self.get(&format!("{mount}/metadata/{folder}?list=true"), &format!("{mount}/{folder}"))?)
            .map_err(|e| VaultError::Api(e.to_string()))?;
        Ok(listed.data.keys)
    }

    fn secret(&self, mount: &str, path: &str) -> Result<serde_json::Map<String, Value>, VaultError> {
        let read: Read = serde_json::from_value(self.get(&format!("{mount}/data/{path}"), &format!("{mount}/{path}"))?)
            .map_err(|e| VaultError::Api(e.to_string()))?;
        Ok(read.data.data.unwrap_or_default())
    }
}

#[derive(Deserialize)]
struct Listed {
    data: ListedKeys,
}

#[derive(Deserialize)]
struct ListedKeys {
    keys: Vec<String>,
}

#[derive(Deserialize)]
struct Read {
    data: ReadData,
}

#[derive(Deserialize)]
struct ReadData {
    /// `null` pour une version supprimée.
    data: Option<serde_json::Map<String, Value>>,
}

fn invalid(what: &str) -> VaultError {
    VaultError::Api(format!("chemin refusé : {what}"))
}

/// Une valeur de secret en texte : telle quelle si c'est une chaîne, en JSON sinon.
fn text(value: Value) -> String {
    match value {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

impl Vault for VaultKv {
    fn paths(&self, mount: &str, prefix: &str) -> Result<Vec<String>, VaultError> {
        if !is_vault_segment(mount) || !is_vault_path(prefix) {
            return Err(invalid(prefix));
        }
        let root = if prefix.is_empty() || prefix.ends_with('/') { prefix.to_owned() } else { format!("{prefix}/") };
        let (mut found, mut folders, mut requests) = (Vec::new(), vec![root], 0);
        while let Some(folder) = folders.pop() {
            requests += 1;
            if requests > MAX_REQUESTS || found.len() >= MAX_PATHS {
                break;
            }
            let keys = match self.list(mount, &folder) {
                // Un dossier vide n'existe pas pour Vault : 404 sur le préfixe de départ seulement.
                Err(VaultError::NotFound(_)) if requests > 1 => continue,
                other => other?,
            };
            for key in keys {
                // Un nom qui sortirait de l'alphabet sûr ne servirait à rien : on ne pourrait pas le relire.
                let name = key.strip_suffix('/').unwrap_or(&key);
                if !is_vault_segment(name) {
                    continue;
                }
                if key.ends_with('/') {
                    folders.push(format!("{folder}{key}"));
                } else {
                    found.push(format!("{folder}{key}"));
                }
            }
        }
        found.sort();
        found.truncate(MAX_PATHS);
        Ok(found)
    }

    fn keys(&self, mount: &str, path: &str) -> Result<Vec<String>, VaultError> {
        if !is_vault_segment(mount) || !is_vault_path(path) || path.is_empty() || path.ends_with('/') {
            return Err(invalid(path));
        }
        // Les valeurs sont lues par l'API avec les clés, et jetées ici même.
        let mut keys: Vec<String> = self.secret(mount, path)?.into_iter().map(|(k, _)| k).collect();
        keys.sort();
        Ok(keys)
    }

    fn read(&self, reference: &VaultRef) -> Result<String, VaultError> {
        if !is_vault_segment(&reference.mount) || !is_vault_path(&reference.path) || reference.path.ends_with('/') {
            return Err(invalid(&reference.path));
        }
        let what = format!("{}/{}#{}", reference.mount, reference.path, reference.key);
        let mut secret = self.secret(&reference.mount, &reference.path)?;
        secret.remove(&reference.key).map(text).ok_or(VaultError::NotFound(what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_listing_and_a_secret_from_the_api_format() {
        let listed: Listed = serde_json::from_str(r#"{"data":{"keys":["DATABASE/","SUPER_MAIL/"]}}"#).unwrap();
        assert_eq!(listed.data.keys, ["DATABASE/", "SUPER_MAIL/"]);
        let read: Read = serde_json::from_str(
            r#"{"data":{"data":{"URL":"postgresql://u:p@h/db","PORT":5432},"metadata":{"version":3}}}"#,
        )
        .unwrap();
        let mut data = read.data.data.unwrap();
        assert_eq!(data.remove("URL").map(text).as_deref(), Some("postgresql://u:p@h/db"));
        assert_eq!(data.remove("PORT").map(text).as_deref(), Some("5432"));
        let deleted: Read = serde_json::from_str(r#"{"data":{"data":null,"metadata":{}}}"#).unwrap();
        assert!(deleted.data.data.is_none());
    }

    #[test]
    fn refuses_paths_outside_the_safe_alphabet_before_any_call() {
        let vault = VaultKv::new(PathBuf::from("/nonexistent"), "vault.example.com", "vault-token");
        assert!(matches!(vault.paths("shop-kv", "../sys"), Err(VaultError::Api(_))));
        assert!(matches!(vault.paths("kv?x=1", ""), Err(VaultError::Api(_))));
        assert!(matches!(vault.keys("shop-kv", "internal/prod/"), Err(VaultError::Api(_))));
    }
}

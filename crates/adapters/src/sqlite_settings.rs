//! Réglages du poste dans `robiade.db`, en clé-valeur : forge et Vault. Aucun secret, seulement des
//! hôtes et des noms de services du trousseau.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::settings::{
    ForgeProvider, ForgeSettings, InstanceUrl, KeychainService, SettingsError, VaultSettings, DEFAULT_FORGE_TOKEN_SERVICE,
    DEFAULT_VAULT_TOKEN_SERVICE,
};
use rusqlite::{params, OptionalExtension};

const FORGE_PROVIDER: &str = "forge.provider";
const FORGE_URL: &str = "forge.url";
const FORGE_TOKEN_SERVICE: &str = "forge.token_service";
const VAULT_URL: &str = "vault.url";
const VAULT_TOKEN_SERVICE: &str = "vault.token_service";

/// Réglages repris d'un profil antérieur à cette table.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Seed {
    pub forge: Option<ForgeSettings>,
    pub vault: Option<VaultSettings>,
}

pub struct SqliteSettings {
    conn: Mutex<rusqlite::Connection>,
}

fn storage(e: impl std::fmt::Display) -> SettingsError {
    SettingsError::Storage(e.to_string())
}

/// Nombre de tables de la base nommées `name`, ou de toutes.
pub(crate) fn table_count(conn: &rusqlite::Connection, name: Option<&str>) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND (?1 IS NULL OR name = ?1)",
        params![name],
        |r| r.get(0),
    )
}

/// Une base qui a déjà des tables vient d'une version antérieure, où la forge, le Vault et les
/// troncs étaient fixés dans le code. À demander avant d'ouvrir le moindre dépôt de la base.
pub fn is_earlier_profile(path: &Path) -> Result<bool, SettingsError> {
    let conn = rusqlite::Connection::open(path).map_err(storage)?;
    Ok(table_count(&conn, None).map_err(storage)? > 0)
}

impl SqliteSettings {
    /// `earlier_profile` (voir `is_earlier_profile`) : `legacy` reprend les réglages, une seule
    /// fois, à la création de la table. Une base neuve démarre sans forge ni Vault.
    pub fn open(path: &Path, earlier_profile: bool, legacy: impl FnOnce() -> Seed) -> Result<Self, SettingsError> {
        let conn = rusqlite::Connection::open(path).map_err(storage)?;
        conn.execute_batch("PRAGMA journal_mode = WAL;").map_err(storage)?;
        let created = table_count(&conn, Some("setting")).map_err(storage)? == 0;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .map_err(storage)?;
        let store = Self { conn: Mutex::new(conn) };
        if created && earlier_profile {
            let seed = legacy();
            store.save_forge(seed.forge.as_ref())?;
            store.save_vault(seed.vault.as_ref())?;
        }
        Ok(store)
    }

    fn get(&self, key: &str) -> Result<Option<String>, SettingsError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(storage)
    }

    /// Remplace ou retire (`None`) les clés données, en une transaction.
    fn put(&self, values: &[(&str, Option<&str>)]) -> Result<(), SettingsError> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(storage)?;
        for (key, value) in values {
            match value {
                Some(v) => tx.execute(
                    "INSERT INTO setting (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
                    params![key, v],
                ),
                None => tx.execute("DELETE FROM setting WHERE key = ?1", [key]),
            }
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// `None` sans forge réglée. Une valeur illisible vaut « pas de réglage » : elle a été validée à
    /// l'écriture, et une forge à moitié lue enverrait le jeton on ne sait où.
    pub fn forge(&self) -> Result<Option<ForgeSettings>, SettingsError> {
        let (Some(provider), Some(url)) = (self.get(FORGE_PROVIDER)?, self.get(FORGE_URL)?) else { return Ok(None) };
        let service = self.get(FORGE_TOKEN_SERVICE)?.unwrap_or_default();
        Ok((|| {
            Some(ForgeSettings {
                provider: ForgeProvider::parse(&provider).ok()?,
                url: InstanceUrl::parse(&url).ok()?,
                token_service: KeychainService::parse(&service, DEFAULT_FORGE_TOKEN_SERVICE).ok()?,
            })
        })())
    }

    pub fn vault(&self) -> Result<Option<VaultSettings>, SettingsError> {
        let Some(url) = self.get(VAULT_URL)? else { return Ok(None) };
        let service = self.get(VAULT_TOKEN_SERVICE)?.unwrap_or_default();
        Ok((|| {
            Some(VaultSettings {
                url: InstanceUrl::parse(&url).ok()?,
                token_service: KeychainService::parse(&service, DEFAULT_VAULT_TOKEN_SERVICE).ok()?,
            })
        })())
    }

    pub fn save_forge(&self, forge: Option<&ForgeSettings>) -> Result<(), SettingsError> {
        let url = forge.map(|f| f.url.url());
        self.put(&[
            (FORGE_PROVIDER, forge.map(|f| f.provider.as_str())),
            (FORGE_URL, url.as_deref()),
            (FORGE_TOKEN_SERVICE, forge.map(|f| f.token_service.as_str())),
        ])
    }

    pub fn save_vault(&self, vault: Option<&VaultSettings>) -> Result<(), SettingsError> {
        let url = vault.map(|v| v.url.url());
        self.put(&[(VAULT_URL, url.as_deref()), (VAULT_TOKEN_SERVICE, vault.map(|v| v.token_service.as_str()))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("robiade-settings-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn forge(host: &str) -> ForgeSettings {
        ForgeSettings {
            provider: ForgeProvider::GitLab,
            url: InstanceUrl::parse(host).unwrap(),
            token_service: KeychainService::parse("", DEFAULT_FORGE_TOKEN_SERVICE).unwrap(),
        }
    }

    fn vault(host: &str) -> VaultSettings {
        VaultSettings {
            url: InstanceUrl::parse(host).unwrap(),
            token_service: KeychainService::parse("mon-vault", DEFAULT_VAULT_TOKEN_SERVICE).unwrap(),
        }
    }

    #[test]
    fn a_new_profile_starts_without_forge_nor_vault() {
        let path = db("neuf");
        assert!(!is_earlier_profile(&path).unwrap());
        let settings = SqliteSettings::open(&path, false, || panic!("aucune reprise sur une base neuve")).unwrap();
        assert_eq!((settings.forge().unwrap(), settings.vault().unwrap()), (None, None));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_earlier_profile_keeps_its_settings_taken_over_once() {
        let path = db("ancien");
        rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TABLE task (id TEXT);").unwrap();
        let seed = Seed { forge: Some(forge("gitlab.example.com")), vault: Some(vault("vault.example.com:8200")) };
        assert!(is_earlier_profile(&path).unwrap());
        let settings = SqliteSettings::open(&path, true, || seed.clone()).unwrap();
        assert_eq!(settings.forge().unwrap(), seed.forge);
        assert_eq!(settings.vault().unwrap(), seed.vault);
        drop(settings);
        let reopened = SqliteSettings::open(&path, true, || panic!("reprise déjà faite")).unwrap();
        assert_eq!(reopened.forge().unwrap(), seed.forge);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_setting_is_replaced_or_removed_as_a_whole() {
        let path = db("ecriture");
        let settings = SqliteSettings::open(&path, false, Seed::default).unwrap();
        settings.save_forge(Some(&forge("a.example.com"))).unwrap();
        settings.save_forge(Some(&forge("b.example.com"))).unwrap();
        assert_eq!(settings.forge().unwrap(), Some(forge("b.example.com")));
        settings.save_vault(Some(&vault("v.example.com"))).unwrap();
        settings.save_forge(None).unwrap();
        assert_eq!(settings.forge().unwrap(), None);
        assert_eq!(settings.vault().unwrap(), Some(vault("v.example.com")));
        std::fs::remove_file(path).unwrap();
    }
}

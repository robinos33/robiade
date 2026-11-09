//! Page Réglages : forge et Vault, lus et écrits dans `robiade.db`. Les adaptateurs sont construits
//! au démarrage : un réglage enregistré s'applique à la relance. Les raccourcis du terminal, eux,
//! s'appliquent tout de suite.

use std::sync::Arc;

use robiade_adapters::github::GitHub;
use robiade_adapters::github_urls::GitHubUrls;
use robiade_adapters::gitlab::GitLab;
use robiade_adapters::gitlab_urls::GitLabUrls;
use robiade_adapters::sqlite_settings::SqliteSettings;
use robiade_adapters::vault::VaultKv;
use robiade_core::databases::{NoVault, Vault};
use robiade_core::forge::{Forge, ForgeUrls, NoForge};
use robiade_core::shortcuts::{Shortcut, Shortcuts};
use robiade_core::settings::{
    unconfirmed_host, ForgeProvider, ForgeSettings, InstanceUrl, KeychainService, SettingsError, VaultSettings,
    DEFAULT_VAULT_TOKEN_SERVICE,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

/// La forge réglée, et la forme de ses adresses ; `NoForge` sans réglage.
pub fn forge_of(settings: Option<&ForgeSettings>) -> (Arc<dyn Forge>, Arc<dyn ForgeUrls>) {
    match settings {
        Some(f) => match f.provider {
            ForgeProvider::GitLab => (
                Arc::new(GitLab::new(f.url.authority(), f.token_service.as_str())),
                Arc::new(GitLabUrls::new(f.url.authority())),
            ),
            ForgeProvider::GitHub => (
                Arc::new(GitHub::new(f.url.authority(), f.token_service.as_str())),
                Arc::new(GitHubUrls::new(f.url.authority())),
            ),
        },
        None => (Arc::new(NoForge), Arc::new(NoForge)),
    }
}

/// Le Vault réglé ; `NoVault` sans réglage.
pub fn vault_of(settings: Option<&VaultSettings>, home: std::path::PathBuf) -> Arc<dyn Vault> {
    match settings {
        Some(v) => Arc::new(VaultKv::new(home, v.url.authority(), v.token_service.as_str())),
        None => Arc::new(NoVault),
    }
}

/// Réglages en vigueur depuis le démarrage : ceux que les adaptateurs utilisent.
pub struct Applied {
    pub forge: Option<ForgeSettings>,
    pub vault: Option<VaultSettings>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForgeSettingsDto {
    pub provider: String,
    /// `https://hôte[:port]`.
    pub url: String,
    pub token_service: String,
}

impl From<&ForgeSettings> for ForgeSettingsDto {
    fn from(f: &ForgeSettings) -> Self {
        Self { provider: f.provider.as_str().to_owned(), url: f.url.url(), token_service: f.token_service.as_str().to_owned() }
    }
}

impl ForgeSettingsDto {
    fn parse(&self) -> Result<ForgeSettings, SettingsError> {
        let provider = ForgeProvider::parse(&self.provider)?;
        Ok(ForgeSettings {
            provider,
            url: InstanceUrl::parse(&self.url)?,
            token_service: KeychainService::parse(&self.token_service, provider.default_token_service())?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettingsDto {
    pub url: String,
    pub token_service: String,
}

impl From<&VaultSettings> for VaultSettingsDto {
    fn from(v: &VaultSettings) -> Self {
        Self { url: v.url.url(), token_service: v.token_service.as_str().to_owned() }
    }
}

impl VaultSettingsDto {
    fn parse(&self) -> Result<VaultSettings, SettingsError> {
        Ok(VaultSettings {
            url: InstanceUrl::parse(&self.url)?,
            token_service: KeychainService::parse(&self.token_service, DEFAULT_VAULT_TOKEN_SERVICE)?,
        })
    }
}

/// Réglages enregistrés, et ceux en vigueur : s'ils diffèrent, l'app doit être relancée.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub forge: Option<ForgeSettingsDto>,
    pub vault: Option<VaultSettingsDto>,
    pub restart_needed: bool,
    pub providers: Vec<ProviderDto>,
    /// Service du trousseau proposé quand le champ est vide.
    pub default_vault_token_service: &'static str,
}

/// Un fournisseur proposé dans Réglages.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub id: &'static str,
    pub default_token_service: &'static str,
}

fn read(store: &SqliteSettings, applied: &Applied) -> Result<SettingsDto, SettingsError> {
    let (forge, vault) = (store.forge()?, store.vault()?);
    Ok(SettingsDto {
        restart_needed: forge != applied.forge || vault != applied.vault,
        forge: forge.as_ref().map(Into::into),
        vault: vault.as_ref().map(Into::into),
        providers: ForgeProvider::ALL
            .iter()
            .map(|p| ProviderDto { id: p.as_str(), default_token_service: p.default_token_service() })
            .collect(),
        default_vault_token_service: DEFAULT_VAULT_TOKEN_SERVICE,
    })
}

/// Issue d'un enregistrement : fait, ou suspendu à la confirmation d'un nouvel hôte.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcomeDto {
    /// Réglages après enregistrement ; `None` si rien n'a été enregistré.
    pub settings: Option<SettingsDto>,
    /// Hôte vers lequel le jeton partirait, à confirmer en le renvoyant dans `confirmed_host`.
    pub confirm_host: Option<String>,
}

#[tauri::command]
pub fn settings(state: State<AppState>) -> CmdResult<SettingsDto> {
    read(&state.settings, &state.applied).map_err(|e| e.to_string())
}

/// Enregistre ou retire (`None`) la forge. Un nouvel hôte n'est enregistré que si `confirmed_host`
/// le répète : c'est vers lui que partira le jeton. Sinon, rien n'est écrit et l'hôte à confirmer
/// est renvoyé. L'hôte comparé est celui de l'API (`api.github.com` pour github.com) : changer de
/// fournisseur sur une même instance change aussi la destination du jeton.
#[tauri::command]
pub fn save_forge_settings(
    state: State<AppState>,
    forge: Option<ForgeSettingsDto>,
    confirmed_host: Option<String>,
) -> CmdResult<SaveOutcomeDto> {
    let save = || -> Result<SaveOutcomeDto, SettingsError> {
        let new = forge.as_ref().map(ForgeSettingsDto::parse).transpose()?;
        let old = state.settings.forge()?.map(|f| f.token_destination());
        let destination = new.as_ref().map(ForgeSettings::token_destination);
        let pending = unconfirmed_host(old.as_ref(), destination.as_ref(), confirmed_host.as_deref());
        if let Some(host) = pending {
            return Ok(SaveOutcomeDto { settings: None, confirm_host: Some(host.to_owned()) });
        }
        state.settings.save_forge(new.as_ref())?;
        Ok(SaveOutcomeDto { settings: Some(read(&state.settings, &state.applied)?), confirm_host: None })
    };
    save().map_err(|e| e.to_string())
}

/// Enregistre ou retire (`None`) le Vault, avec la même confirmation d'hôte que la forge : le jeton
/// de la CLI partirait lui aussi vers le nouvel hôte.
#[tauri::command]
pub fn save_vault_settings(
    state: State<AppState>,
    vault: Option<VaultSettingsDto>,
    confirmed_host: Option<String>,
) -> CmdResult<SaveOutcomeDto> {
    let save = || -> Result<SaveOutcomeDto, SettingsError> {
        let new = vault.as_ref().map(VaultSettingsDto::parse).transpose()?;
        let old = state.settings.vault()?;
        let pending = unconfirmed_host(old.as_ref().map(|v| &v.url), new.as_ref().map(|v| &v.url), confirmed_host.as_deref());
        if let Some(host) = pending {
            return Ok(SaveOutcomeDto { settings: None, confirm_host: Some(host.to_owned()) });
        }
        state.settings.save_vault(new.as_ref())?;
        Ok(SaveOutcomeDto { settings: Some(read(&state.settings, &state.applied)?), confirm_host: None })
    };
    save().map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutDto {
    pub label: String,
    pub text: String,
    /// Ce qu'un clic colle au curseur du prompt.
    pub pasted: String,
}

#[derive(Deserialize)]
pub struct ShortcutInputDto {
    pub label: String,
    pub text: String,
}

fn shortcut_dtos(list: &Shortcuts) -> Vec<ShortcutDto> {
    list.iter()
        .map(|s| ShortcutDto { label: s.label().to_owned(), text: s.text().to_owned(), pasted: s.pasted() })
        .collect()
}

#[tauri::command]
pub fn shortcuts(state: State<AppState>) -> CmdResult<Vec<ShortcutDto>> {
    state.settings.shortcuts().map(|l| shortcut_dtos(&l)).map_err(|e| e.to_string())
}

/// Remplace la liste entière, dans l'ordre donné. Un raccourci refusé n'enregistre rien.
#[tauri::command]
pub fn save_shortcuts(state: State<AppState>, shortcuts: Vec<ShortcutInputDto>) -> CmdResult<Vec<ShortcutDto>> {
    let list = shortcuts
        .iter()
        .enumerate()
        .map(|(i, s)| Shortcut::new(&s.label, &s.text).map_err(|e| format!("raccourci {} : {e}", i + 1)))
        .collect::<Result<Vec<_>, _>>()?;
    let list = Shortcuts::new(list).map_err(|e| e.to_string())?;
    state.settings.save_shortcuts(&list).map_err(|e| e.to_string())?;
    Ok(shortcut_dtos(&list))
}

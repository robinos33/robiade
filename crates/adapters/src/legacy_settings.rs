//! Réglages d'un profil antérieur aux réglages : avant eux, l'instance de forge et le Vault étaient
//! fixés dans le code. On les retrouve là où mes outils en ligne de commande les gardent déjà,
//! sans rien supposer de l'entreprise : l'hôte par défaut de `glab`, et `VAULT_ADDR`.

use std::path::Path;

use robiade_core::settings::{
    ForgeProvider, ForgeSettings, InstanceUrl, KeychainService, VaultSettings, DEFAULT_FORGE_TOKEN_SERVICE,
    DEFAULT_VAULT_TOKEN_SERVICE,
};

use crate::shell_env;
use crate::sqlite_settings::Seed;

/// Lu une seule fois, à la première ouverture d'un ancien profil par cette version.
pub fn seed(home: &Path) -> Seed {
    let forge = glab_host(home).and_then(|h| InstanceUrl::parse(&h).ok()).map(|url| ForgeSettings {
        provider: ForgeProvider::GitLab,
        url,
        token_service: KeychainService::parse("", DEFAULT_FORGE_TOKEN_SERVICE).expect("service par défaut valide"),
    });
    let vault = shell_env::interactive_vault_addr().and_then(|a| InstanceUrl::parse(&a).ok()).map(|url| VaultSettings {
        url,
        token_service: KeychainService::parse("", DEFAULT_VAULT_TOKEN_SERVICE).expect("service par défaut valide"),
    });
    Seed { forge, vault }
}

/// `host` de la config globale de `glab` : `~/Library/Application Support/glab-cli` sur macOS,
/// `~/.config/glab-cli` sinon.
fn glab_host(home: &Path) -> Option<String> {
    ["Library/Application Support/glab-cli/config.yml", ".config/glab-cli/config.yml"]
        .iter()
        .find_map(|file| std::fs::read_to_string(home.join(file)).ok())
        .and_then(|config| top_level_host(&config))
}

/// Clé `host` de premier niveau d'un YAML plat ; celles des sections `hosts:` sont indentées.
fn top_level_host(config: &str) -> Option<String> {
    config.lines().find_map(|line| {
        let value = line.strip_prefix("host:")?;
        let value = value.split(" #").next()?.trim().trim_matches(['"', '\'']);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_default_host_of_glab_not_those_of_its_hosts() {
        let config = "# glab\ngit_protocol: ssh\nhosts:\n    gitlab.com:\n        host: gitlab.com\nhost: \"gitlab.example.com\" # défaut\n";
        assert_eq!(top_level_host(config).as_deref(), Some("gitlab.example.com"));
        assert_eq!(top_level_host("hosts:\n    gitlab.com:\n        host: gitlab.com\n"), None);
        assert_eq!(top_level_host("host:\n"), None);
    }
}

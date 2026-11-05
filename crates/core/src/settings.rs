//! Réglages du poste : l'instance de forge et le Vault que Robiade interroge. Aucun secret : un jeton
//! se désigne par son service du trousseau, il ne se recopie pas.
//!
//! L'hôte réglé est le seul vers lequel le jeton part. Le changer demande une confirmation : sans
//! elle, une faute de frappe enverrait le jeton existant à un autre hôte.

/// Service du trousseau du jeton GitLab, quand le réglage ne le précise pas.
pub const DEFAULT_FORGE_TOKEN_SERVICE: &str = "gitlab-api-token";
/// Service du trousseau du jeton GitHub, quand le réglage ne le précise pas.
pub const DEFAULT_GITHUB_TOKEN_SERVICE: &str = "github-api-token";
/// L'instance publique de GitHub, dont l'API vit sur un autre hôte.
const GITHUB_COM: &str = "github.com";
/// Service du trousseau du jeton Vault de secours, quand le réglage ne le précise pas.
pub const DEFAULT_VAULT_TOKEN_SERVICE: &str = "vault-token";
const SERVICE_MAX_CHARS: usize = 100;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("URL refusée : {0} (attendu https://hôte, sans chemin ni identifiants)")]
    InvalidUrl(String),
    #[error("nom de service du trousseau refusé : {0}")]
    InvalidService(String),
    #[error("fournisseur inconnu : {0}")]
    UnknownProvider(String),
    #[error("stockage des réglages : {0}")]
    Storage(String),
}

/// Adresse d'une instance : `https://hôte[:port]`, rien d'autre. Ni chemin, ni requête, ni
/// identifiants : l'hôte est tout ce qui décide où part le jeton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceUrl {
    /// `hôte[:port]`, en minuscules.
    authority: String,
}

impl InstanceUrl {
    /// Accepte `https://hôte[:port]`, avec ou sans `/` final, ou l'hôte seul.
    pub fn parse(raw: &str) -> Result<Self, SettingsError> {
        let invalid = || SettingsError::InvalidUrl(raw.trim().to_owned());
        let trimmed = raw.trim();
        let rest = match trimmed.split_once("://") {
            Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => rest,
            Some(_) => return Err(invalid()),
            None => trimmed,
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest).to_ascii_lowercase();
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority.as_str(), None),
        };
        if !is_host(host) || port.is_some_and(|p| p.parse::<u16>().map_or(true, |p| p == 0) || p.starts_with('0')) {
            return Err(invalid());
        }
        Ok(Self { authority })
    }

    /// `hôte[:port]`.
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// `https://hôte[:port]`.
    pub fn url(&self) -> String {
        format!("https://{}", self.authority)
    }
}

/// Nom DNS : étiquettes de lettres, chiffres et tirets, séparées par des points. Ni `@`, ni `/`,
/// ni `%` : rien qui puisse déplacer l'hôte réel d'une URL.
fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Nom d'un service du trousseau, passé tel quel à `security -s`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainService(String);

impl KeychainService {
    /// Vide → `default`. Refusé : caractère de contrôle, tiret initial (lu comme une option), plus
    /// de 100 caractères.
    pub fn parse(raw: &str, default: &str) -> Result<Self, SettingsError> {
        let name = raw.trim();
        let name = if name.is_empty() { default } else { name };
        if name.starts_with('-') || name.chars().count() > SERVICE_MAX_CHARS || name.chars().any(char::is_control) {
            return Err(SettingsError::InvalidService(name.to_owned()));
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Fournisseurs de forge pris en charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeProvider {
    GitLab,
    /// github.com ou GitHub Enterprise Server.
    GitHub,
}

impl ForgeProvider {
    pub const ALL: [Self; 2] = [Self::GitLab, Self::GitHub];

    pub fn parse(raw: &str) -> Result<Self, SettingsError> {
        match raw {
            "gitlab" => Ok(Self::GitLab),
            "github" => Ok(Self::GitHub),
            other => Err(SettingsError::UnknownProvider(other.to_owned())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }

    /// Service du trousseau proposé quand le réglage ne le précise pas.
    pub fn default_token_service(self) -> &'static str {
        match self {
            Self::GitLab => DEFAULT_FORGE_TOKEN_SERVICE,
            Self::GitHub => DEFAULT_GITHUB_TOKEN_SERVICE,
        }
    }

    /// Hôte vers lequel le jeton part pour l'instance `url` : celui de l'API. Le même que l'instance,
    /// sauf pour github.com, dont l'API est servie par `api.github.com`.
    pub fn token_destination(self, url: &InstanceUrl) -> InstanceUrl {
        match self {
            Self::GitHub if url.authority == GITHUB_COM => InstanceUrl { authority: format!("api.{GITHUB_COM}") },
            _ => url.clone(),
        }
    }
}

/// La forge : une seule instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSettings {
    pub provider: ForgeProvider,
    pub url: InstanceUrl,
    pub token_service: KeychainService,
}

impl ForgeSettings {
    /// Hôte vers lequel le jeton part : c'est lui que la confirmation montre.
    pub fn token_destination(&self) -> InstanceUrl {
        self.provider.token_destination(&self.url)
    }
}

/// Le Vault, optionnel. Le jeton vient d'abord de la CLI (`~/.vault-token`), sinon du trousseau.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSettings {
    pub url: InstanceUrl,
    pub token_service: KeychainService,
}

/// Passer de `old` à `new` envoie le jeton vers un hôte qu'il ne visait pas : vrai dès que l'hôte
/// réglé change, y compris au premier réglage. Retirer le réglage n'envoie rien.
pub fn requires_confirmation(old: Option<&InstanceUrl>, new: Option<&InstanceUrl>) -> bool {
    new.is_some_and(|new| old != Some(new))
}

/// Hôte à confirmer avant d'enregistrer `new` : `None` si l'hôte ne change pas, ou si `confirmed`
/// (l'hôte montré à l'écran puis validé) le répète.
pub fn unconfirmed_host<'a>(
    old: Option<&InstanceUrl>,
    new: Option<&'a InstanceUrl>,
    confirmed: Option<&str>,
) -> Option<&'a str> {
    new.filter(|new| requires_confirmation(old, Some(new)) && confirmed != Some(new.authority())).map(InstanceUrl::authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_is_an_https_host_and_nothing_else() {
        let url = |raw| InstanceUrl::parse(raw).map(|u| u.authority().to_owned());
        assert_eq!(url("https://GitLab.Example.com/"), Ok("gitlab.example.com".into()));
        assert_eq!(url(" gitlab.example.com "), Ok("gitlab.example.com".into()));
        assert_eq!(url("https://gitlab.example.com:8443"), Ok("gitlab.example.com:8443".into()));
        for bad in [
            "",
            "http://gitlab.example.com",
            "https://gitlab.example.com/gitlab",
            "https://moi@gitlab.example.com",
            "https://gitlab.example.com?x",
            "https://gitlab.example.com#x",
            "https://gitlab.example.com:0",
            "https://gitlab.example.com:99999",
            "https://gitlab.example.com:080",
            "https://-x.example.com",
            "https://gitlab..example.com",
            "https://gitlab.example.com%2f.evil.io",
            "ftp://gitlab.example.com",
        ] {
            assert!(InstanceUrl::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(InstanceUrl::parse("gitlab.example.com").unwrap().url(), "https://gitlab.example.com");
    }

    #[test]
    fn a_keychain_service_defaults_and_cannot_pass_for_an_option() {
        assert_eq!(KeychainService::parse("  ", "def").unwrap().as_str(), "def");
        assert_eq!(KeychainService::parse(" mon jeton ", "def").unwrap().as_str(), "mon jeton");
        assert!(KeychainService::parse("-w", "def").is_err());
        assert!(KeychainService::parse("a\nb", "def").is_err());
        assert!(KeychainService::parse(&"x".repeat(101), "def").is_err());
    }

    #[test]
    fn the_github_token_leaves_for_the_api_host() {
        let destination = |provider, raw| ForgeProvider::token_destination(provider, &InstanceUrl::parse(raw).unwrap());
        assert_eq!(destination(ForgeProvider::GitHub, "https://GitHub.com").authority(), "api.github.com");
        assert_eq!(destination(ForgeProvider::GitHub, "github.example.com").authority(), "github.example.com");
        assert_eq!(destination(ForgeProvider::GitLab, "github.com").authority(), "github.com");
        for p in ForgeProvider::ALL {
            assert_eq!(ForgeProvider::parse(p.as_str()), Ok(p));
        }
        assert!(ForgeProvider::parse("bitbucket").is_err());
    }

    #[test]
    fn the_token_leaves_for_a_new_host_only_once_confirmed() {
        let a = InstanceUrl::parse("a.example.com").unwrap();
        let b = InstanceUrl::parse("b.example.com").unwrap();
        assert!(!requires_confirmation(Some(&a), Some(&a)));
        assert!(!requires_confirmation(Some(&a), None), "retirer le réglage n'envoie rien");
        assert!(requires_confirmation(Some(&a), Some(&b)));
        assert!(requires_confirmation(None, Some(&b)), "premier réglage : le jeton existe peut-être déjà");
        assert_eq!(unconfirmed_host(Some(&a), Some(&b), None), Some("b.example.com"));
        assert_eq!(unconfirmed_host(Some(&a), Some(&b), Some("a.example.com")), Some("b.example.com"));
        assert_eq!(unconfirmed_host(Some(&a), Some(&b), Some("b.example.com")), None);
        assert_eq!(unconfirmed_host(Some(&a), Some(&a), None), None);
        assert_eq!(unconfirmed_host(Some(&a), None, None), None);
    }
}

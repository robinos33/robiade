//! Connexions aux bases de données : leur description, d'où viennent leurs identifiants, et la
//! règle qui impose la lecture seule en production.
//!
//! Aucune valeur secrète n'est portée par une `Connection` : un mot de passe ou une URL complète
//! se désigne (trousseau, Vault), il ne se recopie pas. Les valeurs ne sont résolues qu'au moment
//! de se connecter, par l'appelant, puis assemblées ici en une `Target`.

pub const NAME_MAX_CHARS: usize = 100;
/// Seul hôte Vault que Robiade interroge : le jeton ne part vers aucun autre.
pub const VAULT_HOST: &str = "vault.example.com";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub String);

/// Moteurs prévus par la spec ; seul Postgres sait se connecter pour l'instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Postgres,
    MariaDb,
    Mongo,
    Elasticsearch,
}

impl Engine {
    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::MariaDb => 3306,
            Self::Mongo => 27017,
            Self::Elasticsearch => 9200,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Dev,
    Staging,
    Prod,
}

/// Champs d'une connexion. `Url` couvre le `DATABASE_URL` de Symfony ; les autres champs, quand ils
/// sont renseignés, l'emportent sur ce que l'URL contient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Url,
    Host,
    Port,
    Database,
    User,
    Password,
}

impl Part {
    /// Champs qui peuvent porter un mot de passe : jamais saisis en clair.
    pub fn is_secret(self) -> bool {
        matches!(self, Self::Url | Self::Password)
    }
}

/// Clé d'un secret KV v2 : `<montage>/<chemin>`, clé `<clé>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRef {
    pub mount: String,
    pub path: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Literal(String),
    /// Mot de passe générique du trousseau, saisi par moi dans un terminal.
    Keychain(String),
    Vault(VaultRef),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DbError {
    #[error("le nom est vide")]
    EmptyName,
    #[error("le nom dépasse {NAME_MAX_CHARS} caractères")]
    NameTooLong,
    #[error("il faut une URL ou un hôte")]
    NoTarget,
    #[error("{0:?} ne se saisit pas en clair : trousseau ou Vault")]
    SecretInClear(Part),
    #[error("valeur invalide pour {0:?}")]
    Invalid(Part),
    #[error("référence Vault invalide")]
    InvalidVaultRef,
    #[error("service du trousseau invalide")]
    InvalidKeychainService,
    #[error("une connexion de production est toujours en lecture seule")]
    ProdIsReadOnly,
    #[error("connexion introuvable")]
    NotFound,
    #[error("{0} : pas encore pris en charge")]
    Unsupported(&'static str),
    #[error("{0}")]
    Unreachable(String),
    #[error("stockage des connexions indisponible : {0}")]
    Storage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    id: ConnectionId,
    name: String,
    engine: Engine,
    stage: Stage,
    /// Absent : connexion globale.
    project_path: Option<String>,
    read_only: bool,
    fields: Vec<(Part, Source)>,
}

impl Connection {
    /// Valide tout : nom, champs, références. La production est forcée en lecture seule.
    pub fn create(
        id: ConnectionId,
        name: &str,
        engine: Engine,
        stage: Stage,
        project_path: Option<String>,
        read_only: bool,
        fields: Vec<(Part, Source)>,
    ) -> Result<Self, DbError> {
        if stage == Stage::Prod && !read_only {
            return Err(DbError::ProdIsReadOnly);
        }
        let mut fields = fields;
        fields.sort_by_key(|(part, _)| *part);
        fields.dedup_by_key(|(part, _)| *part);
        for (part, source) in &fields {
            Self::check(*part, source)?;
        }
        if !fields.iter().any(|(p, _)| matches!(p, Part::Url | Part::Host)) {
            return Err(DbError::NoTarget);
        }
        Ok(Self { id, name: Self::valid_name(name)?, engine, stage, project_path, read_only, fields })
    }

    /// Reconstitution depuis le stockage : les invariants ont été vérifiés à l'écriture, sauf la
    /// lecture seule de la production, réimposée ici quoi que dise la ligne.
    pub fn restore(
        id: ConnectionId,
        name: String,
        engine: Engine,
        stage: Stage,
        project_path: Option<String>,
        read_only: bool,
        fields: Vec<(Part, Source)>,
    ) -> Self {
        Self { id, name, engine, stage, project_path, read_only: read_only || stage == Stage::Prod, fields }
    }

    fn valid_name(raw: &str) -> Result<String, DbError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(DbError::EmptyName);
        }
        if name.chars().count() > NAME_MAX_CHARS {
            return Err(DbError::NameTooLong);
        }
        Ok(name.to_owned())
    }

    fn check(part: Part, source: &Source) -> Result<(), DbError> {
        match source {
            Source::Literal(_) if part.is_secret() => Err(DbError::SecretInClear(part)),
            Source::Literal(v) if part == Part::Port => v.trim().parse::<u16>().map(drop).map_err(|_| DbError::Invalid(part)),
            Source::Literal(v) if v.trim().is_empty() || v.chars().any(char::is_control) => Err(DbError::Invalid(part)),
            Source::Literal(_) => Ok(()),
            Source::Keychain(service) => {
                let ok = !service.is_empty() && service.len() <= 200 && !service.starts_with('-')
                    && !service.chars().any(char::is_control);
                ok.then_some(()).ok_or(DbError::InvalidKeychainService)
            }
            Source::Vault(r) => {
                let ok = is_vault_segment(&r.mount) && is_vault_path(&r.path) && !r.path.ends_with('/')
                    && !r.key.is_empty() && r.key.len() <= 200 && !r.key.chars().any(char::is_control);
                ok.then_some(()).ok_or(DbError::InvalidVaultRef)
            }
        }
    }

    pub fn id(&self) -> &ConnectionId {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn engine(&self) -> Engine {
        self.engine
    }
    pub fn stage(&self) -> Stage {
        self.stage
    }
    pub fn project_path(&self) -> Option<&str> {
        self.project_path.as_deref()
    }
    pub fn read_only(&self) -> bool {
        self.read_only
    }
    pub fn fields(&self) -> &[(Part, Source)] {
        &self.fields
    }
}

/// Un montage ou un segment de chemin Vault : il finit dans une URL, d'où l'alphabet restreint.
pub fn is_vault_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Chemin sous un montage, `a/b/c`, avec un `/` final toléré (préfixe d'un dossier). Vide : la racine.
pub fn is_vault_path(s: &str) -> bool {
    s.is_empty() || s.strip_suffix('/').unwrap_or(s).split('/').all(is_vault_segment)
}

/// Où se connecter, une fois toutes les valeurs résolues. Ne vit que le temps de la connexion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    /// `sslmode` de l'URL, seul paramètre de requête repris ; les autres (`serverVersion`,
    /// `charset`) ne concernent que Doctrine.
    pub ssl_mode: Option<String>,
}

impl Target {
    /// Part de l'URL s'il y en a une, puis applique les champs renseignés un à un.
    pub fn assemble(engine: Engine, values: &[(Part, String)]) -> Result<Self, DbError> {
        let url = values.iter().find(|(p, _)| *p == Part::Url).map(|(_, v)| v.as_str());
        let mut target = match url {
            Some(url) => parse_url(url).ok_or(DbError::Invalid(Part::Url))?,
            None => Self { host: String::new(), port: 0, database: None, user: None, password: None, ssl_mode: None },
        };
        for (part, raw) in values {
            let value = raw.trim();
            match part {
                Part::Url => {}
                Part::Host => target.host = value.to_owned(),
                Part::Port => target.port = value.parse().map_err(|_| DbError::Invalid(Part::Port))?,
                Part::Database => target.database = Some(value.to_owned()),
                Part::User => target.user = Some(value.to_owned()),
                // Pas de `trim` sur un mot de passe : une espace peut en faire partie.
                Part::Password => target.password = Some(raw.clone()),
            }
        }
        if target.host.is_empty() {
            return Err(DbError::NoTarget);
        }
        if target.port == 0 {
            target.port = engine.default_port();
        }
        Ok(target)
    }
}

/// `postgres[ql]://user:pass@host:port/base?sslmode=…`, identifiants décodés.
fn parse_url(url: &str) -> Option<Target> {
    let rest = url.trim().strip_prefix("postgresql://").or_else(|| url.trim().strip_prefix("postgres://"))?;
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    // Le dernier `@` : un mot de passe mal encodé peut en contenir un.
    let (userinfo, rest) = match rest.rfind('@') {
        Some(i) => (Some(&rest[..i]), &rest[i + 1..]),
        None => (None, rest),
    };
    let (authority, database) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match authority.strip_prefix('[') {
        // IPv6 entre crochets.
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        },
    };
    let port = match port {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    let (user, password) = match userinfo {
        Some(info) => match info.split_once(':') {
            Some((u, p)) => (Some(percent_decode(u)?), Some(percent_decode(p)?)),
            None => (Some(percent_decode(info)?), None),
        },
        None => (None, None),
    };
    let ssl_mode = query.split('&').find_map(|kv| kv.strip_prefix("sslmode=")).map(str::to_owned);
    Some(Target {
        host: percent_decode(host)?,
        port,
        database: (!database.is_empty()).then(|| percent_decode(database)).flatten(),
        user: user.filter(|u| !u.is_empty()),
        password,
        ssl_mode,
    })
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Ce qu'une connexion d'essai a constaté sur le serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub server_version: String,
    pub user: String,
    pub database: String,
    /// Lu sur le serveur (`SHOW transaction_read_only`), pas déduit de la demande.
    pub read_only: bool,
}

pub trait ConnectionRepository: Send + Sync {
    fn all(&self) -> Result<Vec<Connection>, DbError>;
    fn get(&self, id: &ConnectionId) -> Result<Connection, DbError>;
    fn save(&self, connection: &Connection) -> Result<(), DbError>;
    fn delete(&self, id: &ConnectionId) -> Result<(), DbError>;
}

/// Ouvre une session, la referme aussitôt et rapporte ce qu'elle a vu.
pub trait DatabaseProbe: Send + Sync {
    fn probe(&self, engine: Engine, target: &Target, read_only: bool) -> Result<Probe, DbError>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("aucun jeton Vault : lance `vault login -method=oidc` dans un terminal")]
    NoToken,
    #[error("jeton Vault expiré ou révoqué : relance `vault login -method=oidc` dans un terminal")]
    TokenExpired,
    #[error("Vault refuse l'accès à {0}")]
    Forbidden(String),
    #[error("introuvable dans Vault : {0}")]
    NotFound(String),
    #[error("Vault injoignable : {0}")]
    Network(String),
    #[error("réponse inattendue de Vault : {0}")]
    Api(String),
}

/// Lecture d'un moteur KV v2. Les deux premières méthodes ne rendent que des noms ; seule `read`
/// rend une valeur, pour la connexion en cours.
pub trait Vault: Send + Sync {
    /// Chemins des secrets sous `prefix`, en descendant dans les dossiers.
    fn paths(&self, mount: &str, prefix: &str) -> Result<Vec<String>, VaultError>;
    /// Clés d'un secret, sans leurs valeurs.
    fn keys(&self, mount: &str, path: &str) -> Result<Vec<String>, VaultError>;
    fn read(&self, reference: &VaultRef) -> Result<String, VaultError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(path: &str, key: &str) -> Source {
        Source::Vault(VaultRef { mount: "forum-kv".into(), path: path.into(), key: key.into() })
    }

    fn create(stage: Stage, read_only: bool, fields: Vec<(Part, Source)>) -> Result<Connection, DbError> {
        Connection::create(ConnectionId("c".into()), " Forum ", Engine::Postgres, stage, None, read_only, fields)
    }

    #[test]
    fn production_is_read_only_whatever_is_asked_or_stored() {
        let url = vec![(Part::Url, vault("internal/prod/DATABASE", "URL"))];
        assert_eq!(create(Stage::Prod, false, url.clone()), Err(DbError::ProdIsReadOnly));
        assert!(create(Stage::Prod, true, url.clone()).unwrap().read_only());
        assert!(create(Stage::Dev, false, url.clone()).is_ok());
        let stored = Connection::restore(ConnectionId("c".into()), "x".into(), Engine::Postgres, Stage::Prod, None, false, url);
        assert!(stored.read_only());
    }

    #[test]
    fn secrets_are_never_typed_in_clear() {
        let fields = vec![(Part::Host, Source::Literal("localhost".into())), (Part::Password, Source::Literal("x".into()))];
        assert_eq!(create(Stage::Dev, true, fields), Err(DbError::SecretInClear(Part::Password)));
        let url = vec![(Part::Url, Source::Literal("postgresql://a:b@h/db".into()))];
        assert_eq!(create(Stage::Dev, true, url), Err(DbError::SecretInClear(Part::Url)));
        let keychain = vec![(Part::Host, Source::Literal("localhost".into())), (Part::Password, Source::Keychain("pg-local".into()))];
        assert_eq!(create(Stage::Dev, true, keychain).unwrap().name(), "Forum");
    }

    #[test]
    fn validates_fields_and_references() {
        assert_eq!(create(Stage::Dev, true, vec![]), Err(DbError::NoTarget));
        let port = vec![(Part::Host, Source::Literal("h".into())), (Part::Port, Source::Literal("70000".into()))];
        assert_eq!(create(Stage::Dev, true, port), Err(DbError::Invalid(Part::Port)));
        for bad in ["../prod/DATABASE", "internal/prod/", "a b/c", "a//b"] {
            assert_eq!(create(Stage::Dev, true, vec![(Part::Url, vault(bad, "URL"))]), Err(DbError::InvalidVaultRef), "{bad}");
        }
        let flag = vec![(Part::Host, Source::Literal("h".into())), (Part::Password, Source::Keychain("-w".into()))];
        assert_eq!(create(Stage::Dev, true, flag), Err(DbError::InvalidKeychainService));
    }

    #[test]
    fn reads_a_symfony_database_url() {
        let url = "postgresql://app:p%40ss%3Aword@db.local:6432/forum?serverVersion=11&charset=utf8&sslmode=require";
        let t = Target::assemble(Engine::Postgres, &[(Part::Url, url.into())]).unwrap();
        assert_eq!(
            t,
            Target {
                host: "db.local".into(),
                port: 6432,
                database: Some("forum".into()),
                user: Some("app".into()),
                password: Some("p@ss:word".into()),
                ssl_mode: Some("require".into()),
            }
        );
        let v6 = Target::assemble(Engine::Postgres, &[(Part::Url, "postgres://[::1]/db".into())]).unwrap();
        assert_eq!((v6.host.as_str(), v6.port), ("::1", 5432));
        assert_eq!(Target::assemble(Engine::Postgres, &[(Part::Url, "mysql://h/db".into())]), Err(DbError::Invalid(Part::Url)));
    }

    #[test]
    fn fields_override_the_url() {
        let values = [
            (Part::Url, "postgresql://app:secret@db.local/forum".into()),
            (Part::Database, "forum_ro".into()),
            (Part::User, " lecteur ".into()),
            (Part::Password, " avec espace".into()),
        ];
        let t = Target::assemble(Engine::Postgres, &values).unwrap();
        assert_eq!(t.database.as_deref(), Some("forum_ro"));
        assert_eq!(t.user.as_deref(), Some("lecteur"));
        assert_eq!(t.password.as_deref(), Some(" avec espace"));
        assert_eq!(Target::assemble(Engine::Postgres, &[(Part::User, "u".into())]), Err(DbError::NoTarget));
    }
}

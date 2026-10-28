//! Connexions aux bases de données : leur description, d'où viennent leurs identifiants, et la
//! règle qui impose la lecture seule en production.
//!
//! Un mot de passe ou une URL complète se désigne (trousseau, Vault), il ne se recopie pas. Seule
//! exception : une connexion dev vers la machine locale, dont l'identifiant est déjà en clair dans
//! le `.env` du projet. Les valeurs ne sont résolues qu'au moment de se connecter, par l'appelant,
//! puis assemblées ici en une `Target`.

pub const NAME_MAX_CHARS: usize = 100;
/// Seul hôte Vault que Robiade interroge : le jeton ne part vers aucun autre.
pub const VAULT_HOST: &str = "vault.example.com";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub String);

/// Moteurs prévus par la spec ; Postgres et MariaDB savent se connecter pour l'instant.
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

    /// Langage de la console et des requêtes de page ; absent pour un moteur pas encore pris en charge.
    pub fn language(self) -> Option<Language> {
        match self {
            Self::Postgres => Some(Language::Sql(Dialect::Postgres)),
            Self::MariaDb => Some(Language::Sql(Dialect::MariaDb)),
            Self::Elasticsearch => Some(Language::Elastic),
            Self::Mongo => None,
        }
    }

    /// Le serveur sait tenir une session en lecture seule, et le confirmer. Elasticsearch et Mongo
    /// ne le savent pas : la lecture seule y est celle du compte, ou à défaut celle que la console
    /// impose par sa liste blanche.
    pub fn session_read_only(self) -> bool {
        matches!(self, Self::Postgres | Self::MariaDb)
    }

    /// Préfixes d'URL acceptés : ceux du `DATABASE_URL` de Symfony.
    fn schemes(self) -> &'static [&'static str] {
        match self {
            Self::Postgres => &["postgresql://", "postgres://"],
            Self::MariaDb => &["mysql://", "mariadb://"],
            Self::Elasticsearch => &["https://", "http://"],
            Self::Mongo => &[],
        }
    }
}

/// Ce que parle la console d'une connexion, et dans quoi s'écrit une requête de page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Sql(Dialect),
    /// Requêtes REST au format de la Dev Tools de Kibana.
    Elastic,
}

impl From<Dialect> for Language {
    fn from(dialect: Dialect) -> Self {
        Self::Sql(dialect)
    }
}

/// Dialecte SQL : il change le découpage du texte, la citation des noms et les garde-fous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    MariaDb,
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
    /// Champs qui peuvent porter un mot de passe : en clair seulement en dev, vers la machine locale.
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
    #[error("{0:?} ne se saisit en clair que pour une connexion dev vers localhost : ailleurs, trousseau ou Vault")]
    SecretInClear(Part),
    #[error("valeur invalide pour {0:?}")]
    Invalid(Part),
    #[error("référence Vault invalide")]
    InvalidVaultRef,
    #[error("service du trousseau invalide")]
    InvalidKeychainService,
    #[error("mot de passe en clair refusé pour cette connexion (staging/prod ou hôte distant) : utilise le trousseau ou Vault")]
    ClearSecretRefused,
    #[error("une connexion de production est toujours en lecture seule")]
    ProdIsReadOnly,
    #[error("hors dev, la connexion doit être chiffrée : retire `sslmode=disable|allow|prefer` (TLS exigé)")]
    TlsRequired,
    #[error("`sslmode=disable` n'est accepté que vers localhost : ailleurs, le mot de passe passerait en clair")]
    ClearTextOffLocalhost,
    #[error("colonne ou table inconnue : {0}")]
    UnknownColumn(String),
    #[error("trop de filtres (10 au plus)")]
    TooManyFilters,
    #[error("un filtre « contient » demande une valeur")]
    EmptyFilter,
    #[error("curseur de pagination invalide : recharge la table")]
    InvalidCursor,
    #[error("connexion introuvable")]
    NotFound,
    #[error("une requête commence par sa méthode et son chemin : GET /index/_search")]
    NotARequest,
    #[error("aucune instruction à exécuter")]
    NoStatement,
    #[error("une seule instruction à la fois : sélectionne-la, ou place le curseur dedans")]
    MultipleStatements,
    #[error("pas de contrôle de transaction dans la console : chaque instruction part seule et est validée aussitôt")]
    TransactionControl,
    #[error("refusé sur une connexion en lecture seule : {0}")]
    ReadOnlyGuard(&'static str),
    #[error("en production, la console n'accepte que des lectures, et des UPDATE ou DELETE validés")]
    ProdRefused,
    #[error("en production, la console Elasticsearch n'accepte que des lectures : {0} n'en est pas une")]
    ProdReadsOnly(String),
    #[error("UPDATE ou DELETE sans WHERE refusé en production")]
    ProdWithoutWhere,
    #[error("fonction refusée en production : {0}")]
    ProdFunction(String),
    #[error("une modification de production attend d'être validée ou annulée")]
    ProdWritePending,
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
    /// Secrets en clair écartés à la relecture : la connexion le dit au lieu d'échouer sans eux.
    refused: Vec<Part>,
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
        if let Some(part) = Self::forbidden_literals(engine, stage, &fields).next() {
            return Err(DbError::SecretInClear(part));
        }
        Ok(Self { id, name: Self::valid_name(name)?, engine, stage, project_path, read_only, fields, refused: vec![] })
    }

    /// Reconstitution depuis le stockage : les invariants ont été vérifiés à l'écriture, sauf ceux qui
    /// protègent, réimposés ici quoi que dise la ligne : la lecture seule de la production, et aucun
    /// secret en clair hors dev local (un tel champ est écarté, et `usable` dit pourquoi).
    pub fn restore(
        id: ConnectionId,
        name: String,
        engine: Engine,
        stage: Stage,
        project_path: Option<String>,
        read_only: bool,
        fields: Vec<(Part, Source)>,
    ) -> Self {
        let refused: Vec<Part> = Self::forbidden_literals(engine, stage, &fields).collect();
        let fields = fields.into_iter().filter(|(part, _)| !refused.contains(part)).collect();
        Self { id, name, engine, stage, project_path, read_only: read_only || stage == Stage::Prod, fields, refused }
    }

    /// Une connexion dont un secret en clair a été écarté ne se connecte pas : sans lui, l'échec
    /// viendrait du serveur, sans dire que la cause est ici.
    pub fn usable(&self) -> Result<(), DbError> {
        if self.refused.is_empty() { Ok(()) } else { Err(DbError::ClearSecretRefused) }
    }

    /// Champs secrets saisis en clair alors que la connexion n'est pas une connexion dev vers la
    /// machine locale. L'hôte doit lui-même être en clair pour en juger : venu de Vault ou du
    /// trousseau, il peut désigner n'importe quelle machine.
    fn forbidden_literals(engine: Engine, stage: Stage, fields: &[(Part, Source)]) -> impl Iterator<Item = Part> + '_ {
        let local = stage == Stage::Dev && literal_host(engine, fields).is_some_and(|h| is_local_host(&h));
        fields
            .iter()
            .filter(move |(part, source)| !local && part.is_secret() && matches!(source, Source::Literal(_)))
            .map(|(part, _)| *part)
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
            Some(url) => parse_url(engine, url).ok_or(DbError::Invalid(Part::Url))?,
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

    /// Chiffrement exigé pour joindre cette cible. Hors dev, toujours : un `sslmode` qui laisserait
    /// un attaquant réseau rabattre la session en clair (`prefer`, `allow`) est refusé, pas
    /// silencieusement durci, pour que la configuration fautive se voie. En dev, le clair n'est
    /// accepté explicitement (`disable`) que vers la machine locale.
    pub fn tls(&self, stage: Stage) -> Result<Tls, DbError> {
        let mode = self.ssl_mode.as_deref();
        if stage != Stage::Dev {
            return match mode {
                None | Some("require" | "verify-ca" | "verify-full") => Ok(Tls::Required),
                Some("disable" | "allow" | "prefer") => Err(DbError::TlsRequired),
                Some(_) => Err(DbError::Invalid(Part::Url)),
            };
        }
        match mode {
            None | Some("prefer" | "allow") => Ok(Tls::Preferred),
            Some("require" | "verify-ca" | "verify-full") => Ok(Tls::Required),
            Some("disable") if is_local_host(&self.host) => Ok(Tls::Disabled),
            Some("disable") => Err(DbError::ClearTextOffLocalhost),
            Some(_) => Err(DbError::Invalid(Part::Url)),
        }
    }
}

/// Chiffrement de la session, décidé par `Target::tls`. Le certificat et le nom d'hôte sont
/// vérifiés dès qu'il y a TLS : l'adapter n'a pas de mode « sans vérification ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    Required,
    /// TLS si le serveur le propose : dev seulement, où un repli en clair est acceptable.
    Preferred,
    /// Dev vers la machine locale seulement.
    Disabled,
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// L'hôte visé, s'il est connu sans rien résoudre : champ `Host` en clair, ou à défaut celui d'une
/// URL en clair. Un champ `Host` présent l'emporte, comme dans `Target::assemble`.
fn literal_host(engine: Engine, fields: &[(Part, Source)]) -> Option<String> {
    let source = |part| fields.iter().find(|(p, _)| *p == part).map(|(_, s)| s);
    match (source(Part::Host), source(Part::Url)) {
        (Some(Source::Literal(host)), _) => Some(host.trim().to_owned()),
        (Some(_), _) => None,
        (None, Some(Source::Literal(url))) => parse_url(engine, url).map(|t| t.host),
        (None, _) => None,
    }
}

/// `postgres[ql]://user:pass@host:port/base?sslmode=…`, `mysql://…` pour MariaDB, `http[s]://…` pour
/// Elasticsearch, identifiants décodés. Pour Elasticsearch, c'est le schéma qui dit le chiffrement,
/// et le chemin ne désigne pas de base.
fn parse_url(engine: Engine, url: &str) -> Option<Target> {
    let url = url.trim();
    let (scheme, rest) = engine.schemes().iter().find_map(|scheme| Some((*scheme, url.strip_prefix(scheme)?)))?;
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
    let ssl_mode = match (engine, scheme) {
        (Engine::Elasticsearch, "http://") => Some("disable".to_owned()),
        (Engine::Elasticsearch, _) => Some("verify-full".to_owned()),
        _ => query.split('&').find_map(|kv| kv.strip_prefix("sslmode=")).map(str::to_owned),
    };
    let database = if engine == Engine::Elasticsearch { "" } else { database };
    Some(Target {
        host: percent_decode(host)?,
        port,
        database: (!database.is_empty()).then(|| percent_decode(database)).flatten(),
        user: user.filter(|u| !u.is_empty()),
        password,
        ssl_mode,
    })
}

pub(crate) fn percent_decode(s: &str) -> Option<String> {
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
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Probe, DbError>;
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

    fn literal(v: &str) -> Source {
        Source::Literal(v.into())
    }

    #[test]
    fn secrets_are_typed_in_clear_only_for_a_local_dev_connection() {
        let password = |host: Source| vec![(Part::Host, host), (Part::Password, literal("x"))];
        for host in ["localhost", "127.0.0.1", "::1"] {
            assert!(create(Stage::Dev, false, password(literal(host))).is_ok(), "{host}");
        }
        assert!(create(Stage::Dev, false, vec![(Part::Url, literal("postgresql://a:b@localhost:8032/db"))]).is_ok());

        let refused = Err(DbError::SecretInClear(Part::Password));
        assert_eq!(create(Stage::Dev, true, password(literal("db.dev.local"))), refused);
        assert_eq!(create(Stage::Staging, true, password(literal("localhost"))), refused);
        assert_eq!(create(Stage::Prod, true, password(literal("localhost"))), refused);
        // Hôte venu de Vault : il peut désigner n'importe quelle machine.
        assert_eq!(create(Stage::Dev, true, password(vault("internal/dev1/DB", "HOST"))), refused);
        // Le champ `Host` l'emporte sur l'hôte local de l'URL.
        let moved = vec![(Part::Url, literal("postgresql://a:b@localhost/db")), (Part::Host, literal("db.dev.local"))];
        assert_eq!(create(Stage::Dev, true, moved), Err(DbError::SecretInClear(Part::Url)));
        let remote_url = vec![(Part::Url, literal("postgresql://a:b@h/db"))];
        assert_eq!(create(Stage::Dev, true, remote_url), Err(DbError::SecretInClear(Part::Url)));

        let keychain = vec![(Part::Host, literal("db")), (Part::Password, Source::Keychain("pg-local".into()))];
        assert_eq!(create(Stage::Prod, true, keychain).unwrap().name(), "Forum");
    }

    #[test]
    fn a_stored_clear_secret_is_dropped_once_the_connection_is_no_longer_local_dev() {
        let restore = |stage, host: &str| {
            let fields = vec![(Part::Host, literal(host)), (Part::Password, literal("x"))];
            Connection::restore(ConnectionId("c".into()), "x".into(), Engine::Postgres, stage, None, true, fields)
        };
        assert_eq!(restore(Stage::Dev, "localhost").fields().len(), 2);
        assert_eq!(restore(Stage::Dev, "localhost").usable(), Ok(()));
        let prod = restore(Stage::Prod, "localhost");
        assert_eq!(prod.fields(), [(Part::Host, literal("localhost"))]);
        assert_eq!(prod.usable(), Err(DbError::ClearSecretRefused));
        let remote = restore(Stage::Dev, "db.dev.local");
        assert_eq!(remote.fields(), [(Part::Host, literal("db.dev.local"))]);
        assert_eq!(remote.usable(), Err(DbError::ClearSecretRefused));
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
    fn an_elasticsearch_url_says_whether_it_is_encrypted() {
        let t = Target::assemble(Engine::Elasticsearch, &[(Part::Url, "https://elastic:p%40ss@es.local/prefixe".into())]).unwrap();
        assert_eq!((t.host.as_str(), t.port, t.database.as_deref()), ("es.local", 9200, None));
        assert_eq!((t.user.as_deref(), t.password.as_deref()), (Some("elastic"), Some("p@ss")));
        assert_eq!(t.tls(Stage::Prod), Ok(Tls::Required));
        let clear = Target::assemble(Engine::Elasticsearch, &[(Part::Url, "http://localhost:9201".into())]).unwrap();
        assert_eq!(clear.tls(Stage::Dev), Ok(Tls::Disabled));
        assert_eq!(clear.tls(Stage::Staging), Err(DbError::TlsRequired));
        let remote = Target::assemble(Engine::Elasticsearch, &[(Part::Url, "http://es.dev.local:9200".into())]).unwrap();
        assert_eq!(remote.tls(Stage::Dev), Err(DbError::ClearTextOffLocalhost));
        assert!(!Engine::Elasticsearch.session_read_only() && Engine::MariaDb.session_read_only());
    }

    #[test]
    fn reads_a_mariadb_url_with_the_mysql_scheme() {
        let url = "mysql://app:p%40ss@127.0.0.1/app?serverVersion=10.11.2-MariaDB&charset=utf8mb4";
        let t = Target::assemble(Engine::MariaDb, &[(Part::Url, url.into())]).unwrap();
        assert_eq!((t.host.as_str(), t.port, t.database.as_deref()), ("127.0.0.1", 3306, Some("app")));
        assert_eq!(t.password.as_deref(), Some("p@ss"));
        assert!(Target::assemble(Engine::MariaDb, &[(Part::Url, "mariadb://h:3307/db".into())]).is_ok());
        assert_eq!(Target::assemble(Engine::MariaDb, &[(Part::Url, "postgres://h/db".into())]), Err(DbError::Invalid(Part::Url)));
        // Le `.env` d'un projet local : mot de passe en clair accepté vers localhost.
        let local = vec![(Part::Url, literal("mysql://app:pw@localhost:3306/app"))];
        let c = Connection::create(ConnectionId("c".into()), "Boutique", Engine::MariaDb, Stage::Dev, None, false, local);
        assert!(c.is_ok());
    }

    #[test]
    fn requires_tls_outside_dev_and_clear_text_only_on_localhost() {
        let target = |host: &str, mode: Option<&str>| Target {
            host: host.into(),
            port: 5432,
            database: None,
            user: None,
            password: None,
            ssl_mode: mode.map(str::to_owned),
        };
        for stage in [Stage::Staging, Stage::Prod] {
            assert_eq!(target("db", None).tls(stage), Ok(Tls::Required));
            assert_eq!(target("db", Some("verify-full")).tls(stage), Ok(Tls::Required));
            for weak in ["disable", "allow", "prefer"] {
                assert_eq!(target("localhost", Some(weak)).tls(stage), Err(DbError::TlsRequired), "{weak}");
            }
        }
        assert_eq!(target("db", None).tls(Stage::Dev), Ok(Tls::Preferred));
        assert_eq!(target("db", Some("require")).tls(Stage::Dev), Ok(Tls::Required));
        assert_eq!(target("127.0.0.1", Some("disable")).tls(Stage::Dev), Ok(Tls::Disabled));
        assert_eq!(target("db.dev.local", Some("disable")).tls(Stage::Dev), Err(DbError::ClearTextOffLocalhost));
        assert_eq!(target("db", Some("on")).tls(Stage::Prod), Err(DbError::Invalid(Part::Url)));
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

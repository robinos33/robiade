//! Sessions Postgres. La lecture seule est posée à l'ouverture de la session, par le paramètre de
//! démarrage `default_transaction_read_only`, avant toute requête.

use std::time::Duration;

use postgres::config::SslMode;
use postgres_native_tls::MakeTlsConnector;
use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};

pub struct Postgres;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Le chiffrement décidé par le domaine. Le connecteur TLS est celui du système, avec ses réglages
/// par défaut : certificat et nom d'hôte vérifiés, aucune exception (`require` vaut `verify-full`).
fn ssl_mode(tls: Tls) -> SslMode {
    match tls {
        Tls::Required => SslMode::Require,
        Tls::Preferred => SslMode::Prefer,
        Tls::Disabled => SslMode::Disable,
    }
}

/// Paramètres de session envoyés au démarrage : lecture seule demandée, et une requête qui
/// traînerait est coupée par le serveur.
fn options(read_only: bool) -> String {
    let mut options = format!("-c statement_timeout={}", TIMEOUT.as_millis());
    if read_only {
        options.push_str(" -c default_transaction_read_only=on");
    }
    options
}

fn unreachable(e: postgres::Error) -> DbError {
    // `as_db_error` porte le message du serveur, plus parlant que l'enveloppe du client.
    let message = e.as_db_error().map_or_else(|| e.to_string(), |db| db.message().to_owned());
    DbError::Unreachable(message)
}

impl DatabaseProbe for Postgres {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Probe, DbError> {
        if engine != Engine::Postgres {
            return Err(DbError::Unsupported("ce moteur"));
        }
        let mut config = postgres::Config::new();
        config
            .host(&target.host)
            .port(target.port)
            .ssl_mode(ssl_mode(tls))
            .connect_timeout(TIMEOUT)
            .application_name("robiade")
            .options(&options(read_only));
        if let Some(database) = &target.database {
            config.dbname(database);
        }
        if let Some(user) = &target.user {
            config.user(user);
        }
        if let Some(password) = &target.password {
            config.password(password);
        }
        let tls = native_tls::TlsConnector::new().map_err(|e| DbError::Unreachable(e.to_string()))?;
        let mut client = config.connect(MakeTlsConnector::new(tls)).map_err(unreachable)?;
        let row = client
            .query_one(
                "SELECT current_setting('server_version'), current_user::text, current_database()::text,
                        current_setting('transaction_read_only')",
                &[],
            )
            .map_err(unreachable)?;
        let probe = Probe {
            server_version: row.get(0),
            user: row.get(1),
            database: row.get(2),
            read_only: row.get::<_, String>(3) == "on",
        };
        let _ = client.close();
        Ok(probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_tls_never_falls_back_to_clear_text() {
        assert!(matches!(ssl_mode(Tls::Required), SslMode::Require));
        assert!(matches!(ssl_mode(Tls::Preferred), SslMode::Prefer));
        assert!(matches!(ssl_mode(Tls::Disabled), SslMode::Disable));
    }

    #[test]
    fn asks_for_read_only_at_session_start() {
        assert_eq!(options(true), "-c statement_timeout=10000 -c default_transaction_read_only=on");
        assert!(!options(false).contains("read_only"));
    }
}

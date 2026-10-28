//! Aiguillage vers l'adapter du moteur d'une connexion.

use robiade_core::databases::{DatabaseProbe, DbError, Engine, Probe, Target, Tls};
use robiade_core::db_browse::{DatabaseBrowser, DbSession};

use crate::elasticsearch::Elasticsearch;
use crate::mariadb::MariaDb;
use crate::postgres::Postgres;

pub struct Engines;

impl DatabaseProbe for Engines {
    fn probe(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Probe, DbError> {
        match engine {
            Engine::Postgres => Postgres.probe(engine, target, tls, read_only),
            Engine::MariaDb => MariaDb.probe(engine, target, tls, read_only),
            Engine::Elasticsearch => Elasticsearch.probe(engine, target, tls, read_only),
            Engine::Mongo => Err(DbError::Unsupported("ce moteur")),
        }
    }
}

impl DatabaseBrowser for Engines {
    fn open(&self, engine: Engine, target: &Target, tls: Tls, read_only: bool) -> Result<Box<dyn DbSession>, DbError> {
        match engine {
            Engine::Postgres => Postgres.open(engine, target, tls, read_only),
            Engine::MariaDb => MariaDb.open(engine, target, tls, read_only),
            Engine::Elasticsearch => Elasticsearch.open(engine, target, tls, read_only),
            Engine::Mongo => Err(DbError::Unsupported("ce moteur")),
        }
    }
}

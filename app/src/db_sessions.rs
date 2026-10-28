//! Sessions ouvertes par l'interface, une par connexion, pour le parcours comme pour la console.
//! Ouvrir une autre base de la même connexion remplace la session ; la quitter la ferme.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use robiade_core::db_browse::{Canceller, DbSession};

/// Une session et ce qu'il faut savoir d'elle sans la verrouiller : une requête longue la tient
/// occupée, mais doit rester annulable.
pub struct OpenSession {
    session: Mutex<Box<dyn DbSession>>,
    cancel: Canceller,
    /// Lecture seule exigée par la connexion et vérifiée sur le serveur à l'ouverture.
    pub read_only: bool,
    /// `user@hôte:port/base`, montré avant une écriture.
    pub target: String,
}

impl OpenSession {
    pub fn new(session: Box<dyn DbSession>, read_only: bool, target: String) -> Self {
        Self { cancel: session.canceller(), session: Mutex::new(session), read_only, target }
    }

    pub fn lock(&self) -> MutexGuard<'_, Box<dyn DbSession>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn cancel(&self) -> Result<(), String> {
        (self.cancel)().map_err(|e| e.to_string())
    }
}

type Shared = Arc<OpenSession>;

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Shared>,
    /// Incrémentée à chaque fermeture : une ouverture commencée avant ne peut plus s'installer.
    generations: HashMap<String, u64>,
}

#[derive(Default)]
pub struct DbSessions(Mutex<Inner>);

impl DbSessions {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// À relever avant de résoudre la cible d'une connexion, puis à rendre à `insert`.
    pub fn generation(&self, connection: &str) -> u64 {
        self.inner().generations.get(connection).copied().unwrap_or(0)
    }

    /// Installe la session si la connexion n'a été ni modifiée ni supprimée depuis `generation` :
    /// sinon la session viserait l'ancienne cible, avec l'ancienne règle de lecture seule.
    /// La session précédente de cette connexion, s'il y en a une, est fermée en étant lâchée.
    pub fn insert(&self, connection: &str, generation: u64, session: OpenSession) -> Result<(), String> {
        let mut inner = self.inner();
        if inner.generations.get(connection).copied().unwrap_or(0) != generation {
            return Err("la connexion a changé pendant l'ouverture : rouvre-la".into());
        }
        inner.sessions.insert(connection.to_owned(), Arc::new(session));
        Ok(())
    }

    /// La session d'une connexion. Le verrou global n'est tenu que le temps de la trouver : une
    /// requête lente sur une base ne bloque pas les autres.
    pub fn get(&self, connection: &str) -> Result<Shared, String> {
        self.inner().sessions.get(connection).cloned().ok_or_else(|| "connexion fermée : rouvre-la".into())
    }

    pub fn close(&self, connection: &str) {
        let mut inner = self.inner();
        inner.sessions.remove(connection);
        *inner.generations.entry(connection.to_owned()).or_default() += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::databases::DbError;
    use robiade_core::db_browse::{Canceller, PageQuery, Relation, TableRef, TableShape};
    use robiade_core::db_console::{Executed, StatementKind};

    struct Idle;

    impl DbSession for Idle {
        fn read_only(&mut self) -> Result<bool, DbError> {
            Ok(true)
        }
        fn current_database(&mut self) -> Result<String, DbError> {
            Ok("db".into())
        }
        fn databases(&mut self) -> Result<Vec<String>, DbError> {
            Ok(vec![])
        }
        fn relations(&mut self) -> Result<Vec<Relation>, DbError> {
            Ok(vec![])
        }
        fn describe(&mut self, _: &TableRef) -> Result<TableShape, DbError> {
            Err(DbError::NotFound)
        }
        fn fetch(&mut self, _: &PageQuery) -> Result<Vec<Vec<Option<String>>>, DbError> {
            Ok(vec![])
        }
        fn execute(&mut self, _: &str, _: StatementKind, _: usize) -> Result<Executed, DbError> {
            Ok(Executed::default())
        }
        fn canceller(&self) -> Canceller {
            Box::new(|| Ok(()))
        }
        fn begin_write(&mut self, _: &str) -> Result<u64, DbError> {
            Ok(0)
        }
        fn finish_write(&mut self, _: bool) -> Result<(), DbError> {
            Ok(())
        }
    }

    fn idle() -> OpenSession {
        OpenSession::new(Box::new(Idle), true, "u@h:5432/db".into())
    }

    #[test]
    fn a_closed_connection_can_no_longer_be_queried() {
        let sessions = DbSessions::default();
        sessions.insert("c", sessions.generation("c"), idle()).unwrap();
        assert!(sessions.get("c").is_ok());
        sessions.close("c");
        assert!(sessions.get("c").is_err());
        // Fermer une connexion jamais ouverte ne fait rien.
        sessions.close("absente");
    }

    #[test]
    fn an_opening_overtaken_by_an_edit_does_not_install_its_session() {
        let sessions = DbSessions::default();
        let seen = sessions.generation("c");
        // La connexion est modifiée pendant que l'ouverture résout l'ancienne cible.
        sessions.close("c");
        assert!(sessions.insert("c", seen, idle()).is_err());
        assert!(sessions.get("c").is_err());
        // Une ouverture commencée après la modification passe.
        sessions.insert("c", sessions.generation("c"), idle()).unwrap();
        assert!(sessions.get("c").is_ok());
    }
}

//! Sessions de parcours ouvertes par l'interface, une par connexion. Ouvrir une autre base de la
//! même connexion remplace la session ; la quitter la ferme.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use robiade_core::db_browse::DbSession;

type Shared = Arc<Mutex<Box<dyn DbSession>>>;

#[derive(Default)]
pub struct DbSessions(Mutex<HashMap<String, Shared>>);

impl DbSessions {
    /// La session précédente de cette connexion, s'il y en a une, est fermée en étant lâchée.
    pub fn insert(&self, connection: &str, session: Box<dyn DbSession>) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(connection.to_owned(), Arc::new(Mutex::new(session)));
    }

    /// La session d'une connexion. Le verrou global n'est tenu que le temps de la trouver : une
    /// requête lente sur une base ne bloque pas les autres.
    pub fn get(&self, connection: &str) -> Result<Shared, String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(connection)
            .cloned()
            .ok_or_else(|| "connexion fermée : rouvre-la".into())
    }

    pub fn close(&self, connection: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(connection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::databases::DbError;
    use robiade_core::db_browse::{PageQuery, Relation, TableRef, TableShape};

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
    }

    #[test]
    fn a_closed_connection_can_no_longer_be_queried() {
        let sessions = DbSessions::default();
        sessions.insert("c", Box::new(Idle));
        assert!(sessions.get("c").is_ok());
        sessions.close("c");
        assert!(sessions.get("c").is_err());
        // Fermer une connexion jamais ouverte ne fait rien.
        sessions.close("absente");
    }
}

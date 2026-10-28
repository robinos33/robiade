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

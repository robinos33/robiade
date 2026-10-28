//! Sessions ouvertes par l'interface, une par connexion, pour le parcours comme pour la console.
//! Ouvrir une autre base de la même connexion remplace la session ; la quitter la ferme.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use robiade_core::databases::{DbError, Dialect, Language};
use robiade_core::db_browse::{Canceller, DbSession};
use robiade_core::db_console::Policy;

/// Une modification de prod exécutée dans sa transaction, ni validée ni annulée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWrite {
    token: u64,
    pub statement: String,
    pub affected: u64,
}

/// Issue d'une modification en attente.
#[derive(Debug)]
pub struct Finished {
    pub write: PendingWrite,
    pub committed: bool,
    pub outcome: Result<(), String>,
    /// Relu sur le serveur après coup : la session doit être revenue en lecture seule.
    pub read_only: bool,
}

static TOKENS: AtomicU64 = AtomicU64::new(1);

/// Une session et ce qu'il faut savoir d'elle sans la verrouiller : une requête longue la tient
/// occupée, mais doit rester annulable.
///
/// Verrous toujours pris dans le même ordre : `pending`, puis `session`.
pub struct OpenSession {
    session: Mutex<Box<dyn DbSession>>,
    pending: Mutex<Option<PendingWrite>>,
    cancel: Canceller,
    pub policy: Policy,
    pub language: Language,
    /// Lecture seule exigée par la connexion et vérifiée sur le serveur à l'ouverture.
    pub read_only: bool,
    /// Nom de la connexion, puis `user@hôte:port/base` : ce que montrent la confirmation et le journal.
    pub connection: String,
    pub target: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl OpenSession {
    pub fn new(
        session: Box<dyn DbSession>,
        policy: Policy,
        language: Language,
        read_only: bool,
        connection: String,
        target: String,
    ) -> Self {
        Self {
            cancel: session.canceller(),
            session: Mutex::new(session),
            pending: Mutex::new(None),
            policy,
            language,
            read_only,
            connection,
            target,
        }
    }

    /// Une requête ordinaire, refusée tant qu'une modification attend : elle se lirait dans la
    /// transaction ouverte, et pourrait la prolonger.
    pub fn run<T>(&self, f: impl FnOnce(&mut Box<dyn DbSession>) -> T) -> Result<T, String> {
        let pending = lock(&self.pending);
        if pending.is_some() {
            return Err(DbError::ProdWritePending.to_string());
        }
        Ok(f(&mut lock(&self.session)))
    }

    /// Ce que le journal d'audit retient comme cible : connexion, puis `user@hôte:port/base`.
    pub fn audit_target(&self) -> String {
        format!("{} · {}", self.connection, self.target)
    }

    pub fn cancel(&self) -> Result<(), String> {
        (self.cancel)().map_err(|e| e.to_string())
    }

    pub fn pending(&self) -> Option<PendingWrite> {
        lock(&self.pending).clone()
    }

    /// Exécute une modification de prod dans sa transaction et la laisse en attente. Sans
    /// réponse au bout de `delay`, elle est annulée et `on_expired` reçoit l'issue.
    pub fn begin_write(
        self: &Arc<Self>,
        statement: &str,
        delay: Duration,
        on_expired: impl FnOnce(Finished) + Send + 'static,
    ) -> Result<PendingWrite, String> {
        let mut pending = lock(&self.pending);
        if pending.is_some() {
            return Err(DbError::ProdWritePending.to_string());
        }
        let affected = lock(&self.session).begin_write(statement).map_err(|e| e.to_string())?;
        let write = PendingWrite { token: TOKENS.fetch_add(1, Ordering::Relaxed), statement: statement.to_owned(), affected };
        *pending = Some(write.clone());
        drop(pending);
        let me = Arc::clone(self);
        let token = write.token;
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if let Some(finished) = me.finish_if(Some(token), false) {
                on_expired(finished);
            }
        });
        Ok(write)
    }

    /// Valide ou annule la modification en attente.
    pub fn finish(&self, commit: bool) -> Result<Finished, String> {
        self.finish_if(None, commit).ok_or_else(|| "aucune modification en attente : elle a pu être annulée au délai".into())
    }

    /// Annule ce qui attendait, avant de lâcher la session.
    pub fn abandon(&self) -> Option<Finished> {
        self.finish_if(None, false)
    }

    /// `token` : seulement si c'est encore cette modification-là qui attend.
    fn finish_if(&self, token: Option<u64>, commit: bool) -> Option<Finished> {
        let mut pending = lock(&self.pending);
        if token.is_some_and(|t| pending.as_ref().map(|w| w.token) != Some(t)) {
            return None;
        }
        let write = pending.take()?;
        let mut session = lock(&self.session);
        let outcome = session.finish_write(commit).map_err(|e| e.to_string());
        let read_only = session.read_only().unwrap_or(false);
        Some(Finished { write, committed: commit && outcome.is_ok(), outcome, read_only })
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

    /// Ferme la session ; une modification qui attendait est annulée d'abord, et rendue pour le journal.
    pub fn close(&self, connection: &str) -> Option<(Shared, Finished)> {
        let removed = {
            let mut inner = self.inner();
            *inner.generations.entry(connection.to_owned()).or_default() += 1;
            inner.sessions.remove(connection)
        };
        let session = removed?;
        let finished = session.abandon()?;
        Some((session, finished))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::db_browse::{PageQuery, Relation, TableRef, TableShape};
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
        OpenSession::new(Box::new(Idle), Policy::Prod, Dialect::Postgres.into(), true, "Boutique prod".into(), "u@h:5432/db".into())
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

    /// Note les fins de transaction, pour vérifier ce que la session a reçu.
    struct Recorder(Arc<Mutex<Vec<&'static str>>>);

    impl DbSession for Recorder {
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
            self.0.lock().unwrap().push("begin");
            Ok(3)
        }
        fn finish_write(&mut self, commit: bool) -> Result<(), DbError> {
            self.0.lock().unwrap().push(if commit { "commit" } else { "rollback" });
            Ok(())
        }
    }

    fn recorded() -> (Arc<OpenSession>, Arc<Mutex<Vec<&'static str>>>) {
        let log = Arc::new(Mutex::new(vec![]));
        let session = OpenSession::new(Box::new(Recorder(log.clone())), Policy::Prod, Dialect::Postgres.into(), true, "Boutique prod".into(), "u@h/db".into());
        (Arc::new(session), log)
    }

    #[test]
    fn a_pending_write_blocks_the_session_until_it_is_settled() {
        let (session, log) = recorded();
        let write = session.begin_write("DELETE FROM t WHERE id = 1", Duration::from_secs(60), |_| {}).unwrap();
        assert_eq!(write.affected, 3);
        assert!(session.run(|_| ()).is_err());
        assert!(session.begin_write("DELETE FROM t WHERE id = 2", Duration::from_secs(60), |_| {}).is_err());
        let finished = session.finish(true).unwrap();
        assert!(finished.committed && finished.read_only);
        assert!(session.run(|_| ()).is_ok());
        assert!(session.finish(true).is_err(), "plus rien n'attend");
        assert_eq!(*log.lock().unwrap(), ["begin", "commit"]);
    }

    #[test]
    fn an_unanswered_write_is_rolled_back_after_the_delay() {
        let (session, log) = recorded();
        let (tx, rx) = std::sync::mpsc::channel();
        session.begin_write("UPDATE t SET a = 1 WHERE id = 1", Duration::from_millis(50), move |f| tx.send(f.committed).unwrap()).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(false));
        assert!(session.pending().is_none());
        assert_eq!(*log.lock().unwrap(), ["begin", "rollback"]);
    }

    #[test]
    fn a_late_expiry_does_not_touch_the_next_write() {
        let (session, log) = recorded();
        session.begin_write("UPDATE t SET a = 1 WHERE id = 1", Duration::from_millis(100), |_| panic!("déjà réglée")).unwrap();
        session.finish(false).unwrap();
        session.begin_write("UPDATE t SET a = 2 WHERE id = 1", Duration::from_secs(60), |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(session.pending().is_some(), "le délai de la première ne vise pas la seconde");
        assert_eq!(*log.lock().unwrap(), ["begin", "rollback", "begin"]);
    }

    #[test]
    fn closing_the_session_rolls_back_what_was_pending() {
        let sessions = DbSessions::default();
        let (session, log) = recorded();
        session.begin_write("DELETE FROM t WHERE id = 1", Duration::from_secs(60), |_| {}).unwrap();
        sessions.0.lock().unwrap().sessions.insert("c".into(), session);
        let (_, finished) = sessions.close("c").unwrap();
        assert!(!finished.committed);
        assert_eq!(*log.lock().unwrap(), ["begin", "rollback"]);
    }

    /// Contre le Postgres jetable des tests de `robiade-adapters` (`ROBIADE_PG_TEST_PORT`,
    /// `ROBIADE_PG_TEST_PASSWORD`) : une modification de prod oubliée est annulée au délai.
    #[test]
    #[ignore = "demande un Postgres local jetable"]
    fn a_forgotten_production_write_is_rolled_back_on_a_real_server() {
        use robiade_adapters::postgres::Postgres;
        use robiade_core::databases::{Engine, Target, Tls};
        use robiade_core::db_browse::DatabaseBrowser;

        let target = Target {
            host: "127.0.0.1".into(),
            port: std::env::var("ROBIADE_PG_TEST_PORT").unwrap().parse().unwrap(),
            database: None,
            user: Some("postgres".into()),
            password: Some(std::env::var("ROBIADE_PG_TEST_PASSWORD").unwrap()),
            ssl_mode: None,
        };
        let mut setup = Postgres.open(Engine::Postgres, &target, Tls::Disabled, false).unwrap();
        setup.execute("CREATE TABLE IF NOT EXISTS robiade_expiry (a int)", StatementKind::Write, 1).unwrap();
        setup.execute("TRUNCATE robiade_expiry", StatementKind::Write, 1).unwrap();
        setup.execute("INSERT INTO robiade_expiry VALUES (1)", StatementKind::Write, 1).unwrap();
        let count = |s: &mut Box<dyn DbSession>| {
            s.execute("SELECT count(*) FROM robiade_expiry", StatementKind::Query, 1).unwrap().rows[0][0].clone()
        };

        let prod = Postgres.open(Engine::Postgres, &target, Tls::Disabled, true).unwrap();
        let session = Arc::new(OpenSession::new(prod, Policy::Prod, Dialect::Postgres.into(), true, "test".into(), "postgres@127.0.0.1".into()));
        let (tx, rx) = std::sync::mpsc::channel();
        let write = session
            .begin_write("DELETE FROM robiade_expiry WHERE a = 1", Duration::from_millis(300), move |f| tx.send(f).unwrap())
            .unwrap();
        assert_eq!(write.affected, 1);
        let finished = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!finished.committed && finished.outcome.is_ok() && finished.read_only);
        assert_eq!(count(&mut setup).as_deref(), Some("1"), "la suppression a été annulée");
        assert!(session.run(|s| s.read_only().unwrap()).unwrap(), "la session est revenue en lecture seule");
        setup.execute("DROP TABLE robiade_expiry", StatementKind::Write, 1).unwrap();
    }
}

//! Mise à jour de l'application installée depuis `origin/main`, vérifiée au lancement.
//! L'app connaît le commit dont elle est issue ; en retard, elle construit et installe le commit
//! visé, puis propose de relancer. Rien n'est construit si elle est à jour.

/// Commit visé et nombre de ses commits absents de l'app installée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behind {
    pub target: String,
    pub commits: u32,
}

/// Le dépôt d'où l'app est construite.
pub trait AppSource: Send + Sync {
    /// Compare `installed` au commit visé. L'erreur porte le message de git.
    fn behind(&self, installed: &str) -> Result<Behind, String>;
    /// Construit `commit` hors de la copie de travail et remplace l'app installée.
    /// L'erreur porte la sortie du build.
    fn install(&self, commit: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    /// Construction en cours du commit visé, en avance de `commits` sur l'app qui tourne.
    Installing { commits: u32 },
    /// Nouvelle version installée : elle sera prise à la relance.
    Ready { commits: u32 },
    Failed { output: String },
}

/// SHA-1 ou SHA-256 complet : la seule forme acceptée d'un commit passé à git.
pub fn is_commit(raw: &str) -> bool {
    matches!(raw.len(), 40 | 64) && raw.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Compare l'app installée au commit visé, et l'installe si elle est en retard. Chaque étape est
/// annoncée à `report` : la construction dure plusieurs minutes.
pub fn update(source: &dyn AppSource, installed: &str, report: &mut dyn FnMut(UpdateStatus)) {
    if !is_commit(installed) {
        return report(UpdateStatus::Failed { output: format!("commit installé illisible : {installed:?}") });
    }
    let behind = match source.behind(installed) {
        Ok(behind) => behind,
        Err(output) => return report(UpdateStatus::Failed { output }),
    };
    if behind.commits == 0 {
        return report(UpdateStatus::UpToDate);
    }
    if !is_commit(&behind.target) {
        return report(UpdateStatus::Failed { output: format!("commit visé illisible : {:?}", behind.target) });
    }
    report(UpdateStatus::Installing { commits: behind.commits });
    report(match source.install(&behind.target) {
        Ok(()) => UpdateStatus::Ready { commits: behind.commits },
        Err(output) => UpdateStatus::Failed { output },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const INSTALLED: &str = "1111111111111111111111111111111111111111";
    const TARGET: &str = "2222222222222222222222222222222222222222";

    struct Fake {
        behind: Result<Behind, String>,
        install: Result<(), String>,
        installed: Mutex<Vec<String>>,
    }

    impl Fake {
        fn behind(commits: u32) -> Self {
            Self {
                behind: Ok(Behind { target: TARGET.into(), commits }),
                install: Ok(()),
                installed: Mutex::default(),
            }
        }
    }

    impl AppSource for Fake {
        fn behind(&self, installed: &str) -> Result<Behind, String> {
            assert_eq!(installed, INSTALLED);
            self.behind.clone()
        }
        fn install(&self, commit: &str) -> Result<(), String> {
            self.installed.lock().unwrap().push(commit.to_owned());
            self.install.clone()
        }
    }

    fn run(source: &Fake, installed: &str) -> Vec<UpdateStatus> {
        let mut seen = Vec::new();
        update(source, installed, &mut |s| seen.push(s));
        seen
    }

    #[test]
    fn an_app_up_to_date_builds_nothing() {
        let source = Fake::behind(0);
        assert_eq!(run(&source, INSTALLED), [UpdateStatus::UpToDate]);
        assert!(source.installed.lock().unwrap().is_empty());
    }

    #[test]
    fn an_app_behind_installs_the_target_then_offers_to_relaunch() {
        let source = Fake::behind(3);
        let seen = run(&source, INSTALLED);
        assert_eq!(seen, [UpdateStatus::Installing { commits: 3 }, UpdateStatus::Ready { commits: 3 }]);
        assert_eq!(*source.installed.lock().unwrap(), [TARGET]);
    }

    #[test]
    fn a_failed_build_is_reported_with_its_output() {
        let source = Fake { install: Err("error[E0425]".into()), ..Fake::behind(1) };
        let seen = run(&source, INSTALLED);
        assert_eq!(seen[1], UpdateStatus::Failed { output: "error[E0425]".into() });
    }

    #[test]
    fn an_unknown_installed_commit_is_reported_without_building() {
        let source = Fake { behind: Err("bad revision".into()), ..Fake::behind(0) };
        assert_eq!(run(&source, INSTALLED), [UpdateStatus::Failed { output: "bad revision".into() }]);
        assert!(source.installed.lock().unwrap().is_empty());
    }

    #[test]
    fn only_a_full_commit_reaches_git() {
        assert!(is_commit(INSTALLED));
        assert!(is_commit(&"a".repeat(64)));
        assert!(!is_commit("b71236c"));
        assert!(!is_commit(&format!("--upload-pack={}", "a".repeat(26))));
        let source = Fake::behind(1);
        assert!(matches!(run(&source, "--all")[..], [UpdateStatus::Failed { .. }]));
        let source = Fake { behind: Ok(Behind { target: "-x".into(), commits: 1 }), ..Fake::behind(1) };
        assert!(matches!(run(&source, INSTALLED)[..], [UpdateStatus::Failed { .. }]));
        assert!(source.installed.lock().unwrap().is_empty());
    }
}

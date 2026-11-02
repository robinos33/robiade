//! Dépôt source de Robiade : comparaison à `origin/main` et construction dans un worktree réservé.
//!
//! Pas de `fetch` : je pousse depuis ce dépôt ou l'un de ses worktrees, qui partagent ses refs.
//! La construction ne touche jamais la copie de travail : elle a lieu dans un worktree en tête
//! détachée, gardé d'une fois sur l'autre pour que `target/` et `node_modules` restent chauds.
//! C'est le `scripts/install.sh` du commit construit qui tourne, et il installe l'app.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use robiade_core::updates::{AppSource, Behind};

/// Branche dont l'app suit les commits.
const TARGET: &str = "origin/main";
/// Lignes de la sortie du build gardées pour l'interface ; le journal complet reste sur le disque.
const TAIL_LINES: usize = 80;

pub struct SourceRepo {
    git: String,
    /// `.git` commun du dépôt.
    repo: PathBuf,
    /// Worktree où la construction a lieu.
    worktree: PathBuf,
    /// Sortie complète du dernier build.
    log: PathBuf,
    /// PATH du shell de login : `npm` et `cargo` n'en ont pas d'autre depuis le Finder.
    path: String,
}

impl SourceRepo {
    /// `workspace` accueille le worktree de build et le journal du dernier build.
    pub fn new(repo: PathBuf, workspace: &Path, path: String) -> Self {
        Self {
            git: "/usr/bin/git".to_owned(),
            repo,
            worktree: workspace.join("update"),
            log: workspace.join("update.log"),
            path,
        }
    }

    fn git(&self) -> Command {
        let mut cmd = Command::new(&self.git);
        cmd.arg("--git-dir")
            .arg(&self.repo)
            .args(["-c", "core.hooksPath=/dev/null"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("PATH", &self.path)
            .stdin(Stdio::null());
        cmd
    }

    fn in_worktree(&self) -> Command {
        let mut cmd = Command::new(&self.git);
        cmd.arg("-C")
            .arg(&self.worktree)
            .args(["-c", "core.hooksPath=/dev/null"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("PATH", &self.path)
            .stdin(Stdio::null());
        cmd
    }

    /// Met le worktree de build sur `commit`, en le créant au besoin. Un worktree dont le dossier
    /// a disparu est encore connu de git : `--force` le recrée au même endroit.
    fn checkout(&self, commit: &str) -> Result<(), String> {
        if self.worktree.join(".git").exists() {
            run(self.in_worktree().args(["checkout", "--quiet", "--detach", "--force", commit]))?;
            // Fichiers non suivis laissés par un build : ignorés (`target/`, `node_modules`) gardés.
            return run(self.in_worktree().args(["clean", "--quiet", "-fd"]));
        }
        if let Some(parent) = self.worktree.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        run(self.git().args(["worktree", "add", "--quiet", "--force", "--detach"]).arg(&self.worktree).arg(commit))
    }

    /// Lance `scripts/install.sh` du worktree ; sa sortie va au journal.
    fn build(&self) -> Result<(), String> {
        let log = File::create(&self.log).map_err(|e| format!("journal {} : {e}", self.log.display()))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let status = Command::new("/bin/sh")
            .arg(self.worktree.join("scripts/install.sh"))
            .current_dir(&self.worktree)
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            .status()
            .map_err(|e| format!("install.sh : {e}"))?;
        if status.success() {
            return Ok(());
        }
        let output = std::fs::read_to_string(&self.log).unwrap_or_default();
        Err(format!("{}\n\nJournal complet : {}", tail(&output, TAIL_LINES), self.log.display()))
    }
}

impl AppSource for SourceRepo {
    fn behind(&self, installed: &str) -> Result<Behind, String> {
        let target = run_out(self.git().args(["rev-parse", "--verify", "--end-of-options", &format!("{TARGET}^{{commit}}")]))?;
        let count = run_out(self.git().args(["rev-list", "--count", &format!("{installed}..{target}")]))?;
        let commits = count.parse().map_err(|_| format!("git rev-list --count : {count:?}"))?;
        Ok(Behind { target, commits })
    }

    fn install(&self, commit: &str) -> Result<(), String> {
        self.checkout(commit)?;
        self.build()
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    run_out(cmd).map(drop)
}

/// Sortie standard d'une commande git réussie ; sinon, son erreur.
fn run_out(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("git : {e}"))?;
    if !out.status.success() {
        return Err(format!("git : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} : {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn commit(dir: &Path, script: &str) -> String {
        std::fs::write(dir.join("scripts/install.sh"), script).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "c"]);
        git(dir, &["rev-parse", "HEAD"])
    }

    #[test]
    fn builds_origin_main_in_its_own_worktree_and_reports_a_failure_with_its_output() {
        let root = std::env::temp_dir().join(format!("robiade-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (remote, clone, cache) = (root.join("remote.git"), root.join("clone"), root.join("cache"));
        std::fs::create_dir_all(clone.join("scripts")).unwrap();
        git(&root, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        git(&clone, &["init", "-q", "-b", "main"]);
        git(&clone, &["remote", "add", "origin", remote.to_str().unwrap()]);
        // Le script témoigne du dossier où il tourne : jamais la copie de travail.
        let installed = commit(&clone, "pwd > ../built-in\n");
        git(&clone, &["push", "-q", "origin", "main"]);
        let source = SourceRepo::new(clone.join(".git"), &cache, std::env::var("PATH").unwrap());

        assert_eq!(source.behind(&installed).unwrap(), Behind { target: installed.clone(), commits: 0 });

        commit(&clone, "echo un\n");
        let target = commit(&clone, "pwd > ../built-in\n");
        // Modification locale non poussée : elle n'entre pas dans le build.
        std::fs::write(clone.join("scripts/install.sh"), "exit 1\n").unwrap();
        git(&clone, &["push", "-q", "origin", "main"]);
        assert_eq!(source.behind(&installed).unwrap(), Behind { target: target.clone(), commits: 2 });

        source.install(&target).unwrap();
        let built_in = std::fs::read_to_string(cache.join("built-in")).unwrap();
        assert_eq!(Path::new(built_in.trim()).canonicalize().unwrap(), cache.join("update").canonicalize().unwrap());
        assert_eq!(git(&cache.join("update"), &["rev-parse", "HEAD"]), target);

        // Un build qui échoue remonte sa sortie ; le worktree existant est réutilisé.
        let broken = commit(&clone, "echo 'error[E0425]: introuvable' >&2\nexit 101\n");
        let failure = source.install(&broken).unwrap_err();
        assert!(failure.starts_with("error[E0425]: introuvable"), "{failure}");
        assert!(failure.contains("update.log"));

        // Dossier supprimé à la main : git le connaît encore, il est recréé.
        std::fs::remove_dir_all(cache.join("update")).unwrap();
        source.install(&target).unwrap();

        assert!(source.behind("0".repeat(40).as_str()).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn keeps_the_end_of_the_output() {
        assert_eq!(tail("a\nb\nc\n", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
    }
}

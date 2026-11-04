//! Dépôt source de Robiade : comparaison à `origin/main` et construction dans un worktree réservé.
//!
//! Un `fetch` précède la comparaison : un commit poussé d'ailleurs (la forge, une autre machine)
//! compte aussi. Hors ligne, la ref locale sert ; mes push, faits depuis ce dépôt ou l'un de ses
//! worktrees, l'ont déjà mise à jour.
//! La construction ne touche jamais la copie de travail : elle a lieu dans un worktree en tête
//! détachée, gardé d'une fois sur l'autre pour que `target/` et `node_modules` restent chauds.
//! C'est le `scripts/install.sh` du commit construit qui tourne, et il installe l'app.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use robiade_core::updates::{AppSource, Behind};

/// Branche dont l'app suit les commits.
const TARGET: &str = "origin/main";
/// Un fetch plus long est abandonné : la ref locale sert.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
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
    /// Verrou d'un build en cours, quelle que soit l'instance de Robiade qui l'a lancé.
    lock: PathBuf,
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
            lock: workspace.join("update.lock"),
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

    /// `git fetch origin main`, abandonné au-delà de `FETCH_TIMEOUT`. Un échec n'est pas une
    /// erreur : la comparaison se fait sur ce que le dépôt connaît déjà.
    fn fetch(&self) {
        let branch = TARGET.trim_start_matches("origin/");
        let spawned = self
            .git()
            .args(["fetch", "--quiet", "--no-tags", "origin", branch])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let Ok(mut child) = spawned else { return };
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return,
                Ok(Some(_)) => {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
                    }
                    return eprintln!("fetch de {TARGET} impossible, ref locale gardée : {}", stderr.trim());
                }
                Ok(None) if started.elapsed() < FETCH_TIMEOUT => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return eprintln!("fetch de {TARGET} abandonné, ref locale gardée");
                }
            }
        }
    }

    /// Verrou exclusif, sans attente : un build lancé par une instance précédente, quittée depuis,
    /// peut tourner encore — l'extraction d'un autre commit ou un second `npm install` le casseraient.
    fn lock(&self) -> Result<File, String> {
        if let Some(parent) = self.lock.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.lock)
            .map_err(|e| format!("verrou {} : {e}", self.lock.display()))?;
        // SAFETY: descripteur ouvert, possédé par `file` pour toute la durée de l'appel.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Un build lancé par une instance précédente de Robiade tourne encore : \
                        il installera la mise à jour. Relancer une fois qu'il aura fini."
                .into());
        }
        Ok(file)
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
    ///
    /// Le verrou devient l'entrée standard du build : le `flock` appartient à la description de
    /// fichier, que le script et ses enfants partagent. Il tient donc tant que le build tourne,
    /// même si Robiade est quitté entre-temps.
    fn build(&self, lock: File) -> Result<(), String> {
        let log = File::create(&self.log).map_err(|e| format!("journal {} : {e}", self.log.display()))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let status = Command::new("/bin/sh")
            .arg(self.worktree.join("scripts/install.sh"))
            .current_dir(&self.worktree)
            .env("PATH", &self.path)
            .stdin(lock)
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
        self.fetch();
        let target = run_out(self.git().args(["rev-parse", "--verify", "--end-of-options", &format!("{TARGET}^{{commit}}")]))?;
        let count = run_out(self.git().args(["rev-list", "--count", &format!("{installed}..{target}")]))?;
        let commits = count.parse().map_err(|_| format!("git rev-list --count : {count:?}"))?;
        Ok(Behind { target, commits })
    }

    fn install(&self, commit: &str) -> Result<(), String> {
        let lock = self.lock()?;
        self.checkout(commit)?;
        self.build(lock)
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

        // Poussé d'ailleurs : seul le fetch le fait connaître du dépôt.
        let elsewhere = root.join("elsewhere");
        git(&root, &["clone", "-q", remote.to_str().unwrap(), elsewhere.to_str().unwrap()]);
        let target = commit(&elsewhere, "pwd > ../built-in # ailleurs\n");
        git(&elsewhere, &["push", "-q", "origin", "main"]);
        assert_eq!(source.behind(&installed).unwrap(), Behind { target: target.clone(), commits: 3 });

        source.install(&target).unwrap();
        let built_in = std::fs::read_to_string(cache.join("built-in")).unwrap();
        assert_eq!(Path::new(built_in.trim()).canonicalize().unwrap(), cache.join("update").canonicalize().unwrap());
        assert_eq!(git(&cache.join("update"), &["rev-parse", "HEAD"]), target);

        // Un build qui échoue remonte sa sortie ; le worktree existant est réutilisé.
        let broken = commit(&clone, "echo 'error[E0425]: introuvable' >&2\nexit 101\n");
        let failure = source.install(&broken).unwrap_err();
        assert!(failure.starts_with("error[E0425]: introuvable"), "{failure}");
        assert!(failure.contains("update.log"));

        // Un build encore en cours, lancé par une instance précédente : rien n'est touché.
        let held = source.lock().unwrap();
        assert!(source.install(&target).unwrap_err().contains("tourne encore"));
        assert_eq!(git(&cache.join("update"), &["rev-parse", "HEAD"]), broken);
        drop(held);

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

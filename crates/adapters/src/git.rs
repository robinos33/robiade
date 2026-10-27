//! Modifications en cours lues avec la CLI `git`, en lecture seule et sans prendre de verrou :
//! l'agent peut écrire dans le même dépôt au même moment.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use robiade_core::vcs::{ChangeKind, FileChange, VcsError, WorkingTree};

/// Arbre vide de Git : base de comparaison d'un dépôt sans commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// Un diff plus long est tronqué : l'aperçu n'est pas un outil de revue de 50 000 lignes.
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// Réglages imposés à chaque appel : la config d'un dépôt peut déclarer des commandes que `status`
/// ou `diff` exécuteraient (`core.fsmonitor`, filtres `clean`/`textconv`, diff externe, sous-modules).
/// Robiade lance git sans que je le demande, sur des dépôts où un agent écrit : aucune ne doit tourner.
/// Les filtres, nommés librement, sont neutralisés un par un (voir `filter_overrides`).
const HARDENING: &[&str] = &["-c", "core.fsmonitor=false", "-c", "core.quotepath=off", "-c", "diff.external="];

/// Options de `git diff` qui n'exécutent rien de déclaré par le dépôt.
const SAFE_DIFF: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all", "-M"];

pub struct GitCli {
    git: String,
}

impl Default for GitCli {
    fn default() -> Self {
        Self { git: "/usr/bin/git".to_owned() }
    }
}

impl GitCli {
    fn command(&self, dir: &str) -> Command {
        let mut cmd = Command::new(&self.git);
        cmd.arg("-C").arg(dir).args(HARDENING).arg("--no-pager").env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C");
        cmd
    }

    /// Vide chaque filtre déclaré dans la config (dépôt, global, système) : un filtre `clean`, comme
    /// celui de git-lfs, tourne à chaque `status` ou `diff`. Lire la config n'exécute rien.
    fn filter_overrides(&self, dir: &str) -> Vec<String> {
        let Ok(out) = self.command(dir).args(["config", "--get-regexp", r"^filter\."]).output() else { return vec![] };
        let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next()?.strip_prefix("filter.")?.rsplit_once('.'))
            .map(|(name, _)| name.to_owned())
            .collect();
        names.sort();
        names.dedup();
        names
            .iter()
            .flat_map(|n| ["clean=", "smudge=", "process=", "required=false"].map(|k| format!("filter.{n}.{k}")))
            .flat_map(|setting| ["-c".to_owned(), setting])
            .collect()
    }

    fn run(&self, dir: &str, args: &[&str]) -> Result<Vec<u8>, VcsError> {
        let out = self
            .command(dir)
            .args(self.filter_overrides(dir))
            .args(args)
            .output()
            .map_err(|e| VcsError::Git(e.to_string()))?;
        if out.status.success() {
            return Ok(out.stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("not a git repository") {
            return Err(VcsError::NotARepository);
        }
        Err(VcsError::Git(stderr.lines().next().unwrap_or_default().to_owned()))
    }

    /// Racine du dépôt : toutes les commandes y tournent, les chemins sont relatifs à elle.
    fn root(&self, dir: &str) -> Result<String, VcsError> {
        Ok(String::from_utf8_lossy(&self.run(dir, &["rev-parse", "--show-toplevel"])?).trim().to_owned())
    }

    /// Le dernier commit, ou l'arbre vide si le dépôt n'en a pas encore.
    fn base(&self, dir: &str) -> &'static str {
        match self.run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]) {
            Ok(_) => "HEAD",
            Err(_) => EMPTY_TREE,
        }
    }
}

/// `git status --porcelain=v1 -z` : `XY chemin`, suivi de l'ancien chemin pour un renommage.
fn parse_status(raw: &[u8]) -> Vec<(String, ChangeKind)> {
    let mut fields = raw.split(|b| *b == 0).filter(|f| !f.is_empty());
    let mut out = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (x, y) = (entry[0], entry[1]);
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        let kind = match (x, y) {
            (b'?', _) => ChangeKind::Untracked,
            (b'R', _) | (_, b'R') => {
                fields.next(); // ancien chemin
                ChangeKind::Renamed
            }
            (b'A', _) => ChangeKind::Added,
            (b'D', _) | (_, b'D') => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        out.push((path, kind));
    }
    out
}

/// `git diff --numstat -z` : `ajouts\tsuppressions\tchemin`, ou `\t` puis ancien et nouveau chemin.
fn parse_numstat(raw: &[u8]) -> HashMap<String, (Option<u32>, Option<u32>)> {
    let mut fields = raw.split(|b| *b == 0);
    let mut out = HashMap::new();
    while let Some(entry) = fields.next() {
        let entry = String::from_utf8_lossy(entry);
        let mut cols = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cols.next(), cols.next(), cols.next()) else { continue };
        let path = if path.is_empty() {
            fields.next(); // ancien chemin
            fields.next().map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default()
        } else {
            path.to_owned()
        };
        out.insert(path, (added.parse().ok(), removed.parse().ok()));
    }
    out
}

fn truncate(mut diff: Vec<u8>) -> String {
    if diff.len() > MAX_DIFF_BYTES {
        diff.truncate(MAX_DIFF_BYTES);
        diff.extend_from_slice("\n… diff tronqué à 2 Mo\n".as_bytes());
    }
    String::from_utf8_lossy(&diff).into_owned()
}

impl WorkingTree for GitCli {
    fn changes(&self, dir: &str) -> Result<Vec<FileChange>, VcsError> {
        let root = self.root(dir)?;
        let status =
            self.run(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=all"])?;
        let base = self.base(&root);
        let numstat = [&["diff", "--numstat", "-z"], SAFE_DIFF, &[base]].concat();
        let stats = parse_numstat(&self.run(&root, &numstat)?);
        let mut changes: Vec<FileChange> = parse_status(&status)
            .into_iter()
            .map(|(path, kind)| {
                let (added, removed) = match kind {
                    ChangeKind::Untracked => match std::fs::read(Path::new(&root).join(&path)) {
                        Ok(bytes) if !bytes.contains(&0) => {
                            (Some(bytes.iter().filter(|b| **b == b'\n').count() as u32), Some(0))
                        }
                        _ => (None, None),
                    },
                    _ => stats.get(&path).copied().unwrap_or((Some(0), Some(0))),
                };
                FileChange { path, kind, added, removed }
            })
            .collect();
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    fn diff(&self, dir: &str, path: &str) -> Result<String, VcsError> {
        let change = self.changes(dir)?.into_iter().find(|c| c.path == path).ok_or(VcsError::UnknownFile)?;
        let root = self.root(dir)?;
        if change.kind == ChangeKind::Untracked {
            // `--no-index` sort en code 1 quand il y a une différence : c'est le cas normal ici.
            let out = self
                .command(&root)
                .args(self.filter_overrides(&root))
                .args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-index", "--", "/dev/null", path])
                .output()
                .map_err(|e| VcsError::Git(e.to_string()))?;
            return Ok(truncate(out.stdout));
        }
        let base = self.base(&root);
        let diff = self.run(&root, &[&["diff"], SAFE_DIFF, &[base, "--", path]].concat())?;
        Ok(truncate(diff))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("/usr/bin/git").arg("-C").arg(dir).args(args).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn lists_and_diffs_the_changes_of_a_repository() {
        let dir = std::env::temp_dir().join(format!("robiade-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        let repo = GitCli::default();
        let d = dir.to_str().unwrap();

        std::fs::write(dir.join("a.txt"), "un\ndeux\n").unwrap();
        // Dépôt sans commit : tout est non suivi, comparé à l'arbre vide.
        assert_eq!(repo.changes(d).unwrap()[0].kind, ChangeKind::Untracked);

        git(&dir, &["add", "a.txt"]);
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        std::fs::write(dir.join("a.txt"), "un\ntrois\n").unwrap();
        std::fs::write(dir.join("b.md"), "# titre\n").unwrap();

        let changes = repo.changes(d).unwrap();
        assert_eq!(changes.len(), 2);
        let modified =
            FileChange { path: "a.txt".into(), kind: ChangeKind::Modified, added: Some(1), removed: Some(1) };
        assert_eq!(changes[0], modified);
        assert_eq!(changes[1].kind, ChangeKind::Untracked);
        assert!(repo.diff(d, "a.txt").unwrap().contains("+trois"));
        assert!(repo.diff(d, "b.md").unwrap().contains("+# titre"));
        assert_eq!(repo.diff(d, "../etc/passwd"), Err(VcsError::UnknownFile));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn never_runs_commands_declared_by_the_repository() {
        let dir = std::env::temp_dir().join(format!("robiade-git-trap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join(".gitattributes"), "*.txt diff=piege filter=lfs.like\n").unwrap();
        std::fs::write(dir.join("a.txt"), "un\n").unwrap();
        git(&dir, &["add", "a.txt", ".gitattributes"]);
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        // Pièges posés après le commit : seuls les appels de Robiade peuvent les déclencher.
        let witness = dir.join("piege-declenche");
        let trap = format!("sh -c 'touch {}'", witness.display());
        git(&dir, &["config", "core.fsmonitor", &trap]);
        git(&dir, &["config", "diff.piege.textconv", &trap]);
        git(&dir, &["config", "filter.lfs.like.clean", &format!("sh -c 'touch {}; cat'", witness.display())]);
        git(&dir, &["config", "filter.lfs.like.required", "true"]);
        std::fs::write(dir.join("a.txt"), "deux\n").unwrap();
        let repo = GitCli::default();
        let d = dir.to_str().unwrap();
        assert_eq!(repo.changes(d).unwrap()[0].path, "a.txt");
        assert!(repo.diff(d, "a.txt").unwrap().contains("+deux"));
        assert!(!witness.exists(), "une commande du dépôt a été exécutée");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

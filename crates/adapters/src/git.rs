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

pub struct GitCli {
    git: String,
}

impl Default for GitCli {
    fn default() -> Self {
        Self { git: "/usr/bin/git".to_owned() }
    }
}

impl GitCli {
    fn run(&self, dir: &str, args: &[&str]) -> Result<Vec<u8>, VcsError> {
        let out = Command::new(&self.git)
            .arg("-C")
            .arg(dir)
            .args(["-c", "core.quotepath=off", "--no-pager"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
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
        let status = self.run(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let base = self.base(&root);
        let stats = parse_numstat(&self.run(&root, &["diff", "--numstat", "-z", "-M", "--no-ext-diff", base])?);
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
            let out = Command::new(&self.git)
                .arg("-C")
                .arg(&root)
                .args(["--no-pager", "diff", "--no-color", "--no-ext-diff", "--no-index", "--", "/dev/null", path])
                .env("GIT_OPTIONAL_LOCKS", "0")
                .output()
                .map_err(|e| VcsError::Git(e.to_string()))?;
            return Ok(truncate(out.stdout));
        }
        let base = self.base(&root);
        let diff = self.run(&root, &["diff", "--no-color", "--no-ext-diff", "-M", base, "--", path])?;
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
}

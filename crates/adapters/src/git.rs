//! Modifications en cours lues avec la CLI `git`, en lecture seule et sans prendre de verrou :
//! l'agent peut écrire dans le même dépôt au même moment.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use robiade_core::issue_start::is_valid_branch;
use robiade_core::trunks::TrunkBranches;
use robiade_core::vcs::{ChangeKind, FileChange, LinkedWorktree, VcsError, WorkingTree, Worktrees};

/// Arbre vide de Git : base de comparaison d'un dépôt sans commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// Un diff plus long est tronqué : l'aperçu n'est pas un outil de revue de 50 000 lignes.
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// Réglages imposés à chaque appel : la config d'un dépôt peut déclarer des commandes que `status`
/// ou `diff` exécuteraient (`core.fsmonitor`, filtres `clean`/`textconv`, diff externe, sous-modules).
/// Robiade lance git sans que je le demande, sur des dépôts où un agent écrit : aucune ne doit tourner.
/// Les filtres, nommés librement, sont neutralisés un par un (voir `safe_command`).
const HARDENING: &[&str] = &["-c", "core.fsmonitor=false", "-c", "core.quotepath=off", "-c", "diff.external="];

/// Options de `git diff` qui n'exécutent rien de déclaré par le dépôt.
const SAFE_DIFF: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all", "-M"];

/// Un fetch plus long est abandonné : la base locale sert.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GitCli {
    git: String,
    /// PATH du shell de login, pour les filtres (git-lfs) lors de la création d'un worktree.
    path: Option<String>,
}

impl Default for GitCli {
    fn default() -> Self {
        Self { git: "/usr/bin/git".to_owned(), path: None }
    }
}

impl GitCli {
    pub fn with_path(path: String) -> Self {
        Self { path: Some(path), ..Self::default() }
    }

    /// Écriture que j'ai demandée (créer un worktree). Aucun hook ne tourne ; les filtres, eux,
    /// restent actifs : sans le `smudge` de git-lfs, le worktree contiendrait des pointeurs au lieu
    /// des fichiers. C'est ce que ferait la même commande lancée dans mon terminal.
    fn write_command(&self, dir: &str) -> Command {
        let mut cmd = self.command(dir);
        cmd.args(["-c", "core.hooksPath=/dev/null"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(path) = &self.path {
            cmd.env("PATH", path);
        }
        cmd
    }

    fn command(&self, dir: &str) -> Command {
        let mut cmd = Command::new(&self.git);
        cmd.arg("-C").arg(dir).args(HARDENING).arg("--no-pager").env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C");
        cmd
    }

    /// Commande sûre : chaque filtre déclaré dans la config (dépôt, global, système) est vidé, car un
    /// filtre `clean`, comme celui de git-lfs, tourne à chaque `status` ou `diff`. Lire la config
    /// n'exécute rien. Les clés passent en octets bruts par `GIT_CONFIG_KEY_n` : un nom de filtre
    /// peut contenir espaces, `=` ou retours à la ligne. Config illisible : git ne tourne pas.
    fn safe_command(&self, dir: &str) -> Result<Command, VcsError> {
        let out = self
            .command(dir)
            .args(["config", "-z", "--name-only", "--get-regexp", r"^filter\."])
            .output()
            .map_err(|e| VcsError::Git(e.to_string()))?;
        let keys = match out.status.code() {
            Some(0) => out.stdout,
            Some(1) => Vec::new(), // aucun filtre
            _ => return Err(VcsError::Git("config git illisible : lecture refusée".into())),
        };
        let mut prefixes: Vec<&[u8]> = keys
            .split(|b| *b == 0)
            .filter_map(|key| key.iter().rposition(|b| *b == b'.').map(|dot| &key[..dot]))
            .collect();
        prefixes.sort();
        prefixes.dedup();
        let mut cmd = self.command(dir);
        let mut n = 0;
        for prefix in prefixes {
            for (variable, value) in [("clean", ""), ("smudge", ""), ("process", ""), ("required", "false")] {
                let key = [prefix, b".", variable.as_bytes()].concat();
                cmd.env(format!("GIT_CONFIG_KEY_{n}"), OsStr::from_bytes(&key));
                cmd.env(format!("GIT_CONFIG_VALUE_{n}"), value);
                n += 1;
            }
        }
        cmd.env("GIT_CONFIG_COUNT", n.to_string());
        Ok(cmd)
    }

    fn run(&self, dir: &str, args: &[&str]) -> Result<Vec<u8>, VcsError> {
        let out = self
            .safe_command(dir)?
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

/// `git worktree list --porcelain -z` : un bloc de champs par worktree, clos par un champ vide. Le
/// premier bloc est le dépôt principal, écarté.
fn parse_worktrees(raw: &[u8]) -> Vec<LinkedWorktree> {
    let mut out = Vec::new();
    let mut current: Option<LinkedWorktree> = None;
    for field in raw.split(|b| *b == 0) {
        let field = String::from_utf8_lossy(field);
        let (key, value) = field.split_once(' ').unwrap_or((&field, ""));
        match (key, current.as_mut()) {
            ("worktree", _) => {
                current = Some(LinkedWorktree { path: value.to_owned(), branch: None, locked: false, prunable: false })
            }
            ("branch", Some(w)) => w.branch = Some(value.trim_start_matches("refs/heads/").to_owned()),
            ("locked", Some(w)) => w.locked = true,
            ("prunable", Some(w)) => w.prunable = true,
            ("", _) => out.extend(current.take()),
            _ => {}
        }
    }
    out.extend(current);
    out.into_iter().skip(1).collect()
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
                .safe_command(&root)?
                .args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-index", "--", "/dev/null", path])
                .output()
                .map_err(|e| VcsError::Git(e.to_string()))?;
            return Ok(truncate(out.stdout));
        }
        let base = self.base(&root);
        let diff = self.run(&root, &[&["diff"], SAFE_DIFF, &[base, "--", path]].concat())?;
        Ok(truncate(diff))
    }

    fn files(&self, dir: &str) -> Result<Vec<String>, VcsError> {
        let listed = self.run(dir, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
        let deleted = self.run(dir, &["ls-files", "-z", "--deleted"])?;
        let deleted: HashSet<&[u8]> = deleted.split(|b| *b == 0).collect();
        let mut files: Vec<String> = listed
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty() && !deleted.contains(path))
            .map(|path| String::from_utf8_lossy(path).into_owned())
            .collect();
        // Un fichier en conflit apparaît une fois par version : il n'en faut qu'une.
        files.sort();
        files.dedup();
        Ok(files)
    }
}

impl Worktrees for GitCli {
    fn list(&self, repository: &str) -> Result<Vec<LinkedWorktree>, VcsError> {
        Ok(parse_worktrees(&self.run(repository, &["worktree", "list", "--porcelain", "-z"])?))
    }

    fn bases(&self, repository: &str, trunks: Option<&TrunkBranches>) -> Result<Vec<String>, VcsError> {
        let wanted: Vec<String> = match trunks {
            Some(trunks) => trunks.as_slice().iter().map(|b| format!("origin/{b}")).collect(),
            // Posé par `git clone`, ou par `git remote set-head origin --auto`.
            None => match self.run(repository, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
                Ok(out) => vec![String::from_utf8_lossy(&out).trim().to_owned()],
                Err(VcsError::NotARepository) => return Err(VcsError::NotARepository),
                Err(_) => return Ok(vec![]),
            },
        };
        let wanted: Vec<String> = wanted.into_iter().filter(|b| remote_branch_of(b).is_some()).collect();
        if wanted.is_empty() {
            return Ok(wanted);
        }
        let mut args = vec!["for-each-ref", "--format=%(refname:short)"];
        let refs: Vec<String> = wanted.iter().map(|b| format!("refs/remotes/{b}")).collect();
        args.extend(refs.iter().map(String::as_str));
        let found = String::from_utf8_lossy(&self.run(repository, &args)?).into_owned();
        Ok(wanted.into_iter().filter(|b| found.lines().any(|l| l == b)).collect())
    }

    fn create(&self, repository: &str, branch: &str, base: &str, dir: &str) -> Result<bool, VcsError> {
        let accepted = is_valid_branch(branch) && dir.starts_with('/') && !Path::new(dir).exists();
        let Some(remote_branch) = remote_branch_of(base).filter(|_| accepted) else {
            return Err(VcsError::Git("branche, base ou dossier refusé".into()));
        };
        let fetched = self.fetch(repository, remote_branch);
        let out = self
            .write_command(repository)
            .args(["worktree", "add", "--no-track", "-b", branch, dir, base])
            .output()
            .map_err(|e| VcsError::Git(e.to_string()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(VcsError::Git(stderr.lines().last().unwrap_or_default().to_owned()));
        }
        Ok(fetched)
    }

    fn branch(&self, dir: &str) -> Result<Option<String>, VcsError> {
        match self.run(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
            Ok(out) => Ok(Some(String::from_utf8_lossy(&out).trim().to_owned())),
            Err(VcsError::NotARepository) => Err(VcsError::NotARepository),
            Err(_) => Ok(None), // tête détachée
        }
    }

    fn remove(&self, repository: &str, dir: &str, force: bool) -> Result<(), VcsError> {
        if !dir.starts_with('/') {
            return Err(VcsError::Git("dossier refusé".into()));
        }
        // Un seul `--force` : git en veut deux pour passer outre un verrou.
        let force: &[&str] = if force { &["--force"] } else { &[] };
        let out = self
            .write_command(repository)
            .args([&["worktree", "remove"], force, &["--", dir]].concat())
            .output()
            .map_err(|e| VcsError::Git(e.to_string()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(VcsError::Git(stderr.lines().last().unwrap_or_default().to_owned()));
        }
        Ok(())
    }
}

/// `origin/<branche>` → `<branche>`, si c'est un nom de branche valide : rien d'autre ne sert de base.
fn remote_branch_of(base: &str) -> Option<&str> {
    base.strip_prefix("origin/").filter(|b| is_valid_branch(b))
}

impl GitCli {
    /// `git fetch origin <branche>`, abandonné au-delà de `FETCH_TIMEOUT`.
    fn fetch(&self, repository: &str, branch: &str) -> bool {
        let Ok(mut child) = self.write_command(repository).args(["fetch", "--no-tags", "origin", branch]).spawn() else {
            return false;
        };
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if started.elapsed() < FETCH_TIMEOUT => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
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
    fn reads_linked_worktrees_detached_locked_or_gone() {
        let raw = b"worktree /r\0HEAD a\0branch refs/heads/main\0\0\
worktree /r/.claude/worktrees/x\0HEAD b\0branch refs/heads/feat/x\0locked raison\0\0\
worktree /tmp/y\0HEAD c\0detached\0prunable gitdir file points to non-existent location\0\0";
        let found = parse_worktrees(raw);
        let x = LinkedWorktree {
            path: "/r/.claude/worktrees/x".into(),
            branch: Some("feat/x".into()),
            locked: true,
            prunable: false,
        };
        assert_eq!(found, [x, LinkedWorktree { path: "/tmp/y".into(), branch: None, locked: false, prunable: true }]);
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
    fn lists_the_files_of_a_repository_without_the_ignored_ones() {
        let dir = std::env::temp_dir().join(format!("robiade-git-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("vendor/lib")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join(".gitignore"), "vendor/\n.env.local\n").unwrap();
        std::fs::write(dir.join("vendor/lib/a.php"), "").unwrap();
        std::fs::write(dir.join(".env.local"), "").unwrap();
        std::fs::write(dir.join("src/kept.rs"), "").unwrap();
        std::fs::write(dir.join("src/gone.rs"), "").unwrap();
        git(&dir, &["add", ".gitignore", "src"]);
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        std::fs::remove_file(dir.join("src/gone.rs")).unwrap();
        std::fs::write(dir.join("src/nouveau é.rs"), "").unwrap();

        let files = GitCli::default().files(dir.to_str().unwrap()).unwrap();
        assert_eq!(files, [".gitignore", "src/kept.rs", "src/nouveau é.rs"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn never_runs_commands_declared_by_the_repository() {
        let dir = std::env::temp_dir().join(format!("robiade-git-trap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join(".gitattributes"), "*.txt diff=piege filter=lfs.like\n*.md filter=a=b\n").unwrap();
        std::fs::write(dir.join("b.md"), "un\n").unwrap();
        std::fs::write(dir.join("a.txt"), "un\n").unwrap();
        git(&dir, &["add", "a.txt", "b.md", ".gitattributes"]);
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        // Pièges posés après le commit : seuls les appels de Robiade peuvent les déclencher.
        let witness = dir.join("piege-declenche");
        let trap = format!("sh -c 'touch {}'", witness.display());
        git(&dir, &["config", "core.fsmonitor", &trap]);
        git(&dir, &["config", "diff.piege.textconv", &trap]);
        git(&dir, &["config", "filter.lfs.like.clean", &format!("sh -c 'touch {}; cat'", witness.display())]);
        git(&dir, &["config", "filter.lfs.like.required", "true"]);
        // Un nom qui casserait une clé reconstruite en texte pour `-c clé=valeur`.
        git(&dir, &["config", "filter.a=b.clean", &format!("sh -c 'touch {}; cat'", witness.display())]);
        std::fs::write(dir.join("a.txt"), "deux\n").unwrap();
        std::fs::write(dir.join("b.md"), "deux\n").unwrap();
        let repo = GitCli::default();
        let d = dir.to_str().unwrap();
        assert_eq!(repo.changes(d).unwrap().len(), 2);
        assert!(repo.diff(d, "b.md").unwrap().contains("+deux"));
        assert!(repo.diff(d, "a.txt").unwrap().contains("+deux"));
        assert!(!witness.exists(), "une commande du dépôt a été exécutée");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn creates_a_worktree_from_an_updated_base_without_running_hooks() {
        let base = std::env::temp_dir().join(format!("robiade-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (origin, repo) = (base.join("origin"), base.join("repo"));
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "staging"]);
        std::fs::write(origin.join("a.txt"), "un\n").unwrap();
        git(&origin, &["add", "a.txt"]);
        git(&origin, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        git(&base, &["clone", "-q", origin.to_str().unwrap(), "repo"]);
        // Commit arrivé sur origin après le clone : le fetch doit le ramener.
        std::fs::write(origin.join("b.txt"), "deux\n").unwrap();
        git(&origin, &["add", "b.txt"]);
        git(&origin, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "b"]);
        let witness = base.join("hook-ran");
        let hook = repo.join(".git/hooks/post-checkout");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", witness.display())).unwrap();
        let mut perms = std::fs::metadata(&hook).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&hook, perms).unwrap();

        let git_cli = GitCli::default();
        let r = repo.to_str().unwrap();
        let trunks = |names: &[&str]| TrunkBranches::new(names).unwrap();
        assert_eq!(git_cli.bases(r, Some(&trunks(&["staging", "main", "master"]))).unwrap(), ["origin/staging"]);
        assert_eq!(git_cli.bases(r, Some(&trunks(&["main"]))).unwrap(), Vec::<String>::new(), "main absent d'origin");
        assert_eq!(git_cli.bases(r, None).unwrap(), ["origin/staging"], "sans réglage : origin/HEAD");
        let dir = repo.join(".claude/worktrees/ticket-1-x");
        let d = dir.to_str().unwrap();
        assert_eq!(git_cli.create(r, "ticket-1-x", "origin/staging", d), Ok(true));
        assert!(dir.join("b.txt").exists(), "base mise à jour avant la création");
        assert!(!witness.exists(), "aucun hook ne doit tourner");
        assert!(git_cli.create(r, "ticket-1-x", "origin/staging", d).is_err(), "dossier existant");
        assert!(git_cli.create(r, "-x", "origin/staging", &format!("{d}2")).is_err());
        assert!(git_cli.create(r, "y", "HEAD", &format!("{d}3")).is_err());
        assert!(git_cli.create(r, "y", "origin/-x", &format!("{d}4")).is_err());

        assert_eq!(git_cli.branch(d), Ok(Some("ticket-1-x".to_owned())));
        let listed = git_cli.list(r).unwrap();
        assert_eq!(listed.len(), 1, "le dépôt principal n'est pas listé");
        assert_eq!(listed[0].branch.as_deref(), Some("ticket-1-x"));
        assert!(listed[0].path.ends_with("/.claude/worktrees/ticket-1-x"));
        std::fs::write(dir.join("c.txt"), "trois\n").unwrap();
        assert!(git_cli.remove(r, d, false).is_err(), "un arbre modifié n'est pas supprimé sans force");
        std::fs::remove_file(dir.join("c.txt")).unwrap();
        assert_eq!(git_cli.remove(r, d, false), Ok(()));
        assert!(!dir.exists());
        git(&repo, &["rev-parse", "--verify", "--quiet", "refs/heads/ticket-1-x"]); // branche conservée

        let forced = repo.join(".claude/worktrees/ticket-2-y");
        let f = forced.to_str().unwrap();
        assert_eq!(git_cli.create(r, "ticket-2-y", "origin/staging", f), Ok(true));
        std::fs::write(forced.join("a.txt"), "modifié\n").unwrap();
        std::fs::write(forced.join("d.txt"), "non suivi\n").unwrap();
        // Lien vers un dossier du dépôt, comme un `node_modules` partagé : seul le lien part.
        std::os::unix::fs::symlink(&origin, forced.join("partagé")).unwrap();
        git(&repo, &["worktree", "lock", f]);
        assert!(git_cli.remove(r, f, true).is_err(), "un worktree verrouillé n'est pas supprimé, même de force");
        git(&repo, &["worktree", "unlock", f]);
        assert_eq!(git_cli.remove(r, f, true), Ok(()));
        assert!(!forced.exists());
        assert!(origin.join("a.txt").exists(), "la cible d'un lien n'est pas touchée");
        git(&repo, &["rev-parse", "--verify", "--quiet", "refs/heads/ticket-2-y"]);

        git(&repo, &["remote", "set-head", "origin", "--delete"]);
        assert_eq!(git_cli.bases(r, None).unwrap(), Vec::<String>::new(), "origin/HEAD absent : aucune base devinée");
        std::fs::remove_dir_all(base).unwrap();
    }
}

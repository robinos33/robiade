//! Modifications en cours d'un dossier de travail : ce que l'agent a changé depuis le dernier commit.
//! Lecture seule : Robiade montre les diffs, il ne commite ni n'annule rien. Seule écriture, à ma
//! demande : créer un worktree, ou le supprimer (`Worktrees`).

/// Nature d'un changement par rapport au dernier commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Chemin relatif à la racine du dépôt.
    pub path: String,
    pub kind: ChangeKind,
    /// `None` pour un fichier binaire.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VcsError {
    #[error("pas un dépôt Git")]
    NotARepository,
    #[error("fichier sans modification en cours")]
    UnknownFile,
    #[error("git a échoué : {0}")]
    Git(String),
}

/// Le dépôt d'un dossier de travail, lu tel qu'il est sur le disque.
pub trait WorkingTree: Send + Sync {
    /// Fichiers modifiés, ajoutés, supprimés ou non suivis, par ordre de chemin.
    fn changes(&self, dir: &str) -> Result<Vec<FileChange>, VcsError>;
    /// Diff unifié d'un fichier de `changes` ; tout autre chemin est refusé.
    fn diff(&self, dir: &str, path: &str) -> Result<String, VcsError>;
    /// Fichiers sous `dir`, relatifs à lui et triés : suivis et non suivis, hors
    /// ignorés (`.gitignore`, exclusions globales) et hors fichiers suivis supprimés du disque.
    fn files(&self, dir: &str) -> Result<Vec<String>, VcsError>;
}

/// Un worktree lié d'un dépôt, tel que `git worktree list` le décrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWorktree {
    pub path: String,
    /// `None` si la tête est détachée.
    pub branch: Option<String>,
    /// Verrouillé (`git worktree lock`) : git refuse de le supprimer.
    pub locked: bool,
    /// Dossier disparu : git l'oubliera au prochain `git worktree prune`.
    pub prunable: bool,
}

/// Liste, création de worktrees, toujours sur une branche neuve, et suppression.
pub trait Worktrees: Send + Sync {
    /// Worktrees liés de `repository`, dans l'ordre de git ; le dépôt principal n'en fait pas partie.
    fn list(&self, repository: &str) -> Result<Vec<LinkedWorktree>, VcsError>;
    /// Branches de base proposées (`origin/staging`, `origin/main`, `origin/master`), celles qui existent.
    fn bases(&self, repository: &str) -> Result<Vec<String>, VcsError>;
    /// Met `base` à jour depuis `origin` si possible, puis crée `branch` à partir d'elle dans `dir`.
    /// Renvoie `false` si la mise à jour a échoué et que la base locale a servi.
    fn create(&self, repository: &str, branch: &str, base: &str, dir: &str) -> Result<bool, VcsError>;
    /// Branche extraite dans `dir` ; `None` si la tête est détachée.
    fn branch(&self, dir: &str) -> Result<Option<String>, VcsError>;
    /// Supprime le worktree `dir` de `repository`. Sans `force`, git refuse un arbre modifié ;
    /// avec, les modifications non commitées sont perdues. Un worktree verrouillé est refusé dans
    /// les deux cas. La branche est conservée.
    fn remove(&self, repository: &str, dir: &str, force: bool) -> Result<(), VcsError>;
}

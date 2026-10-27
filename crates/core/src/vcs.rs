//! Modifications en cours d'un dossier de travail : ce que l'agent a changé depuis le dernier commit.
//! Lecture seule : Robiade montre les diffs, il ne commite ni n'annule rien.

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
}

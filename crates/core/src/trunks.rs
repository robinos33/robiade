//! Branches de tronc d'un dépôt : celles où une branche de ticket se termine. Elles décident de la
//! base d'un nouveau worktree (la première qui existe sur `origin`) et des wagons d'un train de MR
//! (une MR qui cible une autre branche). Réglées dépôt par dépôt ; sans réglage, le tronc est la
//! branche par défaut du remote.

use crate::issue_start::is_valid_branch;

/// Au-delà, la liste n'est plus un tronc mais un inventaire.
pub const MAX_TRUNKS: usize = 10;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrunkError {
    #[error("nom de branche refusé : {0}")]
    InvalidBranch(String),
    #[error("au moins une branche de tronc")]
    Empty,
    #[error("au plus {MAX_TRUNKS} branches de tronc")]
    TooMany,
}

/// Liste ordonnée, sans doublon, de noms de branches valides (sans `origin/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrunkBranches(Vec<String>);

impl TrunkBranches {
    /// Noms rognés ; les vides sont ignorés, un doublon ne garde que sa première place.
    pub fn new<S: AsRef<str>>(branches: impl IntoIterator<Item = S>) -> Result<Self, TrunkError> {
        let mut kept: Vec<String> = Vec::new();
        for raw in branches {
            let name = raw.as_ref().trim();
            if name.is_empty() || kept.iter().any(|k| k == name) {
                continue;
            }
            if !is_valid_branch(name) {
                return Err(TrunkError::InvalidBranch(name.to_owned()));
            }
            kept.push(name.to_owned());
        }
        match kept.len() {
            0 => Err(TrunkError::Empty),
            n if n > MAX_TRUNKS => Err(TrunkError::TooMany),
            _ => Ok(Self(kept)),
        }
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn contains(&self, branch: &str) -> bool {
        self.0.iter().any(|b| b == branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trunk_list_keeps_its_order_without_duplicates() {
        let trunks = TrunkBranches::new([" staging ", "main", "", "staging", "master"]).unwrap();
        assert_eq!(trunks.as_slice(), ["staging", "main", "master"]);
        assert!(trunks.contains("main") && !trunks.contains("ticket-1"));
    }

    #[test]
    fn a_trunk_list_refuses_what_git_would_not_take_as_a_branch() {
        assert_eq!(TrunkBranches::new(["", " "]), Err(TrunkError::Empty));
        assert_eq!(TrunkBranches::new(["main", "-x"]), Err(TrunkError::InvalidBranch("-x".into())));
        assert_eq!(TrunkBranches::new(["origin/../main"]), Err(TrunkError::InvalidBranch("origin/../main".into())));
        let many: Vec<String> = (0..=MAX_TRUNKS).map(|i| format!("b{i}")).collect();
        assert_eq!(TrunkBranches::new(&many), Err(TrunkError::TooMany));
    }
}

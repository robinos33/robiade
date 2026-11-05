//! Où Claude Code range les scratchpads de ses sessions sur cette machine.

use std::path::{Path, PathBuf};

/// Racine des scratchpads, résolue : `$CLAUDE_CODE_TMPDIR/claude-<uid>`, `/tmp/claude-<uid>` à
/// défaut, comme Claude Code 2.1.318. Les agents héritent de l'environnement de Robiade : ils
/// voient la même variable. `None` si le dossier temporaire ne se résout pas.
pub fn root() -> Option<PathBuf> {
    let tmp = std::env::var_os("CLAUDE_CODE_TMPDIR").filter(|v| !v.is_empty()).unwrap_or_else(|| "/tmp".into());
    // SAFETY: getuid n'a pas de précondition.
    let uid = unsafe { libc::getuid() };
    Some(Path::new(&tmp).canonicalize().ok()?.join(format!("claude-{uid}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_under_the_resolved_tmp_and_named_after_my_uid() {
        if std::env::var_os("CLAUDE_CODE_TMPDIR").is_some() {
            return;
        }
        let root = root().unwrap();
        // SAFETY: getuid n'a pas de précondition.
        assert!(root.ends_with(format!("claude-{}", unsafe { libc::getuid() })));
        assert_eq!(root.parent(), Path::new("/tmp").canonicalize().ok().as_deref());
    }
}

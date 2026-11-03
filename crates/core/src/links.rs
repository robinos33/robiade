//! Liens du terminal d'un agent : ce qu'un ⌘+clic ouvre. La sortie d'un agent n'est pas fiable
//! (elle reprend une page, une issue ou un fichier qu'il a lu) : ce module ne fait que lire l'URL
//! ou le chemin, l'app décide ensuite si le fichier désigné peut être affiché.

use std::path::{Path, PathBuf};

/// Chemin absolu désigné par une URL `file://`, décodé (`%20`…). Fragment et paramètres sont
/// ignorés. `None` pour toute autre URL, un hôte autre que cette machine, un encodage invalide,
/// un chemin qui n'est pas de l'UTF-8 ou qui contient un octet nul.
pub fn file_url_path(uri: &str) -> Option<String> {
    let scheme = uri.get(..7)?;
    if !scheme.eq_ignore_ascii_case("file://") {
        return None;
    }
    let rest = &uri[7..];
    let (host, path) = rest.split_at(rest.find('/')?);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let decoded = percent_decode(path)?;
    (!decoded.contains('\0')).then_some(decoded)
}

/// Chemin absolu d'un chemin cité en clair (`src/foo.rs`, `/p/a.rs`, `~/notes.md`) : relatif au
/// dossier de l'agent `cwd`, `~/` au dossier personnel `home`. Purement lexical : l'app vérifie
/// ensuite que le fichier existe et reste dans le dossier de l'agent. `None` pour un chemin vide,
/// qui contient un octet nul, ou qui part de `~/` sans dossier personnel connu.
pub fn cited_path(cwd: &Path, home: Option<&Path>, cited: &str) -> Option<PathBuf> {
    if cited.is_empty() || cited.contains('\0') {
        return None;
    }
    match cited.strip_prefix("~/") {
        Some(rest) => Some(home?.join(rest)),
        None => Some(cwd.join(cited)),
    }
}

/// `%xx` remplacé par son octet ; un `%` qui n'est pas suivi de deux chiffres hexadécimaux est
/// refusé plutôt que deviné.
fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_path_of_a_local_file_url() {
        assert_eq!(file_url_path("file:///Users/r/p/rapport.md").as_deref(), Some("/Users/r/p/rapport.md"));
        assert_eq!(file_url_path("FILE:///p/a.md").as_deref(), Some("/p/a.md"));
        assert_eq!(file_url_path("file://localhost/p/a.md").as_deref(), Some("/p/a.md"));
        assert_eq!(file_url_path("file:///p/mon%20rapport.md").as_deref(), Some("/p/mon rapport.md"));
        assert_eq!(file_url_path("file:///p/r%C3%A9sum%C3%A9.md").as_deref(), Some("/p/résumé.md"));
        assert_eq!(file_url_path("file:///p/a.md#titre").as_deref(), Some("/p/a.md"));
        assert_eq!(file_url_path("file:///p/a.md?x=1").as_deref(), Some("/p/a.md"));
        // Décodé une seule fois : `%252e` reste `%2e`, pas `.`.
        assert_eq!(file_url_path("file:///p/%252e%252e/a").as_deref(), Some("/p/%2e%2e/a"));
    }

    #[test]
    fn places_a_cited_path_in_the_agent_folder() {
        let (cwd, home) = (Path::new("/p/projet"), Path::new("/Users/r"));
        let cited = |c: &str| cited_path(cwd, Some(home), c);
        assert_eq!(cited("src/foo.rs"), Some(PathBuf::from("/p/projet/src/foo.rs")));
        assert_eq!(cited("./foo.rs"), Some(PathBuf::from("/p/projet/./foo.rs")));
        assert_eq!(cited("../voisin/a.md"), Some(PathBuf::from("/p/projet/../voisin/a.md")));
        assert_eq!(cited("/etc/passwd"), Some(PathBuf::from("/etc/passwd")));
        assert_eq!(cited("~/notes.md"), Some(PathBuf::from("/Users/r/notes.md")));
        // `~autre/` n'est pas développé : c'est un nom de dossier comme un autre.
        assert_eq!(cited("~autre/a.md"), Some(PathBuf::from("/p/projet/~autre/a.md")));
        assert_eq!(cited(""), None);
        assert_eq!(cited("a\0b"), None);
        assert_eq!(cited_path(cwd, None, "~/notes.md"), None);
    }

    #[test]
    fn refuses_anything_else() {
        for uri in [
            "https://gitlab.example/a",
            "file:/p/a.md",
            "file:p/a.md",
            "file://",
            "file://serveur/partage/a.md",
            "file:///p/%zz.md",
            "file:///p/a%2",
            "file:///p/%FF.md",
            "file:///p/a%00.md",
            "x-apple.systempreferences://",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(file_url_path(uri), None, "{uri}");
        }
    }
}

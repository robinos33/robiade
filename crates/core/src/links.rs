//! Liens du terminal d'un agent : ce qu'un ⌘+clic ouvre. La sortie d'un agent n'est pas fiable
//! (elle reprend une page, une issue ou un fichier qu'il a lu) : ce module ne fait que lire l'URL,
//! l'app décide ensuite si le fichier désigné peut être affiché.

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

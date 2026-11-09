//! Règles d'adresse communes aux forges : une URL web qu'on peut ouvrir, et le chemin d'un remote
//! Git. Chaque fournisseur y ajoute la forme de ses chemins.

/// `https://<authority>` suivi d'un chemin, d'une requête ou d'un fragment, ou de rien. Des
/// identifiants (`moi@`) ou un autre port changent l'autorité : refusés. Un caractère de contrôle
/// ou une espace aussi : `open` ne doit recevoir qu'une URL d'un seul tenant.
pub(crate) fn is_web_url_of(authority: &str, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let found = rest.split(['/', '?', '#']).next().unwrap_or_default();
    found.eq_ignore_ascii_case(authority) && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Chemin d'un remote Git sur `host` (sans port), sans `/` autour ni `.git` final. En SSH
/// (`git@hôte:chemin.git`, `ssh://git@hôte[:port]/chemin.git`) ou en HTTPS. Des identifiants dans
/// l'URL ne sont jamais repris. Le chemin n'est pas validé : c'est au fournisseur de le faire.
pub(crate) fn remote_path<'a>(host: &str, url: &'a str) -> Option<&'a str> {
    let url = url.trim();
    let (authority, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("ssh://")) {
        rest.split_once('/')?
    } else {
        // Forme scp : `utilisateur@hôte:chemin`.
        let (authority, path) = url.split_once(':')?;
        if authority.contains('/') {
            return None;
        }
        (authority, path)
    };
    let found = authority.rsplit('@').next()?;
    let found = found.split_once(':').map_or(found, |(h, _port)| h);
    if !found.eq_ignore_ascii_case(host) {
        return None;
    }
    let path = path.trim_start_matches('/').trim_end_matches('/');
    Some(path.strip_suffix(".git").unwrap_or(path))
}

/// Encode tout sauf lettres, chiffres, `-`, `_` et `~` : un nom de branche ou de label devient un
/// seul segment de chemin. Le point aussi, pour qu'un label `..` ne remonte pas d'un cran.
pub(crate) fn encode_path(raw: &str) -> String {
    raw.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_or_label_name_becomes_a_single_path_segment() {
        assert_eq!(encode_path("feat/cache"), "feat%2Fcache");
        assert_eq!(encode_path("à faire"), "%C3%A0%20faire");
        assert_eq!(encode_path("../x?y#z"), "%2E%2E%2Fx%3Fy%23z");
    }
}

//! Masquage des secrets à l'affichage de l'historique.
//!
//! Un transcript peut contenir un jeton collé par erreur : il ne doit pas s'afficher en clair,
//! ni sur une capture d'écran ni dans un partage d'écran. Le repérage est **heuristique** : il
//! reconnaît les formats de jetons connus par leur préfixe (`glpat-`, `sk-ant-`, `AKIA`…) et les
//! blocs de clé privée, pas un mot de passe quelconque. Le préfixe reste lisible, pour savoir ce
//! qui a été masqué ; le reste devient [`MASK`].
//!
//! Les marqueurs de surbrillance de la recherche ([`HIT_START`], [`HIT_END`]) sont transparents :
//! un passage trouvé peut couper un jeton en deux sans le faire échapper au masquage, et les
//! marqueurs sont conservés, toujours appariés.

use crate::history::{HIT_END, HIT_START};

pub const MASK: &str = "••••••";

#[derive(Clone, Copy)]
enum Charset {
    /// Lettres, chiffres, `-` et `_` : la plupart des jetons.
    Token,
    /// Comme `Token`, plus le `.` : JWT, jetons Vault.
    Dotted,
    /// Majuscules et chiffres : identifiants de clé AWS.
    Upper,
}

impl Charset {
    fn accepts(self, c: char) -> bool {
        match self {
            Self::Token => c.is_ascii_alphanumeric() || c == '-' || c == '_',
            Self::Dotted => c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.',
            Self::Upper => c.is_ascii_uppercase() || c.is_ascii_digit(),
        }
    }
}

/// Préfixe connu, et longueur minimale de ce qui le suit pour que ce soit un jeton plutôt
/// qu'une mention du format (« les jetons commencent par `glpat-` »).
struct Shape {
    prefix: &'static str,
    min: usize,
    charset: Charset,
}

const fn shape(prefix: &'static str, min: usize, charset: Charset) -> Shape {
    Shape { prefix, min, charset }
}

/// Du plus précis au plus général : `sk-ant-` passe avant `sk-`.
const SHAPES: &[Shape] = &[
    // GitLab : jetons personnels, de déploiement, de runner, de déclencheur, OAuth, CI/CD.
    shape("glpat-", 16, Charset::Token),
    shape("gldt-", 16, Charset::Token),
    shape("glrt-", 16, Charset::Token),
    shape("glptt-", 16, Charset::Token),
    shape("gloas-", 16, Charset::Token),
    shape("glcbt-", 16, Charset::Token),
    // Anthropic, puis les clés `sk-` des autres fournisseurs, plus longues qu'un nom de paquet.
    shape("sk-ant-", 16, Charset::Token),
    shape("sk-", 32, Charset::Token),
    // GitHub.
    shape("github_pat_", 16, Charset::Token),
    shape("ghp_", 16, Charset::Token),
    shape("gho_", 16, Charset::Token),
    shape("ghu_", 16, Charset::Token),
    shape("ghs_", 16, Charset::Token),
    shape("ghr_", 16, Charset::Token),
    // Slack.
    shape("xoxb-", 16, Charset::Token),
    shape("xoxp-", 16, Charset::Token),
    shape("xoxa-", 16, Charset::Token),
    shape("xoxr-", 16, Charset::Token),
    shape("xoxs-", 16, Charset::Token),
    shape("xapp-", 16, Charset::Token),
    // AWS (clé longue et clé temporaire), Google.
    shape("AKIA", 16, Charset::Upper),
    shape("ASIA", 16, Charset::Upper),
    shape("AIza", 30, Charset::Token),
    // Vault : jetons de service et de lot.
    shape("hvs.", 20, Charset::Dotted),
    shape("hvb.", 20, Charset::Dotted),
    // Stripe.
    shape("sk_live_", 16, Charset::Token),
    shape("rk_live_", 16, Charset::Token),
    // JWT : en-tête JSON encodé en base64url, toujours `{"`.
    shape("eyJ", 30, Charset::Dotted),
    // En-tête HTTP d'authentification, quel que soit le jeton.
    shape("Bearer ", 20, Charset::Dotted),
];

const KEY_BEGIN: &str = "-----BEGIN ";
const KEY_FENCE: &str = "-----";
const KEY_END: &str = "-----END ";

/// `text` avec chaque secret reconnu remplacé par [`MASK`], préfixe conservé.
pub fn mask_secrets(text: &str) -> String {
    // Repérage sur le texte sans marqueurs, puis réécriture du texte d'origine.
    let plain: Vec<char> = text.chars().filter(|c| !is_marker(*c)).collect();
    let ranges = secret_ranges(&plain);
    if ranges.is_empty() {
        return text.to_owned();
    }

    let mut out = String::with_capacity(text.len());
    let mut ranges = ranges.into_iter().peekable();
    let mut at = 0; // position dans `plain`
    for c in text.chars() {
        if is_marker(c) {
            out.push(c);
            continue;
        }
        while ranges.peek().is_some_and(|r| r.1 <= at) {
            ranges.next();
        }
        match ranges.peek() {
            Some(&(start, _)) if at == start => out.push_str(MASK),
            Some(&(start, end)) if (start..end).contains(&at) => {}
            _ => out.push(c),
        }
        at += 1;
    }
    out
}

fn is_marker(c: char) -> bool {
    c == HIT_START || c == HIT_END
}

/// Plages `[début, fin)` de `plain` à masquer, triées et disjointes.
fn secret_ranges(plain: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = vec![];
    let mut i = 0;
    while i < plain.len() {
        let boundary = i == 0 || !Charset::Dotted.accepts(plain[i - 1]);
        if boundary {
            if let Some(range) = private_key(plain, i).or_else(|| token(plain, i)) {
                ranges.push(range);
                i = range.1;
                continue;
            }
        }
        i += 1;
    }
    ranges
}

fn token(plain: &[char], at: usize) -> Option<(usize, usize)> {
    SHAPES.iter().find_map(|s| {
        let start = at + starts_with(plain, at, s.prefix)?;
        let end = start + plain[start..].iter().take_while(|c| s.charset.accepts(**c)).count();
        (end - start >= s.min).then_some((start, end))
    })
}

/// Corps d'un bloc PEM de clé privée, de la fin de son en-tête à la fin de son pied
/// (ou du texte, si l'extrait est coupé avant).
fn private_key(plain: &[char], at: usize) -> Option<(usize, usize)> {
    let after_begin = at + starts_with(plain, at, KEY_BEGIN)?;
    let header_end = find(plain, after_begin, KEY_FENCE)?;
    let label: String = plain[after_begin..header_end].iter().collect();
    if !label.ends_with("PRIVATE KEY") || label.contains('\n') {
        return None;
    }
    let body = header_end + KEY_FENCE.len();
    let end = find(plain, body, KEY_END)
        .and_then(|e| find(plain, e + KEY_END.len(), KEY_FENCE).map(|f| f + KEY_FENCE.len()))
        .unwrap_or(plain.len());
    Some((body, end))
}

/// Longueur en caractères de `prefix` si `plain` le porte à la position `at`.
fn starts_with(plain: &[char], at: usize, prefix: &str) -> Option<usize> {
    let mut n = 0;
    for p in prefix.chars() {
        if plain.get(at + n) != Some(&p) {
            return None;
        }
        n += 1;
    }
    Some(n)
}

fn find(plain: &[char], from: usize, needle: &str) -> Option<usize> {
    (from..plain.len()).find(|&i| starts_with(plain, i, needle).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Assemblé à la compilation : un jeton littéral déclenche les scanners de secrets.
    const GITLAB: &str = concat!("glpat-", "AbCdEf0123456789xyzw");

    #[test]
    fn masks_a_known_token_and_keeps_its_prefix() {
        assert_eq!(mask_secrets(&format!("export TOKEN={GITLAB} ; ok")), "export TOKEN=glpat-•••••• ; ok");
    }

    #[test]
    fn leaves_a_mention_of_the_format_alone() {
        let text = "les jetons GitLab commencent par glpat- et sk-learn est un paquet";
        assert_eq!(mask_secrets(text), text);
    }

    #[test]
    fn ignores_a_prefix_glued_to_a_word() {
        let text = format!("x{GITLAB}");
        assert_eq!(mask_secrets(&text), text);
    }

    #[test]
    fn masks_every_token_of_a_text() {
        let text = format!("{GITLAB} puis AKIAABCDEFGHIJKLMNOP et Bearer abcdefghijklmnopqrstuvwxyz.123");
        assert_eq!(mask_secrets(&text), "glpat-•••••• puis AKIA•••••• et Bearer ••••••");
    }

    #[test]
    fn prefers_the_most_precise_prefix() {
        assert_eq!(mask_secrets("sk-ant-api03-abcdefghijklmnop"), "sk-ant-••••••");
    }

    #[test]
    fn masks_the_body_of_a_private_key() {
        let text = "clé :\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----\nfin";
        assert_eq!(mask_secrets(text), "clé :\n-----BEGIN OPENSSH PRIVATE KEY-----••••••\nfin");
    }

    #[test]
    fn masks_a_private_key_cut_before_its_end() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow";
        assert_eq!(mask_secrets(text), "-----BEGIN RSA PRIVATE KEY-----••••••");
    }

    #[test]
    fn leaves_a_public_certificate_alone() {
        let text = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
        assert_eq!(mask_secrets(text), text);
    }

    #[test]
    fn sees_a_token_split_by_a_search_highlight_and_keeps_the_markers() {
        let text = format!("{HIT_START}glpat{HIT_END}-AbCdEf0123456789xyzw fin");
        assert_eq!(mask_secrets(&text), format!("{HIT_START}glpat{HIT_END}-•••••• fin"));

        let text = format!("glpat-{HIT_START}AbCdEf0123456789xyzw{HIT_END}");
        assert_eq!(mask_secrets(&text), format!("glpat-{HIT_START}••••••{HIT_END}"));
    }
}

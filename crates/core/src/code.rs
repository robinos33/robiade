//! Onglet Code : lire un fichier d'un projet, en lecture seule, pour un coup d'œil sans ouvrir
//! l'éditeur. Ce module décide **quoi** lire ; l'app vérifie que le fichier reste dans le projet
//! et le lit.

use crate::assets::AssetKind;

/// Texte affiché au plus : au-delà, un fichier ne se lit plus d'un coup d'œil.
pub const MAX_TEXT_BYTES: u64 = 1024 * 1024;
/// Image affichée au plus, comme pour les assets d'un agent.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Octets examinés pour reconnaître un binaire, comme le fait git.
pub const BINARY_PROBE: usize = 8000;

/// Ce qu'on fait d'un fichier avant de l'ouvrir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peek {
    /// Fichier de secrets connu : lu seulement à ma demande.
    Secret,
    /// Trop gros pour l'aperçu : rien n'est lu.
    TooLarge { size: u64, limit: u64 },
    /// À lire, et à afficher ainsi.
    Read(AssetKind),
}

/// Décide de la lecture d'après le chemin et la taille, sans rien lire. Un fichier qui n'est pas
/// un asset (le code) s'affiche en texte.
pub fn peek(path: &str, size: u64, reveal: bool) -> Peek {
    if is_secret_file(path) && !reveal {
        return Peek::Secret;
    }
    let kind = AssetKind::of(path).unwrap_or(AssetKind::Text);
    let limit = if kind == AssetKind::Image { MAX_IMAGE_BYTES } else { MAX_TEXT_BYTES };
    if size > limit {
        return Peek::TooLarge { size, limit };
    }
    Peek::Read(kind)
}

/// Binaire si un octet nul apparaît dans les [`BINARY_PROBE`] premiers octets : la règle de git.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
}

/// Le masquage des jetons s'applique à tout ce qui s'affiche, sauf au fichier de secrets que
/// j'ai demandé à voir : le masquer contredirait la demande.
pub fn masks_tokens(path: &str, reveal: bool) -> bool {
    !(reveal && is_secret_file(path))
}

/// Chemin relatif simple : des noms séparés par `/`, sans `.`, `..`, ni élément vide, et hors du
/// dossier `.git` (sa config peut porter des identifiants), quelle que soit la casse : le disque
/// d'un Mac l'ignore. C'est la seule forme qu'une liste de `git ls-files` contient ; tout le reste
/// est refusé.
pub fn is_plain_relative(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.eq_ignore_ascii_case(".git"))
}

/// Noms exacts de fichiers qui portent des secrets.
const SECRET_NAMES: &[&str] = &[
    ".envrc",
    ".npmrc",
    ".yarnrc.yml",
    ".pypirc",
    ".netrc",
    ".pgpass",
    ".my.cnf",
    ".htpasswd",
    ".git-credentials",
    ".vault-token",
    ".dockercfg",
    "auth.json", // Composer : jetons des dépôts privés
    "credentials",
    "credentials.json",
    "secrets.json",
    "secrets.yml",
    "secrets.yaml",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
];

/// Extensions de clés, certificats avec clé, et variables Terraform.
const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "tfvars", "kdbx"];

/// Fichier de secrets connu, d'après son nom seul : `.env*`, clés privées, fichiers d'identifiants
/// d'outils. Heuristique, comme le masquage de l'historique : un mot de passe dans `config.php`
/// lui échappe, mais pas un jeton au format connu (voir [`masks_tokens`]).
pub fn is_secret_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if lower == ".env" || lower.starts_with(".env.") || SECRET_NAMES.contains(&lower.as_str()) {
        return true;
    }
    // Clé de déchiffrement des secrets Symfony (`config/secrets/<env>/<env>.decrypt.private.php`).
    if lower.ends_with(".decrypt.private.php") {
        return true;
    }
    lower.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && SECRET_EXTENSIONS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_known_secret_files_by_name() {
        for path in [
            ".env",
            "app/.env.local",
            ".ENV.prod",
            "auth.json",
            "~/.ssh/id_ed25519",
            "certs/server.key",
            "infra/prod.tfvars",
            "config/secrets/prod/prod.decrypt.private.php",
        ] {
            assert!(is_secret_file(path), "{path}");
        }
        for path in [
            "src/Environment.php",
            ".envoy.yml",
            "id_ed25519.pub",
            "config/secrets/prod/prod.encrypt.public.php",
            "keyboard.ts",
            ".key",
        ] {
            assert!(!is_secret_file(path), "{path}");
        }
    }

    #[test]
    fn a_secret_file_is_read_only_on_request() {
        assert_eq!(peek(".env", 10, false), Peek::Secret);
        assert_eq!(peek(".env", 10, true), Peek::Read(AssetKind::Text));
        assert!(!masks_tokens(".env", true));
        assert!(masks_tokens(".env", false));
        // Demander à voir un fichier ordinaire ne lève pas le masquage des jetons.
        assert!(masks_tokens("src/main.rs", true));
    }

    #[test]
    fn code_reads_as_text_and_assets_keep_their_kind() {
        assert_eq!(peek("src/main.rs", 10, false), Peek::Read(AssetKind::Text));
        assert_eq!(peek("Makefile", 10, false), Peek::Read(AssetKind::Text));
        assert_eq!(peek("docs/plan.md", 10, false), Peek::Read(AssetKind::Markdown));
        assert_eq!(peek("logo.png", 10, false), Peek::Read(AssetKind::Image));
    }

    #[test]
    fn a_large_file_is_not_read() {
        let over = MAX_TEXT_BYTES + 1;
        assert_eq!(peek("dump.sql", over, false), Peek::TooLarge { size: over, limit: MAX_TEXT_BYTES });
        assert_eq!(peek("capture.png", over, false), Peek::Read(AssetKind::Image));
        let huge = MAX_IMAGE_BYTES + 1;
        assert_eq!(peek("capture.png", huge, false), Peek::TooLarge { size: huge, limit: MAX_IMAGE_BYTES });
    }

    #[test]
    fn a_nul_byte_near_the_start_means_binary() {
        assert!(looks_binary(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(!looks_binary("<?php echo 'é';".as_bytes()));
        let mut late = vec![b'a'; BINARY_PROBE];
        late.push(0);
        assert!(!looks_binary(&late));
    }

    #[test]
    fn accepts_only_plain_relative_paths() {
        assert!(is_plain_relative("src/main.rs"));
        assert!(is_plain_relative(".env"));
        assert!(is_plain_relative("..."));
        assert!(is_plain_relative(".gitignore"));
        for path in ["", "/etc/hosts", "../x", "src/../../x", "./src", "src//a", "src/", ".git/config", "sub/.Git/HEAD"] {
            assert!(!is_plain_relative(path), "{path}");
        }
    }
}

//! Assets : fichiers qu'un agent écrit et que je veux regarder sans quitter Robiade
//! (captures, schémas, pages HTML, notes Markdown). Le code source n'en fait pas partie.

use crate::Millis;

/// Comment Robiade affiche un asset ; l'extension seule en décide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Html,
    Markdown,
    Text,
}

impl AssetKind {
    /// `None` pour tout fichier qui n'est pas un asset, dont le code.
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next()?;
        let (stem, ext) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => Self::Image,
            "html" | "htm" => Self::Html,
            "md" | "markdown" => Self::Markdown,
            "txt" | "csv" => Self::Text,
            _ => return None,
        })
    }

    /// Type MIME des images, pour les afficher depuis leurs octets.
    pub fn image_mime(path: &str) -> Option<&'static str> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub path: String,
    pub kind: AssetKind,
    pub written_at: Millis,
}

/// Assets d'un agent, le dernier écrit en tête. Réécrire un fichier le remonte sans le dupliquer.
#[derive(Debug, Clone, Default)]
pub struct Assets(Vec<Asset>);

impl Assets {
    /// Garde le fichier s'il est un asset désigné par un chemin absolu ; ignore le reste.
    pub fn record(&mut self, path: &str, now: Millis) {
        if !path.starts_with('/') {
            return;
        }
        let Some(kind) = AssetKind::of(path) else { return };
        self.0.retain(|a| a.path != path);
        self.0.insert(0, Asset { path: path.to_owned(), kind, written_at: now });
    }

    pub fn get(&self, path: &str) -> Option<&Asset> {
        self.0.iter().find(|a| a.path == path)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Asset> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_viewable_files_are_assets() {
        assert_eq!(AssetKind::of("/p/capture.PNG"), Some(AssetKind::Image));
        assert_eq!(AssetKind::of("/p/rapport.html"), Some(AssetKind::Html));
        assert_eq!(AssetKind::of("/p/PLAN.md"), Some(AssetKind::Markdown));
        assert_eq!(AssetKind::of("/p/src/main.rs"), None);
        assert_eq!(AssetKind::of("/p/.md"), None);
        assert_eq!(AssetKind::of("/p/Makefile"), None);
    }

    #[test]
    fn a_rewritten_asset_moves_to_the_top_once() {
        let mut assets = Assets::default();
        assets.record("/p/a.png", 1);
        assets.record("/p/b.md", 2);
        assets.record("/p/a.png", 3);
        assets.record("/p/main.rs", 4);
        assets.record("relatif.png", 5);
        let paths: Vec<_> = assets.iter().map(|a| (a.path.as_str(), a.written_at)).collect();
        assert_eq!(paths, [("/p/a.png", 3), ("/p/b.md", 2)]);
    }
}

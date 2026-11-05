//! Couleur d'un projet : une identité, jamais un état. Choisie dans une palette fixe, sans orange ni
//! rouge, que l'action et le danger portent déjà ; un projet sans couleur reste neutre. Réglée dépôt
//! par dépôt, comme les branches de tronc : les worktrees héritent de leur projet.

/// Teintes proposées, dans l'ordre du sélecteur. Chacune a sa variable `--project-<nom>` dans
/// `ui/src/app.css`, définie pour les deux thèmes. Sept, pas huit : une huitième tomberait près du
/// rouge du danger, de l'ambre de l'attente ou du gris neutre, qu'elle laisserait lire « aucune ».
pub const PALETTE: [&str; 7] = ["olive", "vert", "sarcelle", "ciel", "bleu", "violet", "rose"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("couleur de projet inconnue : {0}")]
pub struct UnknownColor(String);

/// Une teinte de la palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectColor(&'static str);

impl ProjectColor {
    pub fn parse(name: &str) -> Result<Self, UnknownColor> {
        PALETTE.iter().find(|c| **c == name).map(|c| Self(c)).ok_or_else(|| UnknownColor(name.to_owned()))
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_palette_is_accepted() {
        assert_eq!(ProjectColor::parse("sarcelle").unwrap().as_str(), "sarcelle");
        assert!(ProjectColor::parse("orange").is_err());
        assert!(ProjectColor::parse("#ff0000").is_err());
        assert!(ProjectColor::parse("Bleu").is_err());
    }

    #[test]
    fn the_palette_leaves_out_the_colors_of_action_and_danger() {
        for taken in ["orange", "rouge", "jaune", "ambre", "gris"] {
            assert!(!PALETTE.contains(&taken));
        }
    }
}

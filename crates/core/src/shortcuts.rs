//! Raccourcis de la barre sous le terminal : un libellé et un texte, collé au curseur du prompt de
//! l'agent sans être envoyé. Le texte part vers un PTY : aucun caractère de contrôle n'y entre,
//! sauf la tabulation et le saut de ligne, qu'un collage garde dans le prompt.

const LABEL_MAX_CHARS: usize = 24;
const TEXT_MAX_CHARS: usize = 4_000;
const MAX_SHORTCUTS: usize = 20;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShortcutError {
    #[error("libellé vide")]
    EmptyLabel,
    #[error("libellé trop long : {LABEL_MAX_CHARS} caractères au plus, sur une ligne")]
    LabelTooLong,
    #[error("texte vide")]
    EmptyText,
    #[error("texte trop long : {TEXT_MAX_CHARS} caractères au plus")]
    TextTooLong,
    #[error("caractère de contrôle refusé dans « {0} »")]
    Control(String),
    #[error("{MAX_SHORTCUTS} raccourcis au plus")]
    TooMany,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    label: String,
    text: String,
}

impl Shortcut {
    /// Libellé et texte débarrassés des espaces qui les entourent.
    pub fn new(label: &str, text: &str) -> Result<Self, ShortcutError> {
        let (label, text) = (label.trim(), text.trim());
        if label.is_empty() {
            return Err(ShortcutError::EmptyLabel);
        }
        if label.chars().count() > LABEL_MAX_CHARS || label.contains('\n') {
            return Err(ShortcutError::LabelTooLong);
        }
        if text.is_empty() {
            return Err(ShortcutError::EmptyText);
        }
        if text.chars().count() > TEXT_MAX_CHARS {
            return Err(ShortcutError::TextTooLong);
        }
        if [label, text].iter().any(|s| s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')) {
            return Err(ShortcutError::Control(label.to_owned()));
        }
        Ok(Self { label: label.to_owned(), text: text.to_owned() })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Ce qui est collé : une commande slash seule est suivie d'une espace, pour taper ses
    /// arguments à la suite.
    pub fn pasted(&self) -> String {
        let command = self.text.starts_with('/') && !self.text.contains(char::is_whitespace);
        if command {
            format!("{} ", self.text)
        } else {
            self.text.clone()
        }
    }
}

/// La liste, dans l'ordre de la barre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcuts(Vec<Shortcut>);

impl Shortcuts {
    pub fn new(list: Vec<Shortcut>) -> Result<Self, ShortcutError> {
        if list.len() > MAX_SHORTCUTS {
            return Err(ShortcutError::TooMany);
        }
        Ok(Self(list))
    }

    /// Ceux d'un profil qui n'a jamais réglé sa liste.
    pub fn defaults() -> Self {
        Self(vec![
            Shortcut { label: "/simplify".into(), text: "/simplify".into() },
            Shortcut { label: "/code-review".into(), text: "/code-review".into() },
        ])
    }

    pub fn iter(&self) -> impl Iterator<Item = &Shortcut> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_is_trimmed_and_bounded() {
        let s = Shortcut::new("  Retours PR ", "\n prends en compte les retours de la PR\n").unwrap();
        assert_eq!((s.label(), s.text()), ("Retours PR", "prends en compte les retours de la PR"));
        assert_eq!(Shortcut::new(" ", "x"), Err(ShortcutError::EmptyLabel));
        assert_eq!(Shortcut::new("x", " \n"), Err(ShortcutError::EmptyText));
        assert_eq!(Shortcut::new(&"x".repeat(25), "x"), Err(ShortcutError::LabelTooLong));
        assert_eq!(Shortcut::new("a\nb", "x"), Err(ShortcutError::LabelTooLong));
        assert_eq!(Shortcut::new("x", &"x".repeat(4_001)), Err(ShortcutError::TextTooLong));
    }

    #[test]
    fn no_control_character_reaches_the_terminal_but_tab_and_newline() {
        assert!(Shortcut::new("Plan", "1. lire\n2.\técrire").is_ok());
        for bad in ["fin\x1b[201~\r", "a\rb", "\x03", "a\u{9b}b"] {
            assert_eq!(Shortcut::new("Piège", bad), Err(ShortcutError::Control("Piège".into())), "{bad:?}");
        }
    }

    #[test]
    fn a_lone_slash_command_is_pasted_ready_for_its_arguments() {
        assert_eq!(Shortcut::new("s", "/simplify").unwrap().pasted(), "/simplify ");
        assert_eq!(Shortcut::new("r", "/code-review high").unwrap().pasted(), "/code-review high");
        assert_eq!(Shortcut::new("p", "relis la PR").unwrap().pasted(), "relis la PR");
    }

    #[test]
    fn the_list_is_bounded() {
        let one = Shortcut::new("x", "y").unwrap();
        assert!(Shortcuts::new(vec![one.clone(); 20]).is_ok());
        assert_eq!(Shortcuts::new(vec![one; 21]), Err(ShortcutError::TooMany));
    }
}

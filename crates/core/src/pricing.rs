//! Coût estimé : tokens × grille tarifaire publique de l'API. Avec un abonnement, ce n'est pas ce
//! que je paie : l'interface montre les tokens d'abord et le coût ensuite, comme une estimation.

use crate::history::TokenUsage;

/// Relevé de la grille, à refaire quand un modèle apparaît sans prix.
pub const PRICES_AS_OF: &str = "2026-10-27";
pub const PRICES_SOURCE: &str = "https://platform.claude.com/docs/en/about-claude/pricing";

/// Prix en dollars par million de tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    pub output: f64,
}

const fn price(input: f64, cache_write_5m: f64, cache_write_1h: f64, cache_read: f64, output: f64) -> Price {
    Price { input, cache_write_5m, cache_write_1h, cache_read, output }
}

/// Grille standard du Claude API, par identifiant de modèle sans suffixe de date.
const PRICES: &[(&str, Price)] = &[
    ("claude-fable-5-1", price(10.0, 12.5, 20.0, 0.25, 50.0)),
    ("claude-mythos-5-1", price(10.0, 12.5, 20.0, 0.25, 50.0)),
    ("claude-fable-5", price(10.0, 12.5, 20.0, 1.0, 50.0)),
    ("claude-mythos-5", price(10.0, 12.5, 20.0, 1.0, 50.0)),
    ("claude-opus-5-5", price(4.0, 5.0, 8.0, 0.2, 20.0)),
    ("claude-opus-5", price(5.0, 6.25, 10.0, 0.5, 25.0)),
    ("claude-opus-4-8", price(5.0, 6.25, 10.0, 0.5, 25.0)),
    ("claude-opus-4-7", price(5.0, 6.25, 10.0, 0.5, 25.0)),
    ("claude-opus-4-6", price(5.0, 6.25, 10.0, 0.5, 25.0)),
    ("claude-opus-4-5", price(5.0, 6.25, 10.0, 0.5, 25.0)),
    ("claude-opus-4-1", price(15.0, 18.75, 30.0, 1.5, 75.0)),
    ("claude-opus-4", price(15.0, 18.75, 30.0, 1.5, 75.0)),
    ("claude-sonnet-5-5", price(2.0, 2.5, 4.0, 0.1, 10.0)),
    ("claude-sonnet-5", price(2.0, 2.5, 4.0, 0.2, 10.0)),
    ("claude-sonnet-4-6", price(3.0, 3.75, 6.0, 0.3, 15.0)),
    ("claude-sonnet-4-5", price(3.0, 3.75, 6.0, 0.3, 15.0)),
    ("claude-sonnet-4", price(3.0, 3.75, 6.0, 0.3, 15.0)),
    // Tarif des prompts de moins de 100 000 tokens : l'usage agrégé ne dit pas la taille de chaque prompt.
    ("claude-haiku-5-5", price(0.1, 0.125, 0.2, 0.01, 0.5)),
    ("claude-haiku-4-5", price(1.0, 1.25, 2.0, 0.1, 5.0)),
    ("claude-3-5-haiku", price(0.8, 1.0, 1.6, 0.08, 4.0)),
];

/// `None` pour un modèle absent de la grille : son coût n'est pas inventé.
pub fn price_of(model: &str) -> Option<Price> {
    let id = match model.rsplit_once('-') {
        Some((base, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => model,
    };
    PRICES.iter().find(|(m, _)| *m == id).map(|(_, p)| *p)
}

/// Coût en dollars d'un usage au prix donné.
pub fn cost(usage: &TokenUsage, p: &Price) -> f64 {
    let cache_write_5m = usage.cache_creation.saturating_sub(usage.cache_creation_1h);
    let dollars = usage.input as f64 * p.input
        + cache_write_5m as f64 * p.cache_write_5m
        + usage.cache_creation_1h as f64 * p.cache_write_1h
        + usage.cache_read as f64 * p.cache_read
        + usage.output as f64 * p.output;
    dollars / 1_000_000.0
}

/// Coût estimé d'un usage ventilé par modèle, et tokens des modèles sans prix connu.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Estimate {
    pub dollars: f64,
    pub unpriced_tokens: u64,
}

impl Estimate {
    pub fn of<'a>(by_model: impl IntoIterator<Item = (&'a str, &'a TokenUsage)>) -> Self {
        let mut out = Self::default();
        for (model, usage) in by_model {
            match price_of(model) {
                Some(p) => out.dollars += cost(usage, &p),
                None => out.unpriced_tokens += usage.total(),
            }
        }
        out
    }

    pub fn add(&mut self, other: Estimate) {
        self.dollars += other.dollars;
        self.unpriced_tokens += other.unpriced_tokens;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dated_and_undated_ids_without_confusing_neighbours() {
        assert_eq!(price_of("claude-haiku-4-5-20251001").map(|p| p.output), Some(5.0));
        assert_eq!(price_of("claude-opus-4-20250514").map(|p| p.input), Some(15.0));
        assert_eq!(price_of("claude-opus-4-8").map(|p| p.input), Some(5.0));
        assert_eq!(price_of("<synthetic>"), None);
    }

    #[test]
    fn prices_each_kind_of_token_and_keeps_unknown_models_apart() {
        let usage = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_creation: 3_000_000,
            cache_creation_1h: 1_000_000,
        };
        // Opus 5.5 : 4 + 20 + 0,20 + 2 × 5 + 8.
        assert!((cost(&usage, &price_of("claude-opus-5-5").unwrap()) - 42.2).abs() < 1e-9);
        let e = Estimate::of([("claude-opus-5-5", &usage), ("inconnu", &usage)]);
        assert!((e.dollars - 42.2).abs() < 1e-9);
        assert_eq!(e.unpriced_tokens, usage.total());
    }
}

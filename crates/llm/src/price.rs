//! Cost per call for the logs (spec §23, §25).

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

/// Anthropic base prices, checked against
/// platform.claude.com/docs/en/about-claude/pricing on 2026-11-08
/// (Haiku 5.5 for prompts up to 100,000 tokens; ours stay under 1,000).
const ANTHROPIC: [(&str, Price); 3] = [
    ("claude-haiku-5-5", Price::new(0.10, 0.50)),
    ("claude-sonnet-5-5", Price::new(2.0, 10.0)),
    ("claude-opus-5-5", Price::new(4.0, 20.0)),
];

impl Price {
    pub const FREE: Price = Price::new(0.0, 0.0);

    pub const fn new(input_per_mtok: f64, output_per_mtok: f64) -> Self {
        Self {
            input_per_mtok,
            output_per_mtok,
        }
    }

    /// The listed price of an Anthropic model, if it is in the table.
    pub fn anthropic(model: &str) -> Option<Price> {
        ANTHROPIC.iter().find(|(m, _)| *m == model).map(|(_, p)| *p)
    }

    /// Dollars for one call.
    pub fn cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (f64::from(input_tokens) * self.input_per_mtok
            + f64::from(output_tokens) * self.output_per_mtok)
            / 1e6
    }
}

//! What LLM use cost so far, for the log row (spec §25: calls, tokens, cost).

use crate::Reply;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LlmStats {
    /// Replies received, from the provider or the replay cache.
    pub calls: u32,
    pub cached: u32,
    /// Times the code tool ran instead (budget, error or unusable reply).
    pub fallbacks: u32,
    /// Replies that could not be used.
    pub unparsed: u32,
    /// Spent on calls that went out (cached replies are free).
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

impl LlmStats {
    /// Count one reply.
    pub fn record(&mut self, r: &Reply) {
        self.calls += 1;
        if r.cached {
            self.cached += 1;
        } else {
            self.input_tokens += u64::from(r.input_tokens);
            self.output_tokens += u64::from(r.output_tokens);
            self.cost_usd += r.cost_usd;
        }
    }
}

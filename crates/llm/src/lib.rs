//! LLM layer (spec §23, #37): one provider-agnostic interface (Ollama /
//! OpenAI-compatible and Anthropic) behind three spend tiers. The LLM is a tool
//! that internal actions call (#40); it never picks actions for arm C.
//!
//! The parts: [`Provider`] (one call), [`anthropic`] and [`openai`] (the two
//! wire formats), [`Cached`] (replay by prompt hash), [`BudgetGuard`] (the
//! hourly call cap) and [`Price`] (cost per call for the logs).

pub mod anthropic;
mod budget;
mod cache;
pub mod describe;
mod http;
pub mod openai;
mod price;
mod stats;

pub use budget::BudgetGuard;
pub use cache::{CacheMode, Cached, prompt_key};
pub use price::Price;
pub use stats::LlmStats;

use serde::{Deserialize, Serialize};

/// Spend tiers (#37). Arms in one comparison always share a tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Free local model on Michael's PC (12 GB VRAM).
    Local,
    /// Up to about $1/h per bot.
    Cheap,
    /// Up to about $5/h, only if Cheap is shown to fail.
    Strong,
}

/// Limits that hold for every call (spec §23).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LlmParams {
    /// Prompts stay under this many input tokens.
    pub max_input_tokens: u32,
    /// Most tool calls per hour on T-cheap: 1 Hz, about $0.90/h on the
    /// default model at 1,000 tokens in and 300 out (proposed until the pilot).
    pub cheap_calls_per_hour: u32,
    /// Most tool calls per hour on T-strong: about $5/h on the default model
    /// at the same sizes (proposed until the pilot).
    pub strong_calls_per_hour: u32,
    /// Cap on each reply, thinking included (proposed until the pilot).
    pub max_reply_tokens: u32,
    /// Seconds per world tick, for the hourly cap (Minecraft: 20 ticks/s).
    pub seconds_per_tick: f64,
}

impl Default for LlmParams {
    fn default() -> Self {
        Self {
            max_input_tokens: 1000,
            cheap_calls_per_hour: 3600,
            strong_calls_per_hour: 1000,
            max_reply_tokens: 1000,
            seconds_per_tick: 0.05,
        }
    }
}

impl LlmParams {
    /// The hourly call cap for a tier (spec §23); none on T-local.
    pub fn calls_per_hour(&self, tier: Tier) -> Option<u32> {
        match tier {
            Tier::Local => None,
            Tier::Cheap => Some(self.cheap_calls_per_hour),
            Tier::Strong => Some(self.strong_calls_per_hour),
        }
    }

    /// World time in seconds at `tick`, for the budget guard.
    pub fn seconds_at(&self, tick: u64) -> f64 {
        tick as f64 * self.seconds_per_tick
    }
}

/// One request: a fixed system part and the per-call user part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
    /// Cap on the reply, thinking included.
    pub max_tokens: u32,
}

impl Prompt {
    /// Rough input size: four characters a token (industry norm for
    /// English). Used to keep prompts under `max_input_tokens` before
    /// sending; the provider's own count is what gets logged.
    pub fn estimated_tokens(&self) -> u32 {
        let chars = self.system.chars().count() + self.user.chars().count();
        chars.div_ceil(4) as u32
    }
}

/// One answer, with what it cost (spec §23: tokens and cost per call).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub text: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
    /// True when it came from the replay cache, not the provider.
    #[serde(default)]
    pub cached: bool,
}

/// Why a call gave no reply. Every caller falls back to its code tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// No connection, timeout or unreadable body.
    Transport(String),
    /// The provider answered with an error status.
    Status(u16, String),
    /// The answer was not in the expected shape.
    Parse(String),
    /// The model declined (Anthropic `stop_reason: "refusal"`).
    Refused,
    /// Replay mode and the prompt is not in the cache.
    NotCached,
}

/// A model behind some API (spec §23).
pub trait Provider {
    /// "provider/model", part of the cache key and the logs.
    fn id(&self) -> String;
    /// Send one prompt and wait for the answer.
    fn complete(&mut self, prompt: &Prompt) -> Result<Reply, LlmError>;
}

//! The Anthropic Messages API over raw HTTP (spec §23; Rust has no
//! official SDK). Used for T-cheap and T-strong.

use serde_json::{Value, json};

use crate::http::{agent, count, post};
use crate::{LlmError, Price, Prompt, Provider, Reply};

/// The API version this client is written against.
pub const API_VERSION: &str = "2026-10-21";
/// Beta that lets the server retry a refused request on another model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Which model and how hard it thinks (proposed until the pilot).
#[derive(Debug, Clone, PartialEq)]
pub struct AnthropicConfig {
    pub base_url: String,
    pub model: String,
    /// `output_config.effort`: "low" to "max". Low keeps each call cheap.
    pub effort: String,
    /// Ask the server to retry a refusal on another model (not on Haiku).
    pub fallbacks: bool,
}

impl AnthropicConfig {
    /// T-cheap: Claude Haiku 5.5.
    pub fn cheap() -> Self {
        Self::new("claude-haiku-5-5", false)
    }

    /// T-strong: Claude Sonnet 5.5, which fits about 1,000 calls in $5/h.
    pub fn strong() -> Self {
        Self::new("claude-sonnet-5-5", true)
    }

    fn new(model: &str, fallbacks: bool) -> Self {
        Self {
            base_url: "https://api.anthropic.com".into(),
            model: model.into(),
            effort: "low".into(),
            fallbacks,
        }
    }
}

pub struct Anthropic {
    cfg: AnthropicConfig,
    key: String,
    agent: ureq::Agent,
}

impl Anthropic {
    /// `key` comes from the local `.env` (never committed).
    pub fn new(cfg: AnthropicConfig, key: String) -> Self {
        Self {
            cfg,
            key,
            agent: agent(),
        }
    }

    /// The request body for one prompt.
    pub fn body(&self, p: &Prompt) -> Value {
        let mut body = json!({
            "model": self.cfg.model,
            "max_tokens": p.max_tokens,
            "system": p.system,
            "messages": [{"role": "user", "content": p.user}],
            "output_config": {"effort": self.cfg.effort},
        });
        if self.cfg.fallbacks {
            body["fallbacks"] = json!("default");
        }
        body
    }
}

/// Read text and usage from a Messages API answer; a refusal is an error.
pub fn parse(v: &Value, model: &str) -> Result<Reply, LlmError> {
    if v["stop_reason"] == "refusal" {
        return Err(LlmError::Refused);
    }
    let blocks = v["content"]
        .as_array()
        .ok_or_else(|| LlmError::Parse("missing content".into()))?;
    let text: String = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    let usage = &v["usage"];
    let (input, output) = (
        count(usage, "input_tokens")?,
        count(usage, "output_tokens")?,
    );
    // A fallback may answer with another model; price the one that ran.
    let ran = v["model"].as_str().unwrap_or(model);
    let price = Price::anthropic(ran).unwrap_or(Price::FREE);
    Ok(Reply {
        text,
        input_tokens: input,
        output_tokens: output,
        cost_usd: price.cost(input, output),
        cached: false,
    })
}

impl Provider for Anthropic {
    fn id(&self) -> String {
        format!("anthropic/{}", self.cfg.model)
    }

    fn complete(&mut self, prompt: &Prompt) -> Result<Reply, LlmError> {
        let url = format!("{}/v1/messages", self.cfg.base_url);
        let mut headers = vec![
            ("x-api-key", self.key.as_str()),
            ("anthropic-version", API_VERSION),
        ];
        if self.cfg.fallbacks {
            headers.push(("anthropic-beta", FALLBACK_BETA));
        }
        let v = post(&self.agent, &url, &headers, &self.body(prompt))?;
        parse(&v, &self.cfg.model)
    }
}

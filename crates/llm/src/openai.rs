//! OpenAI-compatible chat completions (spec §23), as served by Ollama on
//! Michael's PC for T-local.

use serde_json::{Value, json};

use crate::http::{agent, count, post};
use crate::{LlmError, Price, Prompt, Provider, Reply};

/// Where the server is and which model to load (picked in the pilot).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub model: String,
    /// Bearer key, for hosted servers; Ollama needs none.
    pub key: Option<String>,
    pub price: Price,
}

impl OpenAiConfig {
    /// T-local: Ollama's default port. The model is a placeholder that fits
    /// 12 GB of VRAM at 4-bit until the pilot picks one.
    pub fn local() -> Self {
        Self {
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5:7b-instruct".into(),
            key: None,
            price: Price::FREE,
        }
    }
}

pub struct OpenAi {
    cfg: OpenAiConfig,
    agent: ureq::Agent,
}

impl OpenAi {
    pub fn new(cfg: OpenAiConfig) -> Self {
        Self {
            cfg,
            agent: agent(),
        }
    }

    /// The request body for one prompt. Temperature 0 keeps runs as
    /// repeatable as the server allows; the replay cache does the rest.
    pub fn body(&self, p: &Prompt) -> Value {
        json!({
            "model": self.cfg.model,
            "max_tokens": p.max_tokens,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": p.system},
                {"role": "user", "content": p.user},
            ],
        })
    }
}

/// Read text and usage from a chat completion.
pub fn parse(v: &Value, price: &Price) -> Result<Reply, LlmError> {
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| LlmError::Parse("missing choices[0].message.content".into()))?;
    let usage = &v["usage"];
    let (input, output) = (
        count(usage, "prompt_tokens")?,
        count(usage, "completion_tokens")?,
    );
    Ok(Reply {
        text: text.into(),
        input_tokens: input,
        output_tokens: output,
        cost_usd: price.cost(input, output),
        cached: false,
    })
}

impl Provider for OpenAi {
    fn id(&self) -> String {
        format!("openai/{}", self.cfg.model)
    }

    fn complete(&mut self, prompt: &Prompt) -> Result<Reply, LlmError> {
        let url = format!("{}/v1/chat/completions", self.cfg.base_url);
        let bearer = self.cfg.key.as_ref().map(|k| format!("Bearer {k}"));
        let headers: Vec<(&str, &str)> = bearer
            .iter()
            .map(|b| ("authorization", b.as_str()))
            .collect();
        let v = post(&self.agent, &url, &headers, &self.body(prompt))?;
        parse(&v, &self.cfg.price)
    }
}

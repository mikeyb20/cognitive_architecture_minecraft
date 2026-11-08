//! LLM layer (spec §23, #37): one provider-agnostic interface (Ollama /
//! OpenAI-compatible and Anthropic) behind three spend tiers. The LLM is a tool
//! that internal actions call (#40); it never picks actions for arm C.

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

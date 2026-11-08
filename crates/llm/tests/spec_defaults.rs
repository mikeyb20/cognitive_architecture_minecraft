// Spec §23 values, pinned. Change one only with the spec.

use llm::LlmParams;
use llm::anthropic::AnthropicConfig;

#[test]
fn llm_defaults() {
    let p = LlmParams::default();
    assert_eq!(
        p.max_input_tokens, 1000,
        "spec §23: input under 1,000 tokens"
    );
    assert_eq!(p.cheap_calls_per_hour, 3600, "≈$0.90/h on Haiku 5.5");
    assert_eq!(p.strong_calls_per_hour, 1000, "≈$5/h on Sonnet 5.5");
    let (c, s) = (AnthropicConfig::cheap(), AnthropicConfig::strong());
    assert_eq!(
        (c.model.as_str(), c.effort.as_str(), c.fallbacks),
        ("claude-haiku-5-5", "low", false)
    );
    assert_eq!(
        (s.model.as_str(), s.effort.as_str(), s.fallbacks),
        ("claude-sonnet-5-5", "low", true)
    );
    assert_eq!(c.base_url, "https://api.anthropic.com");
}

// The Anthropic client against a local stand-in server (spec §23).

mod common;

use common::{prompt, serve};
use llm::anthropic::{API_VERSION, Anthropic, AnthropicConfig, parse};
use llm::{LlmError, Provider};
use serde_json::json;

const OK: &str = r#"{"model":"claude-haiku-5-5","stop_reason":"end_turn",
 "content":[{"type":"thinking","thinking":""},{"type":"text","text":"3"},{"type":"text","text":"!"}],
 "usage":{"input_tokens":1000,"output_tokens":300}}"#;

fn client(url: &str, cfg: AnthropicConfig) -> Anthropic {
    Anthropic::new(
        AnthropicConfig {
            base_url: url.into(),
            ..cfg
        },
        "sk-test".into(),
    )
}

#[test]
fn cheap_call_sends_the_messages_shape_and_prices_the_reply() {
    let (url, seen) = serve(vec![(200, OK.into())]);
    let mut c = client(&url, AnthropicConfig::cheap());
    assert_eq!(c.id(), "anthropic/claude-haiku-5-5");
    let r = c.complete(&prompt("pick")).unwrap();
    assert_eq!(r.text, "3!", "text blocks only, joined");
    assert_eq!((r.input_tokens, r.output_tokens), (1000, 300));
    // 1000 × $0.10 + 300 × $0.50 per million = $0.00025.
    assert!((r.cost_usd - 0.000_25).abs() < 1e-12);
    assert!(!r.cached);
    let s = seen.recv().unwrap();
    assert_eq!(s.line, "POST /v1/messages HTTP/1.1");
    assert_eq!(s.header("x-api-key"), Some("sk-test"));
    assert_eq!(s.header("anthropic-version"), Some(API_VERSION));
    assert_eq!(s.header("content-type"), Some("application/json"));
    assert_eq!(s.header("anthropic-beta"), None, "no fallback on Haiku");
    assert_eq!(
        s.body,
        json!({
            "model": "claude-haiku-5-5",
            "max_tokens": 2000,
            "system": "You pick one action.",
            "messages": [{"role": "user", "content": "pick"}],
            "output_config": {"effort": "low"},
        })
    );
}

#[test]
fn strong_tier_asks_for_server_fallbacks_and_prices_the_model_that_ran() {
    let ran = OK.replace("claude-haiku-5-5", "claude-opus-5-5");
    let (url, seen) = serve(vec![(200, ran)]);
    let mut c = client(&url, AnthropicConfig::strong());
    let r = c.complete(&prompt("pick")).unwrap();
    // 1000 × $4 + 300 × $20 per million = $0.01.
    assert!((r.cost_usd - 0.01).abs() < 1e-12);
    let s = seen.recv().unwrap();
    assert_eq!(
        s.header("anthropic-beta"),
        Some("server-side-fallback-2026-07-01")
    );
    assert_eq!(s.body["fallbacks"], "default");
    assert_eq!(s.body["model"], "claude-sonnet-5-5");
}

#[test]
fn errors_refusals_and_bad_shapes_are_errors() {
    let (url, _seen) = serve(vec![(429, "slow down".into())]);
    let mut c = client(&url, AnthropicConfig::cheap());
    assert_eq!(
        c.complete(&prompt("x")),
        Err(LlmError::Status(429, "slow down".into()))
    );
    let refused = json!({"stop_reason": "refusal", "content": []});
    assert_eq!(parse(&refused, "m"), Err(LlmError::Refused));
    assert!(matches!(parse(&json!({}), "m"), Err(LlmError::Parse(_))));
    let no_usage = json!({"content": [], "usage": {"input_tokens": 1}});
    assert_eq!(
        parse(&no_usage, "m"),
        Err(LlmError::Parse("missing output_tokens".into()))
    );
}

#[test]
fn an_unknown_model_costs_nothing_and_a_missing_model_uses_the_asked_one() {
    let v = json!({"content": [], "usage": {"input_tokens": 10, "output_tokens": 10}});
    // 10 × $2 + 10 × $10 per million = $0.00012.
    let sonnet = parse(&v, "claude-sonnet-5-5").unwrap().cost_usd;
    assert!((sonnet - 0.000_12).abs() < 1e-12);
    assert_eq!(parse(&v, "someone-else").unwrap().cost_usd, 0.0);
}

#[test]
fn bad_json_and_no_server_are_errors() {
    let (url, _seen) = serve(vec![(200, "not json".into())]);
    let mut c = client(&url, AnthropicConfig::cheap());
    assert!(matches!(c.complete(&prompt("x")), Err(LlmError::Parse(_))));
    let mut gone = client("http://127.0.0.1:9", AnthropicConfig::cheap());
    assert!(matches!(
        gone.complete(&prompt("x")),
        Err(LlmError::Transport(_))
    ));
}

// The OpenAI-compatible client (Ollama, T-local) against a stand-in server.

mod common;

use common::{prompt, serve};
use llm::openai::{OpenAi, OpenAiConfig, parse};
use llm::{LlmError, Price, Provider};
use serde_json::json;

const OK: &str = r#"{"choices":[{"message":{"role":"assistant","content":"7"}}],
 "usage":{"prompt_tokens":800,"completion_tokens":5}}"#;

#[test]
fn local_call_sends_chat_completions_and_costs_nothing() {
    let (url, seen) = serve(vec![(200, OK.into())]);
    let mut c = OpenAi::new(OpenAiConfig {
        base_url: url,
        ..OpenAiConfig::local()
    });
    assert_eq!(c.id(), "openai/qwen2.5:7b-instruct");
    let r = c.complete(&prompt("pick")).unwrap();
    assert_eq!(
        (r.text.as_str(), r.input_tokens, r.output_tokens),
        ("7", 800, 5)
    );
    assert_eq!(r.cost_usd, 0.0);
    let s = seen.recv().unwrap();
    assert_eq!(s.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(s.header("authorization"), None);
    assert_eq!(
        s.body,
        json!({
            "model": "qwen2.5:7b-instruct",
            "max_tokens": 2000,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": "You pick one action."},
                {"role": "user", "content": "pick"},
            ],
        })
    );
}

#[test]
fn a_hosted_server_gets_the_key_and_its_price() {
    let (url, seen) = serve(vec![(200, OK.into())]);
    let mut c = OpenAi::new(OpenAiConfig {
        base_url: url,
        model: "m".into(),
        key: Some("k1".into()),
        price: Price::new(1.0, 2.0),
    });
    let r = c.complete(&prompt("pick")).unwrap();
    // 800 × $1 + 5 × $2 per million = $0.00081.
    assert!((r.cost_usd - 0.000_81).abs() < 1e-12);
    assert_eq!(
        seen.recv().unwrap().header("authorization"),
        Some("Bearer k1")
    );
}

#[test]
fn bad_shapes_are_parse_errors() {
    let free = Price::FREE;
    assert!(matches!(parse(&json!({}), &free), Err(LlmError::Parse(_))));
    let no_usage = json!({"choices": [{"message": {"content": "x"}}]});
    assert_eq!(
        parse(&no_usage, &free),
        Err(LlmError::Parse("missing prompt_tokens".into()))
    );
}

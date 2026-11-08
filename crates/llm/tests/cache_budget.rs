// Replay cache, budget guard, prices and prompt size (spec §23).

mod common;

use std::collections::VecDeque;

use common::prompt;
use llm::{
    BudgetGuard, CacheMode, Cached, LlmError, LlmParams, Price, Prompt, Provider, Reply, Tier,
    prompt_key,
};

/// Answers from a list and counts calls.
struct Script {
    replies: VecDeque<Result<Reply, LlmError>>,
    calls: usize,
}

impl Provider for Script {
    fn id(&self) -> String {
        "script/m".into()
    }
    fn complete(&mut self, _: &Prompt) -> Result<Reply, LlmError> {
        self.calls += 1;
        self.replies.pop_front().unwrap()
    }
}

fn reply(text: &str) -> Reply {
    Reply {
        text: text.into(),
        input_tokens: 10,
        output_tokens: 2,
        cost_usd: 0.5,
        cached: false,
    }
}

fn script(replies: Vec<Result<Reply, LlmError>>) -> Script {
    Script {
        replies: replies.into(),
        calls: 0,
    }
}

#[test]
fn record_calls_once_then_replays_from_memory() {
    let mut c = Cached::in_memory(
        script(vec![Ok(reply("a")), Ok(reply("b"))]),
        CacheMode::Record,
    );
    assert!(c.is_empty());
    assert_eq!(c.complete(&prompt("p")).unwrap(), reply("a"));
    let again = c.complete(&prompt("p")).unwrap();
    assert_eq!((again.text.as_str(), again.cached), ("a", true));
    assert_eq!((c.len(), c.is_empty()), (1, false));
    c.complete(&prompt("q")).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c.id(), "script/m");
}

#[test]
fn replay_never_calls_and_errors_are_not_stored() {
    let mut r = Cached::in_memory(script(vec![]), CacheMode::Replay);
    assert_eq!(r.complete(&prompt("p")), Err(LlmError::NotCached));
    let mut c = Cached::in_memory(
        script(vec![Err(LlmError::Refused), Ok(reply("b"))]),
        CacheMode::Record,
    );
    assert_eq!(c.complete(&prompt("p")), Err(LlmError::Refused));
    assert!(c.is_empty());
    assert_eq!(c.complete(&prompt("p")).unwrap().text, "b");
}

#[test]
fn a_cache_file_survives_a_restart_and_skips_torn_lines() {
    let dir = std::env::temp_dir().join(format!("llm-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cache.jsonl");
    let _ = std::fs::remove_file(&file);
    let mut c = Cached::open(
        script(vec![Ok(reply("a"))]),
        CacheMode::Record,
        file.clone(),
    )
    .unwrap();
    c.complete(&prompt("p")).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"key\":\"tor"))
        .unwrap();
    let mut r = Cached::open(script(vec![]), CacheMode::Replay, file.clone()).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r.complete(&prompt("p")).unwrap().text, "a");
    assert!(
        Cached::open(script(vec![]), CacheMode::Replay, dir.clone()).is_err(),
        "a directory"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_key_covers_every_part_and_is_stable() {
    let p = prompt("p");
    let k = prompt_key("a/m", &p);
    assert_eq!(k, prompt_key("a/m", &p.clone()));
    assert_ne!(k, prompt_key("b/m", &p));
    assert_ne!(
        k,
        prompt_key(
            "a/m",
            &Prompt {
                system: "s".into(),
                ..p.clone()
            }
        )
    );
    assert_ne!(k, prompt_key("a/m", &prompt("q")));
    assert_ne!(
        k,
        prompt_key(
            "a/m",
            &Prompt {
                max_tokens: 1,
                ..p.clone()
            }
        )
    );
    // Moving text across the separator changes the key.
    let a = Prompt {
        system: "ab".into(),
        user: "c".into(),
        max_tokens: 1,
    };
    let b = Prompt {
        system: "a".into(),
        user: "bc".into(),
        max_tokens: 1,
    };
    assert_ne!(prompt_key("x", &a), prompt_key("x", &b));
    // FNV-1a 64 of the empty string, then the 0xff separator, by hand.
    let e = Prompt {
        system: String::new(),
        user: String::new(),
        max_tokens: 0,
    };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in [0xff, 0xff, 0xff, b'0', 0xff] {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    assert_eq!(prompt_key("", &e), h);
}

#[test]
fn the_guard_allows_cap_calls_in_any_hour() {
    let mut g = BudgetGuard::new(Some(2));
    assert!(g.try_spend(0.0));
    assert!(g.try_spend(10.0));
    assert!(!g.try_spend(3599.0), "third call inside the hour");
    assert_eq!(g.used(), 2);
    assert!(g.try_spend(3600.0), "the first call has aged out");
    assert!(!g.try_spend(3609.0));
    assert!(g.try_spend(3610.0));
    let mut free = BudgetGuard::new(None);
    assert!((0..10_000).all(|t| free.try_spend(f64::from(t))));
}

#[test]
fn tiers_prices_and_prompt_size() {
    let p = LlmParams::default();
    assert_eq!(p.calls_per_hour(Tier::Local), None);
    assert_eq!(p.calls_per_hour(Tier::Cheap), Some(3600));
    assert_eq!(p.calls_per_hour(Tier::Strong), Some(1000));
    assert_eq!(
        Price::anthropic("claude-haiku-5-5"),
        Some(Price::new(0.10, 0.50))
    );
    assert_eq!(
        Price::anthropic("claude-opus-5-5"),
        Some(Price::new(4.0, 20.0))
    );
    assert_eq!(Price::anthropic("gpt"), None);
    assert_eq!(Price::new(2.0, 10.0).cost(1_000_000, 100_000), 3.0);
    let q = Prompt {
        system: "abcd".into(),
        user: "é".into(),
        max_tokens: 1,
    };
    assert_eq!(q.estimated_tokens(), 2, "five characters round up");
}

#[test]
fn a_boxed_provider_passes_calls_through() {
    let mut b: Box<dyn Provider> = Box::new(script(vec![Ok(reply("z"))]));
    assert_eq!(b.id(), "script/m");
    assert_eq!(b.complete(&prompt("p")).unwrap().text, "z");
}

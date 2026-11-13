// Picking the arm and LLM for a Minecraft run, and the log row (spec §23–§26).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use host_api::{AgentId, Candidate, Inventory, Observation, Outcome, Pos, Skill, Target, TaskType};
use llm::{LlmError, LlmStats, Prompt, Provider, Reply, Tier};
use runner::mc::{McArm, McStep, arm, provider, row, tier};
use serde_json::json;

#[test]
fn arm_and_tier_names() {
    assert_eq!(McArm::parse("a"), Some(McArm::A));
    assert_eq!(McArm::parse("b"), Some(McArm::B));
    assert_eq!(McArm::parse("bn"), Some(McArm::BN));
    assert_eq!(McArm::parse("c"), None);
    assert!(!McArm::A.needs_llm());
    assert!(McArm::B.needs_llm() && McArm::BN.needs_llm());
    assert_eq!(tier("local"), Some(Tier::Local));
    assert_eq!(tier("cheap"), Some(Tier::Cheap));
    assert_eq!(tier("strong"), Some(Tier::Strong));
    assert_eq!(tier("free"), None);
}

/// A one-reply chat-completions server; returns its URL.
fn ollama(text: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let body = json!({"choices": [{"message": {"content": text}}],
                      "usage": {"prompt_tokens": 1, "completion_tokens": 1}})
    .to_string();
    thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut h = String::new();
            r.read_line(&mut h).unwrap();
            if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
            if h.trim().is_empty() {
                break;
            }
        }
        r.read_exact(&mut vec![0; len]).unwrap();
        let mut w = s;
        write!(
            w,
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    url
}

fn ask(p: &mut Box<dyn Provider>) -> Result<Reply, LlmError> {
    p.complete(&Prompt {
        system: "s".into(),
        user: "u".into(),
        max_tokens: 5,
    })
}

#[test]
fn providers_by_tier() {
    let mut local = provider(Tier::Local, Some(ollama("7")), None, None).unwrap();
    assert_eq!(local.id(), "openai/qwen2.5:7b-instruct");
    assert_eq!(
        ask(&mut local).unwrap().text,
        "7",
        "the URL override is used"
    );
    let cheap = provider(Tier::Cheap, None, Some("k".into()), None).unwrap();
    assert_eq!(cheap.id(), "anthropic/claude-haiku-5-5");
    let strong = provider(Tier::Strong, None, Some("k".into()), None).unwrap();
    assert_eq!(strong.id(), "anthropic/claude-sonnet-5-5");
    let no_key = provider(Tier::Cheap, None, None, None).err();
    assert_eq!(no_key.as_deref(), Some("ANTHROPIC_API_KEY is not set"));
}

#[test]
fn the_cache_records_or_replays() {
    let dir = std::env::temp_dir().join(format!("mc-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("c.jsonl");
    let _ = std::fs::remove_file(&file);
    let url = Some(ollama("3"));
    let mut rec = provider(Tier::Local, url, None, Some((file.clone(), false))).unwrap();
    assert_eq!(ask(&mut rec).unwrap().text, "3");
    let mut rep = provider(Tier::Local, None, None, Some((file.clone(), true))).unwrap();
    assert!(ask(&mut rep).unwrap().cached, "replayed, no server needed");
    let other = dir.join("empty.jsonl");
    let mut miss = provider(Tier::Local, None, None, Some((other, true))).unwrap();
    assert_eq!(ask(&mut miss), Err(LlmError::NotCached));
    let bad = provider(Tier::Local, None, None, Some((dir.clone(), true))).err();
    assert!(bad.unwrap().starts_with("cannot open the cache"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Replies "0" and keeps every prompt.
struct Zero(Arc<Mutex<Vec<Prompt>>>);

impl Provider for Zero {
    fn id(&self) -> String {
        "zero/m".into()
    }
    fn complete(&mut self, p: &Prompt) -> Result<Reply, LlmError> {
        self.0.lock().unwrap().push(p.clone());
        Ok(Reply {
            text: "0".into(),
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: 0.0,
            cached: false,
        })
    }
}

fn obs() -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 5,
        day_phase: 0.25,
        health: 1.0,
        food: 0.5,
        position: Pos { x: 1, y: 2, z: 3 },
        inventory: Inventory::default(),
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn cand() -> Candidate {
    Candidate {
        id: 0,
        skill: Skill::Explore,
        target: Some(Target::Pos(Pos { x: 0, y: 0, z: 0 })),
        task_type: TaskType::Travel,
        est_cost: 0.5,
        est_risk: 0.0,
        commanded: false,
    }
}

#[test]
fn arms_get_their_llm() {
    let mut a = arm(McArm::A, Tier::Local, None).unwrap();
    assert_eq!(a.choose(&obs(), &[cand()]), Some(0));
    assert!(a.llm_stats().is_none());
    assert_eq!(
        arm(McArm::B, Tier::Local, None).err().as_deref(),
        Some("arms B and B+N need an LLM")
    );
    for (which, ranked) in [(McArm::B, false), (McArm::BN, true)] {
        let sent = Arc::new(Mutex::new(vec![]));
        let p: Box<dyn Provider> = Box::new(Zero(sent.clone()));
        let mut b = arm(which, Tier::Local, Some(p)).unwrap();
        assert_eq!(b.choose(&obs(), &[cand()]), Some(0));
        assert_eq!(b.llm_stats().unwrap().calls, 1);
        let system = sent.lock().unwrap()[0].system.clone();
        assert_eq!(system.contains("Options are ranked"), ranked, "{which:?}");
    }
}

#[test]
fn the_tier_sets_the_call_cap() {
    let p: Box<dyn Provider> = Box::new(Zero(Arc::new(Mutex::new(vec![]))));
    let mut b = arm(McArm::B, Tier::Strong, Some(p)).unwrap();
    for _ in 0..1001 {
        b.choose(&obs(), &[cand()]);
    }
    assert_eq!(
        b.llm_stats().unwrap().calls,
        1000,
        "strong: 1000 calls an hour"
    );
}

/// A bot that is ready, always sees `obs()` and finishes every action.
fn fake_bot() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        for line in BufReader::new(s).lines() {
            let req: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            let reply = match req["op"].as_str().unwrap() {
                "hello" => json!({"reply": "ready", "names": []}),
                "perceive" => json!({"reply": "observed", "obs": obs()}),
                "execute" => json!({"reply": "done", "outcome": {
                    "success": true, "progress": 1.0, "items_gained": {"items": []},
                    "damage": 0.0, "new_things": [], "blocked": false, "duration_ticks": 20}}),
                _ => json!({"reply": "said"}),
            };
            writeln!(w, "{reply}").unwrap();
        }
    });
    addr
}

fn mc_cli(args: &[&str], ollama_url: Option<String>) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_runner"));
    cmd.arg("mc").args(args).env_remove("ANTHROPIC_API_KEY");
    if let Some(url) = ollama_url {
        cmd.env("OLLAMA_BASE_URL", url);
    }
    cmd.output().unwrap()
}

/// One arm-B decision through the CLI against a fake bot and LLM; the log row.
fn one_b_decision(
    log: &std::path::Path,
    cache: &std::path::Path,
    replay: bool,
) -> serde_json::Value {
    let (addr, log, cache) = (fake_bot(), log.to_str().unwrap(), cache.to_str().unwrap());
    let mut args = vec![
        "b",
        "--addr",
        &addr,
        "--decisions",
        "1",
        "--log",
        log,
        "--cache",
        cache,
    ];
    if replay {
        args.push("--replay");
    }
    let out = mc_cli(&args, Some(ollama("0")));
    assert!(out.status.success(), "{out:?}");
    serde_json::from_str(&std::fs::read_to_string(log).unwrap()).unwrap()
}

#[test]
fn the_mc_command_logs_each_decision() {
    let dir = std::env::temp_dir().join(format!("mc-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("a.jsonl");
    let addr = fake_bot();
    let args = [
        "a",
        "--addr",
        &addr,
        "--decisions",
        "2",
        "--log",
        log.to_str().unwrap(),
    ];
    let out = mc_cli(&args, None);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 2);

    // --replay with an empty cache: no call reaches the LLM server.
    let cache = dir.join("cache.jsonl");
    std::fs::write(&cache, "").unwrap();
    assert_eq!(one_b_decision(&log, &cache, true)["llm"]["calls"], 0);
    // Without it the reply is fetched and recorded.
    assert_eq!(one_b_decision(&log, &cache, false)["llm"]["calls"], 1);
    assert_eq!(std::fs::read_to_string(&cache).unwrap().lines().count(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_mc_command_explains_bad_input() {
    let err = |args: &[&str]| {
        let out = mc_cli(args, None);
        assert_eq!(out.status.code(), Some(2));
        String::from_utf8(out.stderr).unwrap()
    };
    assert!(err(&["x"]).starts_with("usage: runner mc"));
    assert!(err(&["b", "--tier", "free"]).starts_with("--tier is local"));
    assert!(err(&["b", "--tier", "cheap"]).starts_with("ANTHROPIC_API_KEY"));
    let gone = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = gone.local_addr().unwrap().to_string();
    drop(gone);
    assert!(err(&["a", "--addr", &addr]).starts_with("no bot at"));
}

#[test]
fn a_log_row_by_hand() {
    let outcome = Outcome {
        success: true,
        progress: 1.0,
        items_gained: Inventory::default(),
        damage: 0.0,
        new_things: vec![],
        blocked: false,
        duration_ticks: 20,
    };
    let stats = LlmStats {
        calls: 2,
        cached: 1,
        fallbacks: 3,
        unparsed: 4,
        input_tokens: 50,
        output_tokens: 6,
        cost_usd: 0.25,
    };
    let step = McStep {
        obs: obs(),
        chosen: Some(cand()),
        reflex: true,
        outcome: Some(outcome.clone()),
        llm: Some(stats),
    };
    assert_eq!(
        row(&step),
        json!({
            "tick": 5, "health": 1.0, "food": 0.5, "day_phase": 0.25,
            "position": {"x": 1, "y": 2, "z": 3}, "sheltered": false,
            "chosen": cand(), "reflex": true, "outcome": outcome,
            "llm": {"calls": 2, "cached": 1, "fallbacks": 3, "unparsed": 4,
                    "input_tokens": 50, "output_tokens": 6, "cost_usd": 0.25},
        })
    );
    let bare = McStep {
        llm: None,
        chosen: None,
        outcome: None,
        reflex: false,
        obs: obs(),
    };
    assert_eq!(row(&bare)["llm"], serde_json::Value::Null);
}

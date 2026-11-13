// The shared status text and prompt fitting (spec §23), and call stats.

use host_api::{AgentId, ChatMsg, Inventory, Observation, Pos, Seen, SeenKind, ThingId};
use llm::describe::{at, fit, status};
use llm::{LlmStats, Reply};

fn obs() -> Observation {
    let seen = |t: &str, x, d| Seen {
        thing: ThingId(t.into()),
        kind: SeenKind::Block,
        pos: Pos { x, y: 64, z: 0 },
        distance: d,
    };
    let say = |t: &str| ChatMsg {
        from: "alex".into(),
        text: t.into(),
    };
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.25,
        health: 0.904,
        food: 0.555,
        position: Pos { x: 1, y: -2, z: 3 },
        inventory: Inventory {
            items: vec![
                ("stick".into(), 2),
                ("dirt".into(), 2),
                ("oak_log".into(), 5),
            ],
        },
        visible: (0..10)
            .map(|i| seen(&format!("block:b{i}"), i, 10.0 - i as f32))
            .collect(),
        chat: vec![say("one"), say("two"), say("three")],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

#[test]
fn status_by_hand() {
    let o = obs();
    assert_eq!(at(o.position), "(1, -2, 3)");
    let s = status(&o, 3, 2);
    assert_eq!(
        s,
        [
            "health 90%, food 56%, day, at (1, -2, 3)",
            "inventory: oak_log x5, dirt x2, stick x2",
            "see block:b9 1 away",
            "see block:b8 2 away",
            "alex says: three",
            "alex says: two",
        ]
    );
    let mut n = o.clone();
    n.day_phase = 0.75;
    n.sheltered = true;
    let s = status(&n, 0, 0);
    assert_eq!(
        s[0],
        "health 90%, food 56%, night, sheltered, at (1, -2, 3)"
    );
    assert_eq!(s[1], "inventory: empty");
}

#[test]
fn fit_drops_far_things_then_small_stacks_never_the_tail() {
    let o = obs();
    let tail = vec!["Options:".to_string(), "0: rest".to_string()];
    let with = |items, seen| {
        let mut l = status(&o, items, seen);
        l.extend(tail.iter().cloned());
        l.join("\n")
    };
    // Room for everything: eight things listed, all three stacks.
    assert_eq!(fit(&o, &tail, 0, 1000), with(3, 8));
    // One character short of that: the farthest of the eight goes.
    let tokens = |text: String, sys: usize| ((sys + text.chars().count() - 1) / 4) as u32;
    assert_eq!(fit(&o, &tail, 7, tokens(with(3, 8), 7)), with(3, 7));
    // Short of the no-things form: stacks go, smallest first.
    assert_eq!(fit(&o, &tail, 5, tokens(with(3, 0), 5)), with(2, 0));
    // No room at all: the bare form still goes out.
    assert_eq!(fit(&o, &tail, 50, 0), with(0, 0));
    assert!(with(0, 0).ends_with("Options:\n0: rest"));
}

#[test]
fn stats_count_spend_only_for_calls_that_went_out() {
    let r = |cached| Reply {
        text: String::new(),
        input_tokens: 100,
        output_tokens: 10,
        cost_usd: 0.25,
        cached,
    };
    let mut s = LlmStats::default();
    s.record(&r(false));
    s.record(&r(true));
    s.record(&r(false));
    assert_eq!(
        s,
        LlmStats {
            calls: 3,
            cached: 1,
            fallbacks: 0,
            unparsed: 0,
            input_tokens: 200,
            output_tokens: 20,
            cost_usd: 0.5,
        }
    );
}

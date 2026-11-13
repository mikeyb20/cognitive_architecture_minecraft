// The runner's Minecraft host and loop against a fake bot on a local
// socket (spec §22). The fake answers each request with a scripted reply
// and hands back every request it got.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use baselines::ArmA;
use host_api::link::BotConfig;
use host_api::reflex::ReflexThresholds;
use host_api::{
    AgentId, Candidate, ChatMsg, Host, Intensity, Inventory, Observation, Outcome, Pos, Seen,
    SeenKind, Skill, Target, TaskType, ThingId,
};
use minecraft::McParams;
use runner::FULL;
use runner::mc::{Link, LinkError, McHost, run};
use serde_json::{Value, json};

/// A fake bot: answers with `reply(request)`; `None` hangs up.
fn bot(
    mut reply: impl FnMut(&Value) -> Option<String> + Send + 'static,
) -> (String, Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut w = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines() {
            let req: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let back = reply(&req);
            let _ = tx.send(req);
            match back {
                Some(b) => writeln!(w, "{b}").unwrap(),
                None => return,
            }
        }
    });
    (addr, rx)
}

fn obs(chat: &[&str]) -> Observation {
    let block = |t: &str, x| Seen {
        thing: ThingId(t.into()),
        kind: SeenKind::Block,
        pos: Pos { x, y: 64, z: 0 },
        distance: x as f32,
    };
    Observation {
        agent: AgentId(0),
        tick: 10,
        day_phase: 0.25,
        health: 1.0,
        food: 1.0,
        position: Pos { x: 0, y: 64, z: 0 },
        inventory: Inventory {
            items: vec![("oak_log".into(), 1)],
        },
        visible: vec![block("block:oak_log", 3)],
        chat: chat
            .iter()
            .map(|t| ChatMsg {
                from: "alex".into(),
                text: (*t).into(),
            })
            .collect(),
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn observed(o: &Observation) -> String {
    json!({"reply": "observed", "obs": o}).to_string()
}

fn done(success: bool) -> String {
    let o = Outcome {
        success,
        progress: 1.0,
        items_gained: Inventory::default(),
        damage: 0.0,
        new_things: vec![],
        blocked: false,
        duration_ticks: 20,
    };
    json!({"reply": "done", "outcome": o}).to_string()
}

/// A bot that works: ready, then `seen` for every perceive.
fn good_bot(seen: Observation) -> (String, Receiver<Value>) {
    bot(move |req| {
        Some(match req["op"].as_str().unwrap() {
            "hello" => json!({"reply": "ready", "names": ["oak_log", "stone"]}).to_string(),
            "perceive" => observed(&seen),
            "execute" => done(true),
            _ => json!({"reply": "said"}).to_string(),
        })
    })
}

fn connect(addr: &str) -> Result<McHost, LinkError> {
    McHost::connect(
        Link::tcp(addr).unwrap(),
        BotConfig::default(),
        McParams::default(),
    )
}

#[test]
fn connect_says_hello_then_looks() {
    let (addr, got) = good_bot(obs(&[]));
    let mut h = connect(&addr).unwrap();
    let hello = got.recv().unwrap();
    assert_eq!(hello["op"], "hello");
    let config: BotConfig = serde_json::from_value(hello["config"].clone()).unwrap();
    assert_eq!(config, BotConfig::default());
    assert_eq!(got.recv().unwrap(), json!({"op": "perceive"}));
    assert_eq!(h.perceive(), obs(&[]));
    assert!(h.error.is_none());
}

#[test]
fn chat_commands_are_answered_and_mark_candidates() {
    let (addr, got) = good_bot(obs(&["bot, get 2 oak_log", "bot, get 1 unobtainium"]));
    let mut h = connect(&addr).unwrap();
    let o = h.perceive();
    let c = h.candidate_actions(&o);
    let said: Vec<Value> = got.try_iter().filter(|r| r["op"] == "say").collect();
    assert_eq!(said.len(), 2);
    assert!(
        said[0]["text"].as_str().unwrap().starts_with("On it"),
        "{said:?}"
    );
    assert_eq!(said[1]["text"], "I don't know what \"unobtainium\" is.");
    let chop = c.iter().find(|x| x.skill == Skill::Chop).unwrap();
    assert!(chop.commanded, "the log in view serves the command");
    assert_eq!(h.queue().waiting().count(), 0);
    assert!(h.queue().current().is_some());
    assert_eq!(h.places().count("block:oak_log"), 1);
}

fn cand(skill: Skill, target: Target) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target: Some(target),
        task_type: TaskType::Crafting,
        est_cost: 0.1,
        est_risk: 0.0,
        commanded: false,
    }
}

#[test]
fn crafts_carry_their_recipe() {
    let (addr, got) = good_bot(obs(&[]));
    let mut h = connect(&addr).unwrap();
    let craft = cand(Skill::Craft, Target::Item("iron_ingot".into()));
    assert!(h.execute(&craft, FULL).success);
    let chop = cand(Skill::Chop, Target::Pos(Pos { x: 3, y: 64, z: 0 }));
    h.execute(&chop, FULL);
    let sent: Vec<Value> = got.try_iter().filter(|r| r["op"] == "execute").collect();
    assert_eq!(
        sent[0]["recipe"],
        json!({"station": "furnace", "inputs": [["raw_iron", 1]]})
    );
    let action: Candidate = serde_json::from_value(sent[0]["action"].clone()).unwrap();
    assert_eq!(action, craft);
    let intensity: Intensity = serde_json::from_value(sent[0]["intensity"].clone()).unwrap();
    assert_eq!(intensity, FULL);
    assert_eq!(sent[1]["recipe"], Value::Null);
    let other = cand(Skill::Craft, Target::Pos(Pos { x: 0, y: 0, z: 0 }));
    h.execute(&other, FULL);
    assert_eq!(got.recv().unwrap()["recipe"], Value::Null);
}

#[test]
fn the_loop_uses_reflexes_and_the_arm() {
    let mut weak = obs(&[]);
    weak.health = 0.1;
    weak.visible.push(Seen {
        thing: ThingId("entity:zombie".into()),
        kind: SeenKind::Entity { hostile: true },
        pos: Pos { x: 2, y: 64, z: 0 },
        distance: 2.0,
    });
    let (addr, _got) = good_bot(weak);
    let mut h = connect(&addr).unwrap();
    let mut steps = Vec::new();
    let n = run(
        &mut h,
        &mut ArmA::default(),
        3,
        ReflexThresholds::default(),
        |s| steps.push(s.clone()),
    );
    assert_eq!(n, 3);
    assert!(steps.iter().all(|s| s.reflex));
    assert_eq!(steps[0].chosen.as_ref().unwrap().skill, Skill::Flee);
    assert!(steps[0].outcome.as_ref().unwrap().success);

    let (addr, _got) = good_bot(obs(&[]));
    let mut h = connect(&addr).unwrap();
    let mut chose = None;
    run(
        &mut h,
        &mut ArmA::default(),
        1,
        ReflexThresholds::default(),
        |s| chose = Some((s.reflex, s.chosen.clone())),
    );
    let (reflex, chosen) = chose.unwrap();
    assert!(!reflex);
    assert!(chosen.is_some());
}

#[test]
fn a_broken_link_stops_the_run_and_keeps_the_last_view() {
    let mut n = 0;
    let (addr, _got) = bot(move |req| {
        n += 1;
        match req["op"].as_str().unwrap() {
            "hello" => Some(json!({"reply": "ready", "names": []}).to_string()),
            "perceive" if n == 2 => Some(observed(&obs(&[]))),
            "perceive" => Some(json!({"reply": "failed", "message": "not spawned"}).to_string()),
            _ => None,
        }
    });
    let mut h = connect(&addr).unwrap();
    let made = run(
        &mut h,
        &mut ArmA::default(),
        5,
        ReflexThresholds::default(),
        |_| {},
    );
    assert_eq!(made, 1, "the failed look still counts as one pass");
    assert_eq!(h.error, Some(LinkError::Failed("not spawned".into())));
    assert_eq!(h.perceive(), obs(&[]), "last view kept");
}

#[test]
fn hang_ups_bad_lines_and_wrong_replies_are_errors() {
    let (addr, _) = bot(|_| None);
    assert_eq!(connect(&addr).err(), Some(LinkError::Closed));
    let (addr, _) = bot(|_| Some("{oops".into()));
    assert!(matches!(connect(&addr).err(), Some(LinkError::Bad(_))));
    let (addr, _) = bot(|_| Some(json!({"reply": "said"}).to_string()));
    assert_eq!(
        connect(&addr).err(),
        Some(LinkError::Unexpected("Said".into()))
    );
    // Ready again where an observation belongs.
    let (addr, _) = bot(|_| Some(json!({"reply": "ready", "names": []}).to_string()));
    assert!(matches!(
        connect(&addr).err(),
        Some(LinkError::Unexpected(_))
    ));

    let mut n = 0;
    let (addr, _) = bot(move |req| {
        n += 1;
        Some(match (n, req["op"].as_str().unwrap()) {
            (1, _) => json!({"reply": "ready", "names": []}).to_string(),
            (2, _) => observed(&obs(&["!status"])),
            (_, "execute") => json!({"reply": "said"}).to_string(),
            _ => json!({"reply": "failed", "message": "chat off"}).to_string(),
        })
    });
    let mut h = connect(&addr).unwrap();
    let o = obs(&["!status"]);
    h.candidate_actions(&o);
    assert_eq!(h.error, Some(LinkError::Failed("chat off".into())));
    let mut h2 = h;
    h2.error = None;
    let out = h2.execute(&cand(Skill::Rest, Target::Item(String::new())), FULL);
    assert!(out.blocked && !out.success && out.duration_ticks == 0);
    assert_eq!(h2.error, Some(LinkError::Unexpected("Said".into())));
}

#[test]
fn no_bot_no_link() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    assert!(Link::tcp(&addr).is_err());
}

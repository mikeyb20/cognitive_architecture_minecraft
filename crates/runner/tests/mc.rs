// The runner's Minecraft host and loop against a fake bot on a local
// socket (spec §22). The fake answers each request with a scripted reply
// and hands back every request it got.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use baselines::ArmA;
use cognitive_core::agent::{Brain, BrainConfig};
use host_api::link::BotConfig;
use host_api::reflex::ReflexThresholds;
use host_api::{
    AgentId, Candidate, ChatMsg, Host, Intensity, Inventory, Observation, Outcome, Pos, Seen,
    SeenKind, Skill, Target, TaskType, ThingId,
};
use minecraft::McParams;
use runner::FULL;
use runner::mc::{BrainCounts, BrainRun, Link, LinkError, McHost, McStep, run, run_brain};
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
    let want = BotConfig {
        danger_range: McParams::default().threat_range,
        ..BotConfig::default()
    };
    assert_eq!(config, want, "the bot gets the candidates' threat range");
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

fn plan(decisions: u32, teacher: bool) -> BrainRun {
    BrainRun {
        decisions,
        thresholds: ReflexThresholds::default(),
        teacher,
        teach_days: None,
    }
}

/// The brain on a fake bot for `n` decisions: counts, steps, waits.
fn brain_run(seen: Observation, n: u32) -> (BrainCounts, Vec<McStep>, u32, Brain) {
    let (addr, _got) = good_bot(seen);
    let mut h = connect(&addr).unwrap();
    let mut brain = Brain::new(BrainConfig::default(), 7);
    let (mut steps, mut waits) = (Vec::new(), 0);
    let counts = run_brain(
        &mut h,
        &mut brain,
        plan(n, true),
        || waits += 1,
        |s| steps.push(s.clone()),
    );
    (counts, steps, waits, brain)
}

#[test]
fn the_brain_drives_the_bot() {
    let (counts, steps, waits, brain) = brain_run(obs(&[]), 40);
    assert_eq!(counts.decisions(), 40);
    assert_eq!(steps.len(), 40);
    assert_eq!(counts.reflexes, 0);
    assert!(counts.acts > 0, "{counts:?}");
    assert_eq!(waits, counts.thoughts + counts.idle, "the game moves on");
    let acted = steps.iter().filter(|s| s.chosen.is_some()).count();
    assert_eq!(acted as u32, counts.acts);
    for s in &steps {
        assert!(!s.reflex && s.llm.is_none());
        assert_eq!(s.chosen.is_some(), s.outcome.is_some());
    }
    assert!(!brain.memory().is_empty(), "outcomes reach the brain");
}

#[test]
fn outcomes_reach_the_brain() {
    let run_with = |success: bool| {
        let (addr, _got) = bot(move |req| {
            Some(match req["op"].as_str().unwrap() {
                "hello" => json!({"reply": "ready", "names": []}).to_string(),
                "perceive" => observed(&obs(&[])),
                _ => done(success),
            })
        });
        let mut h = connect(&addr).unwrap();
        let mut brain = Brain::new(BrainConfig::default(), 7);
        run_brain(&mut h, &mut brain, plan(40, true), || {}, |_| {});
        (brain.c(), brain.v(), brain.memory().len())
    };
    assert_ne!(
        run_with(true),
        run_with(false),
        "success and failure differ"
    );
}

#[test]
fn the_cli_runs_the_brain_on_a_bot() {
    let (addr, got) = good_bot(obs(&[]));
    let log = std::env::temp_dir().join(format!("mc-c-{}.jsonl", std::process::id()));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_runner"))
        .args([
            "mc",
            "c",
            "--seed",
            "2",
            "--decisions",
            "4",
            "--addr",
            &addr,
        ])
        .arg("--log")
        .arg(&log)
        .output()
        .expect("runner should start");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{text}");
    assert!(text.contains("link error: None"), "{text}");
    let rows = std::fs::read_to_string(&log).unwrap();
    assert_eq!(rows.lines().count(), 4);
    assert_eq!(got.recv().unwrap()["op"], "hello");
    std::fs::remove_file(log).unwrap();
}

#[test]
fn reflexes_come_before_the_brain() {
    let mut weak = obs(&[]);
    weak.health = 0.1;
    weak.visible.push(Seen {
        thing: ThingId("entity:zombie".into()),
        kind: SeenKind::Entity { hostile: true },
        pos: Pos { x: 2, y: 64, z: 0 },
        distance: 2.0,
    });
    let (counts, steps, waits, _) = brain_run(weak, 3);
    let want = BrainCounts {
        reflexes: 3,
        ..BrainCounts::default()
    };
    assert_eq!(counts, want);
    assert_eq!(counts.decisions(), 3);
    assert_eq!(waits, 0);
    assert!(steps.iter().all(|s| s.reflex && s.outcome.is_some()));
    assert_eq!(steps[0].chosen.as_ref().unwrap().skill, Skill::Flee);
}

/// Run the brain where each perceive returns `views[i]` (the last
/// repeats; `connect` reads the first).
fn teach_run(views: Vec<Observation>, n: u32, teacher: bool) -> (BrainCounts, Vec<McStep>, Brain) {
    teach_with(views, plan(n, teacher))
}

fn teach_with(views: Vec<Observation>, run: BrainRun) -> (BrainCounts, Vec<McStep>, Brain) {
    let mut i = 0;
    let (addr, _got) = bot(move |req| {
        Some(match req["op"].as_str().unwrap() {
            "hello" => json!({"reply": "ready", "names": []}).to_string(),
            "perceive" => {
                i += 1;
                observed(&views[(i - 1).min(views.len() - 1)])
            }
            _ => done(false),
        })
    });
    let mut h = connect(&addr).unwrap();
    let mut brain = Brain::new(BrainConfig::default(), 3);
    let mut steps = Vec::new();
    let counts = run_brain(
        &mut h,
        &mut brain,
        run,
        || {},
        |s| {
            steps.push(s.clone());
        },
    );
    (counts, steps, brain)
}

fn at(day_phase: f32, sheltered: bool) -> Observation {
    Observation {
        day_phase,
        sheltered,
        ..obs(&[])
    }
}

#[test]
fn the_teacher_calls_once_at_dusk_and_the_brain_takes_the_goal() {
    let dusk = at(0.5, false);
    let (counts, steps, brain) = teach_run(vec![dusk.clone()], 6, true);
    assert_eq!(counts.dusk_calls, 1, "once a day");
    let said: Vec<&str> = steps
        .iter()
        .flat_map(|s| &s.obs.chat)
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(said, ["get shelter"]);
    assert_eq!(steps[0].obs.chat[0].from, "steve");
    assert_eq!(
        brain.goal().map(|g| (g.item.as_str(), g.commanded)),
        Some(("shelter", true))
    );

    let (counts, steps, brain) = teach_run(vec![dusk], 6, false);
    assert_eq!(counts.dusk_calls, 0, "teacher off");
    assert!(steps.iter().all(|s| s.obs.chat.is_empty()));
    assert_ne!(brain.goal().map(|g| g.item.as_str()), Some("shelter"));

    let (counts, _, _) = teach_run(vec![at(0.3, false)], 4, true);
    assert_eq!(counts.dusk_calls, 0, "not before dusk");
    let (counts, _, _) = teach_run(vec![at(0.5, true)], 4, true);
    assert_eq!(counts.dusk_calls, 0, "not when sheltered");
}

#[test]
fn the_teacher_shows_the_bed_lesson_until_its_days_are_up() {
    // At dusk in the open with planks and a sheep in view: the shown
    // recipe's first step is to hunt the sheep for wool (`bed_lesson`).
    let mut dusk = at(0.5, false);
    dusk.inventory.items = vec![("oak_planks".into(), 4)];
    dusk.visible.push(Seen {
        thing: ThingId("entity:sheep".into()),
        kind: SeenKind::Entity { hostile: false },
        pos: Pos { x: 4, y: 64, z: 0 },
        distance: 4.0,
    });
    let step = |teacher, teach_days| {
        let run = BrainRun {
            teach_days,
            ..plan(2, teacher)
        };
        let (_, _, brain) = teach_with(vec![dusk.clone()], run);
        brain.last_log().recipe_step.clone()
    };
    assert_eq!(step(true, None).as_deref(), Some("Attack:entity:sheep"));
    assert_eq!(step(true, Some(1)).as_deref(), Some("Attack:entity:sheep"));
    assert_eq!(step(true, Some(0)), None, "the teacher's days are up");
}

#[test]
fn the_bots_sheltered_flag_is_the_shelter_test() {
    // Called at dusk in the open, then the bot reports a shelter: the
    // test passes and the goal is done (01 §4.11, #58), as in the sim.
    // The first view is the one `connect` reads.
    let views = vec![at(0.5, false), at(0.5, false), at(0.5, true)];
    let (counts, steps, brain) = teach_run(views, 12, true);
    assert_eq!(counts.dusk_calls, 1);
    assert!(steps[1].obs.sheltered);
    assert_ne!(
        brain.goal().map(|g| g.item.as_str()),
        Some("shelter"),
        "goal done"
    );
}

#[test]
fn the_host_runs_with_the_params_it_was_given() {
    let (addr, _got) = good_bot(obs(&[]));
    let params = McParams {
        day_ticks: 7,
        ..McParams::default()
    };
    let h = McHost::connect(Link::tcp(&addr).unwrap(), BotConfig::default(), params).unwrap();
    assert_eq!(h.params(), &params);
}

#[test]
fn the_cli_teacher_can_be_turned_off() {
    let calls = |extra: &[&str]| {
        let (addr, _got) = good_bot(at(0.5, false));
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_runner"))
            .args(["mc", "c", "--decisions", "3", "--addr", &addr])
            .args(extra)
            .output()
            .expect("runner should start");
        String::from_utf8(out.stdout).unwrap()
    };
    let on = calls(&[]);
    assert!(on.contains("dusk_calls: 1"), "{on}");
    let off = calls(&["--no-teacher"]);
    assert!(off.contains("dusk_calls: 0"), "{off}");
}

#[test]
fn the_brain_stops_on_a_broken_link() {
    let mut n = 0;
    let (addr, _got) = bot(move |req| {
        n += 1;
        match (n, req["op"].as_str().unwrap()) {
            (1, _) => Some(json!({"reply": "ready", "names": []}).to_string()),
            (2, _) => Some(observed(&obs(&[]))),
            _ => None,
        }
    });
    let mut h = connect(&addr).unwrap();
    let mut brain = Brain::new(BrainConfig::default(), 0);
    let counts = run_brain(&mut h, &mut brain, plan(5, true), || {}, |_| {});
    assert_eq!(
        counts.decisions(),
        1,
        "the failed look still counts as one pass"
    );
    assert_eq!(h.error, Some(LinkError::Closed));
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

//! The sim's code tools (spec §21), the run header (spec §25) and the brain
//! CLI commands.

use cognitive_core::agent::{Brain, BrainConfig, Horizon, ThinkTools};
use cognitive_core::direction::PlanStep;
use host_api::{AgentId, Inventory, Observation, Pos, Seen, SeenKind, Skill, TaskType, ThingId};
use runner::brain::{DUSK, HUNGRY, SimTools, header, run_brain, setup};
use std::process::Command;

fn obs() -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: Pos::default(),
        inventory: Inventory::default(),
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn step(skill: Skill, thing: &str, task: TaskType) -> Option<PlanStep> {
    Some(PlanStep {
        skill,
        thing: thing.into(),
        task,
    })
}

#[test]
fn plan_puts_shelter_then_food_then_the_goal() {
    let mut t = SimTools::default();
    let goal = "oak_planks";
    let mut o = obs();
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Chop, "block:oak_log", TaskType::Mining)
    );
    o.day_phase = DUSK;
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Shelter, "", TaskType::Building)
    );
    o.sheltered = true;
    assert_eq!(t.plan(goal, &o), step(Skill::Rest, "", TaskType::Building));
    let mut o = obs();
    o.food = HUNGRY - 0.01;
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Farm, "block:wheat", TaskType::Farming),
        "no food anywhere: grow bread"
    );
    o.visible.push(Seen {
        thing: ThingId("entity:pig".into()),
        kind: SeenKind::Entity { hostile: false },
        pos: Pos::default(),
        distance: 2.0,
    });
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Attack, "entity:pig", TaskType::Farming)
    );
    o.inventory.items.push(("bread".into(), 1));
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Eat, "item:bread", TaskType::Farming)
    );
    o.food = HUNGRY;
    assert_eq!(
        t.plan(goal, &o),
        step(Skill::Chop, "block:oak_log", TaskType::Mining),
        "not hungry at the threshold"
    );
    assert_eq!(t.plan("diamond", &obs()), None);
}

#[test]
fn progress_and_own_goals_come_from_the_planner() {
    let mut t = SimTools::default();
    let mut o = obs();
    assert_eq!(t.progress("oak_planks", &o), 0.0);
    assert_eq!(t.propose_goal(&o), Some("wooden_pickaxe".into()));
    o.inventory.items.push(("oak_planks".into(), 4));
    assert_eq!(t.progress("oak_planks", &o), 1.0);
}

#[test]
fn header_names_the_run() {
    let h: serde_json::Value = serde_json::from_str(&header("harsh", 7)).unwrap();
    assert_eq!(h["header"], true);
    assert_eq!(h["scenario"], "harsh");
    assert_eq!(h["seed"], 7);
    assert_eq!(h["arm"], "C");
    assert!(h["llm_tier"].is_null());
    assert_eq!(h["defs_version"], cognitive_core::DEFS_VERSION);
    assert_eq!(h["spec_version"], cognitive_core::SPEC_VERSION);
    assert_eq!(h["paper_version"], cognitive_core::PAPER_VERSION);
}

#[test]
fn a_horizon_sets_the_lifespan() {
    let s = setup("long_horizon", 1);
    let h = s.horizon.expect("long_horizon has a horizon");
    let cfg = BrainConfig {
        horizon: Some(Horizon { ticks: h }),
        ..BrainConfig::default()
    };
    let life = Brain::new(cfg, 1).lifespan().expect("a lifespan");
    let mut rows = 0;
    let r = run_brain(s, BrainConfig::default(), 1, |_| rows += 1);
    // The run ends at the hidden lifespan at the latest (spec §15); the
    // last action may run a little past it.
    assert!(r.ticks < life + 200, "{r:?} vs {life}");
    assert!(!r.survived || r.ticks >= life, "{r:?} vs {life}");
    assert_eq!(rows, r.decisions);
}

#[test]
#[should_panic(expected = "unknown scenario")]
fn unknown_scenarios_are_refused() {
    let _ = setup("nope", 0);
}

fn runner(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(args)
        .output()
        .expect("runner should start");
    (out.status.success(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn brain_command_reports_and_logs() {
    let dir = std::env::temp_dir().join(format!("brain-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("run.jsonl");
    let (ok, text) = runner(&["brain", "first_night", "2", "--log", log.to_str().unwrap()]);
    assert!(ok, "{text}");
    assert!(
        text.contains("seed 0:") && text.contains("seed 1:"),
        "{text}"
    );
    assert!(text.contains("brain first_night: 2/2 survived"), "{text}");
    let body = std::fs::read_to_string(&log).unwrap();
    let headers = body
        .lines()
        .filter(|l| l.contains("\"header\":true"))
        .count();
    assert_eq!(headers, 2);
    let row: serde_json::Value = serde_json::from_str(body.lines().nth(1).unwrap()).unwrap();
    assert!(row["c"].is_number() && row["chosen"].is_string());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn speed_command_reports_the_rate_and_applies_the_gate() {
    let (ok, text) = runner(&["speed", "1", "--gate", "0"]);
    assert!(ok, "{text}");
    let ticks = runner::brain::speed_ticks(1);
    assert!(
        text.contains(&format!("brain speed: {ticks} world ticks")) && text.contains("ticks/s"),
        "{text}"
    );
    let (ok, _) = runner(&["speed", "1", "--gate", "1e15"]);
    assert!(!ok, "far below the gate");
}

#[test]
fn speed_sums_ticks_over_seeds() {
    use runner::brain::{rate, run_brain, setup, speed_ticks};
    let one = |s| run_brain(setup("open_day", s), Default::default(), s, |_| {}).ticks;
    assert_eq!(speed_ticks(2), one(0) + one(1));
    assert_eq!(speed_ticks(0), 0);
    assert_eq!(rate(5000, 2.0), 2500.0);
}

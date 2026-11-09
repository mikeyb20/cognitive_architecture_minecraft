//! Open-question sweeps (spec §27 step 5): the registry, the metrics, the
//! tables and the CLI.

use cognitive_core::agent::{BrainConfig, TickLog};
use runner::brain::BrainResult;
use runner::sweep::{
    FREEZE_CONFLICT, Meter, Row, RunMetrics, Summary, all_scenarios, csv, ids, markdown, run_one,
    run_sweep, summarise, sweep,
};
use std::process::Command;

#[test]
fn every_sweep_has_its_default_once() {
    assert_eq!(ids(), ["OQ8", "OQ12", "OQ16", "OQ17", "OQ18", "OQ19"]);
    for id in ids() {
        let s = sweep(id).unwrap();
        assert_eq!(s.id, id);
        assert!(s.question.contains("spec §"), "{}", s.question);
        let defaults: Vec<_> = s
            .variants
            .iter()
            .filter(|v| v.config == BrainConfig::default())
            .collect();
        assert_eq!(defaults.len(), 1, "{id}");
        assert!(defaults[0].label.ends_with("(default)"), "{id}");
        let marked = s.variants.iter().filter(|v| v.label.contains("default"));
        assert_eq!(marked.count(), 1, "{id}");
        assert!(s.variants.len() >= 2);
    }
    assert_eq!(sweep("oq17").unwrap().variants.len(), 9, "3 × 3 grid");
    assert!(sweep("OQ99").is_none());
}

fn row(chosen: &str, c: f32, d: f32, v: f32, conflict: f32) -> TickLog {
    TickLog {
        chosen: chosen.into(),
        c,
        d,
        v,
        conflict,
        ..TickLog::default()
    }
}

fn result(survived: bool, inventory: Vec<(String, u32)>) -> BrainResult {
    BrainResult {
        scenario: "first_night",
        seed: 0,
        survived,
        ticks: 2400,
        death: None,
        decisions: 4,
        idle: 1,
        thoughts: 0,
        reflexes: 2,
        new_things: 7,
        inventory,
    }
}

#[test]
fn meter_counts_by_hand() {
    let mut m = Meter::default();
    m.push(&row("idle", 0.1, 0.0, 0.0, 0.0));
    m.push(&row("Chop:1,2", 0.3, 0.2, -0.4, 0.95));
    m.push(&row("Mine:3,4", 0.2, 0.0, 0.6, 0.99));
    let mut pushed = row("Flee", 0.2, 0.2, 0.0, FREEZE_CONFLICT);
    pushed.external_drive = true;
    m.push(&pushed);
    let mut kicked = row("Plan", 0.4, 0.0, 0.2, 0.5);
    kicked.stuck_kick = true;
    kicked.retrieval_hit = true;
    kicked.mem_items = 12;
    m.push(&kicked);
    let inv = vec![
        ("wooden_pickaxe".to_string(), 1),
        ("furnace".to_string(), 0),
        ("stone_pickaxe".to_string(), 1),
        ("oak_log".to_string(), 3),
    ];
    let r = m.finish(&result(true, inv));
    assert_eq!(r.decisions, 5);
    assert_eq!((r.active, r.self_started), (0.8, 0.6));
    assert!((r.mean_c - 0.24).abs() < 1e-6 && (r.mean_d - 0.08).abs() < 1e-6);
    assert!((r.mean_v - 0.08).abs() < 1e-6, "{}", r.mean_v);
    assert_eq!(
        r.freeze, 0.4,
        "only conflict above the threshold, on actions"
    );
    assert_eq!((r.stuck_kicks, r.reflexes, r.new_things), (1, 2, 7));
    assert_eq!(
        (r.memory, r.recall),
        (12.0, 0.2),
        "memory at the last row; one recall in five"
    );
    assert!(
        (r.thinking - 1000.0 / 2400.0).abs() < 1e-6,
        "one Plan in 2,400 ticks"
    );
    assert!(r.plan_lag.is_nan(), "no goal, no lag");
    assert_eq!(
        r.tech, 2,
        "two held pickaxes; a zero-count furnace is not held"
    );
    assert!(r.survived && r.ticks == 2400);
    let empty = Meter::default().finish(&result(false, vec![]));
    assert_eq!(
        (empty.decisions, empty.active, empty.mean_c, empty.recall),
        (0, 0.0, 0.0, 0.0)
    );
}

#[test]
fn meter_times_plans_and_counts_thoughts() {
    let at = |tick: u64, chosen: &str, goal: Option<&str>, planned: bool| TickLog {
        tick,
        chosen: chosen.into(),
        goal: goal.map(Into::into),
        plan_step: planned.then(|| "Chop:block:oak_log".into()),
        ..TickLog::default()
    };
    let mut m = Meter::default();
    m.push(&at(0, "idle", None, false));
    m.push(&at(10, "idle", Some("stick"), false));
    m.push(&at(15, "Plan", Some("stick"), true)); // lag 5
    m.push(&at(16, "Chop", Some("stick"), true)); // already planned
    let mut fired = at(20, "Rehearse", Some("stick"), true);
    fired.triggered = 2;
    m.push(&fired);
    m.push(&at(30, "idle", Some("iron"), false));
    m.push(&at(31, "Imagine", None, false)); // goal dropped unplanned
    m.push(&at(40, "Reflect", Some("iron"), true)); // lag 0
    let r = m.finish(&result(true, vec![]));
    assert_eq!(r.plan_lag, 2.5, "mean of 5 and 0");
    // Plan, Imagine, Reflect and two triggered: 5 in 2,400 ticks.
    assert!(
        (r.thinking - 5000.0 / 2400.0).abs() < 1e-5,
        "{}",
        r.thinking
    );
}

#[test]
fn summary_averages_over_seeds() {
    let a = RunMetrics {
        survived: true,
        active: 0.5,
        mean_c: 0.2,
        stuck_kicks: 2,
        tech: 1,
        new_things: 10,
        memory: 30.0,
        recall: 0.25,
        thinking: 2.0,
        plan_lag: f32::NAN,
        ..RunMetrics::default()
    };
    let b = RunMetrics {
        survived: false,
        active: 0.1,
        mean_c: 0.4,
        stuck_kicks: 1,
        tech: 2,
        new_things: 20,
        memory: 50.0,
        recall: 0.75,
        thinking: 4.0,
        plan_lag: 12.0,
        ..RunMetrics::default()
    };
    let s = summarise(&[a, b]);
    assert_eq!(s.runs, 2);
    assert_eq!(
        (
            s.survival,
            s.stuck_kicks,
            s.tech,
            s.new_things,
            s.memory,
            s.recall
        ),
        (0.5, 1.5, 1.5, 15.0, 40.0, 0.5)
    );
    assert_eq!(
        (s.thinking, s.plan_lag),
        (3.0, 12.0),
        "the lag averages only runs that planned"
    );
    assert!((s.active - 0.3).abs() < 1e-6 && (s.mean_c - 0.3).abs() < 1e-6);
    let lag = |plan_lag| RunMetrics {
        plan_lag,
        ..RunMetrics::default()
    };
    let three = summarise(&[lag(8.0), lag(f32::NAN), lag(12.0)]);
    assert_eq!(
        three.plan_lag, 10.0,
        "mean of 8 and 12; the NaN run is left out"
    );
    let none = summarise(&[]);
    assert!(none.plan_lag.is_nan());
    assert_eq!(
        Summary {
            plan_lag: 0.0,
            ..none
        },
        Summary::default()
    );
}

#[test]
fn sweeps_replay_whatever_the_thread_count() {
    let s = sweep("OQ16").unwrap();
    let scen = ["first_night"];
    let one = run_sweep(&s, &scen, 2, 1);
    let three = run_sweep(&s, &scen, 2, 3);
    // Debug text, because a NaN plan lag never equals itself.
    assert_eq!(format!("{one:?}"), format!("{three:?}"));
    assert_eq!(one.len(), s.variants.len() * scen.len());
    assert_eq!(one[0].variant, s.variants[0].label);
    assert!(
        one.iter()
            .all(|r| r.summary.runs == 2 && r.scenario == "first_night")
    );
}

#[test]
fn tables_have_one_line_per_row() {
    let s = sweep("OQ18").unwrap();
    let rows = vec![
        Row {
            variant: "EXP3 γ=0.1".into(),
            scenario: "harsh".into(),
            summary: Summary {
                survival: 0.5,
                stuck_kicks: 1.25,
                memory: 41.6,
                recall: 0.125,
                thinking: 3.5,
                plan_lag: f32::NAN,
                ..Summary::default()
            },
        },
        Row {
            variant: "a, b".into(),
            scenario: "open_day".into(),
            summary: Summary {
                plan_lag: 7.4,
                ..Summary::default()
            },
        },
    ];
    let md = markdown(&s, &rows, 4);
    assert!(md.starts_with("## OQ18: Oracle learner"));
    assert!(md.contains("4 seeds per cell"));
    assert!(md.contains("| EXP3 γ=0.1 | harsh | 0.50 |"), "{md}");
    assert_eq!(md.lines().filter(|l| l.starts_with('|')).count(), 4);
    let c = csv(&s, &rows);
    let lines: Vec<&str> = c.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("oq,variant,scenario,survival"));
    assert!(lines[1].starts_with("OQ18,EXP3 γ=0.1,harsh,0.50,"));
    assert!(
        lines[1].ends_with(",42,0.12,3.5,–"),
        "memory whole, recall to two places, think to one, no lag: {}",
        lines[1]
    );
    assert!(
        lines[1].contains(",1.2,"),
        "stuck kicks to one decimal: {}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("OQ18,\"a, b\",open_day,") && lines[2].ends_with(",0.0,7"),
        "{}",
        lines[2]
    );
}

#[test]
fn sweep_command_writes_tables() {
    let dir = std::env::temp_dir().join(format!("sweep-cli-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args([
            "sweep",
            "oq16",
            "--seeds",
            "1",
            "--scenarios",
            "first_night",
        ])
        .args(["--out", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("## OQ16"), "{text}");
    let md = std::fs::read_to_string(dir.join("OQ16.md")).unwrap();
    assert!(md.contains("RestBoost (default)") && md.contains("Fixed"));
    let c = std::fs::read_to_string(dir.join("OQ16.csv")).unwrap();
    assert_eq!(c.lines().count(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["sweep", "OQ99"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn affect_grid_scales_each_rate_by_its_label() {
    let base = BrainConfig::default().affect;
    let s = sweep("OQ17").unwrap();
    let v = s
        .variants
        .iter()
        .find(|v| v.label == "rise×2 recover×0.5")
        .unwrap();
    assert_eq!(v.config.affect.d_rise, base.d_rise * 2.0);
    assert_eq!(v.config.affect.d_recover, base.d_recover * 0.5);
}

#[test]
fn one_run_is_measured_and_every_scenario_is_listed() {
    let r = run_one(&BrainConfig::default(), "first_night", 0);
    // The run reaches the end; an action already under way finishes, so
    // the last tick can pass 2,400 (here a long explore trip).
    assert!(r.survived && r.decisions > 0 && r.ticks >= 2400, "{r:?}");
    assert_eq!(all_scenarios(), sim::scenario::NAMES.to_vec());
}

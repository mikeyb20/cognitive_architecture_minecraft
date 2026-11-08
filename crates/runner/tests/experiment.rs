//! Experiments (spec §26; plan §5): parsing and checking the TOML, brain
//! settings per arm, baseline metrics by hand, the run, the tables and the
//! CLI.

use cognitive_core::agent::BrainConfig;
use runner::archetype::{drawn, means};
use runner::brain::BrainResult;
use runner::experiment::{ArmKind, baseline_metrics, config_for, header, markdown, parse, run};
use runner::sweep::csv_rows;
use std::process::Command;

const SMALL: &str = r#"
id = "SMALL"
question = "Does it run?"
scenarios = ["first_night"]
seeds = 2

[[arms]]
label = "A"
arm = "a"
expected = "survives"

[[arms]]
label = "C slow"
arm = "c"
config = { curiosity = { rate = 0.0 }, kick_enabled = false }
expected = "flat C"
"#;

#[test]
fn the_shipped_experiments_parse() {
    for f in ["baselines", "ablations"] {
        let text = std::fs::read_to_string(format!("../../configs/experiments/{f}.toml")).unwrap();
        let e = parse(&text).unwrap_or_else(|err| panic!("{f}: {err}"));
        assert!(e.arms.iter().all(|a| !a.expected.is_empty()), "{f}");
    }
}

#[test]
fn defaults_and_arm_settings() {
    let e = parse(SMALL).unwrap();
    assert_eq!((e.archetype.as_str(), e.per_agent), ("default", false));
    assert_eq!(e.arms[0].arm, ArmKind::A);
    let c = config_for(&e, &e.arms[1], 5).unwrap();
    let mut want = BrainConfig::default();
    want.curiosity.rate = 0.0;
    want.kick_enabled = false;
    assert_eq!(c, want, "only the named settings change");
    assert_eq!(
        config_for(&e, &e.arms[0], 5).unwrap(),
        BrainConfig::default()
    );
    let short = parse("id='x'\nquestion='q'\nscenarios=[]\narms=[]").unwrap();
    assert_eq!(short.seeds, 10);
}

#[test]
fn archetypes_and_per_agent_draws() {
    let text = SMALL.replace(
        "seeds = 2",
        "seeds = 2\narchetype = \"explorer\"\nper_agent = true",
    );
    let e = parse(&text).unwrap();
    let mut mean = means("explorer").unwrap();
    mean.curiosity.rate = 0.0;
    mean.kick_enabled = false;
    assert_eq!(config_for(&e, &e.arms[1], 3).unwrap(), drawn(&mean, 3));
}

#[test]
fn mistakes_are_errors() {
    let bad = |from: &str, to: &str| parse(&SMALL.replace(from, to)).unwrap_err();
    assert_eq!(bad("first_night", "moon"), "unknown scenario \"moon\"");
    assert_eq!(
        bad("rate = 0.0", "rat = 0.0"),
        "unknown setting curiosity.rat"
    );
    assert_eq!(bad("kick_enabled", "kick_on"), "unknown setting kick_on");
    assert!(bad("kick_enabled = false", "kick_enabled = 3").starts_with("C slow: "));
    let a_cfg = SMALL.replace("arm = \"a\"", "arm = \"a\"\nconfig = { tm_k = 0 }");
    assert_eq!(parse(&a_cfg).unwrap_err(), "A: arm a has no brain settings");
    assert!(parse(&SMALL.replace("arm = \"a\"", "arm = \"z\"")).is_err());
    assert!(parse(&SMALL.replace("seeds = 2", "seedz = 2")).is_err());
    let bad_arch = SMALL.replace("seeds = 2", "seeds = 2\narchetype = \"nobody\"");
    assert_eq!(
        parse(&bad_arch).unwrap_err(),
        "no archetype called \"nobody\""
    );
}

#[test]
fn baseline_metrics_by_hand() {
    let r = BrainResult {
        scenario: "first_night",
        seed: 0,
        survived: true,
        ticks: 2400,
        death: None,
        decisions: 8,
        idle: 2,
        thoughts: 0,
        reflexes: 1,
        new_things: 4,
        inventory: vec![("wooden_pickaxe".into(), 1)],
    };
    let m = baseline_metrics(&r);
    assert_eq!((m.survived, m.ticks, m.decisions), (true, 2400, 8));
    assert_eq!((m.active, m.self_started), (0.75, 0.75));
    assert!(m.mean_c.is_nan() && m.mean_d.is_nan() && m.mean_v.is_nan() && m.freeze.is_nan());
    assert_eq!(
        (m.stuck_kicks, m.reflexes, m.new_things, m.tech),
        (0, 1, 4, 1)
    );
    let none = baseline_metrics(&BrainResult {
        decisions: 0,
        idle: 0,
        ..r
    });
    assert_eq!(none.active, 0.0);
}

#[test]
fn runs_replay_and_tabulate() {
    let e = parse(SMALL).unwrap();
    let rows = run(&e, 1);
    let again = run(&e, 3);
    assert_eq!(
        format!("{rows:?}"),
        format!("{again:?}"),
        "thread count does not matter"
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].variant.as_str(), rows[1].variant.as_str()),
        ("A", "C slow")
    );
    assert_eq!(rows[0].summary.runs, 2);
    assert!(rows[0].summary.mean_c.is_nan() && !rows[1].summary.mean_c.is_nan());
    assert_eq!(
        rows[0].summary.survival, 1.0,
        "arm A survives the first night"
    );
    let md = markdown(&e, &rows);
    assert!(md.starts_with("## SMALL: Does it run?\n\nExpected before the run (plan §5.5):\n\n- **A**: survives\n- **C slow**: flat C\n\n2 seeds per cell."), "{md}");
    assert!(md.contains("| A | first_night | 1.00 |"), "{md}");
    assert!(
        md.contains(" | – | – | – | – | "),
        "dashes for A's missing variables: {md}"
    );
    let csv = csv_rows("experiment", &e.id, &rows);
    assert!(csv.starts_with("experiment,variant,scenario,survival"));
    assert!(
        csv.lines()
            .nth(1)
            .unwrap()
            .starts_with("SMALL,A,first_night,1.00")
    );
    let h = header(&e);
    assert_eq!(h["experiment"], "SMALL");
    assert_eq!(h["arms"][1]["expected"], "flat C");
    assert_eq!(h["arms"][1]["config"]["curiosity"]["rate"], 0.0);
    assert_eq!(
        (h["seeds"].as_u64(), h["per_agent"].as_bool()),
        (Some(2), Some(false))
    );
    assert_eq!(h["spec_version"], cognitive_core::SPEC_VERSION);
}

#[test]
fn the_run_command_writes_header_table_and_csv() {
    let dir = std::env::temp_dir().join(format!("exp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("small.toml");
    std::fs::write(&file, SMALL).unwrap();
    let out_dir = dir.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args([
            "run",
            "--experiment",
            file.to_str().unwrap(),
            "--seeds",
            "1",
        ])
        .args(["--out", out_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let md = std::fs::read_to_string(out_dir.join("SMALL.md")).unwrap();
    assert!(
        md.contains("1 seeds per cell"),
        "--seeds overrides the file: {md}"
    );
    assert!(
        std::fs::read_to_string(out_dir.join("SMALL.csv"))
            .unwrap()
            .starts_with("experiment,")
    );
    let head = std::fs::read_to_string(out_dir.join("SMALL.header.json")).unwrap();
    assert!(head.contains("\"expected\":\"survives\""), "{head}");
    let fails = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_runner"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2));
        String::from_utf8(o.stderr).unwrap()
    };
    assert!(fails(&["run"]).starts_with("usage: runner run"));
    let missing = dir.join("none.toml");
    assert!(fails(&["run", "--experiment", missing.to_str().unwrap()]).contains("none.toml"));
    std::fs::write(&file, "id = 3").unwrap();
    assert!(fails(&["run", "--experiment", file.to_str().unwrap()]).contains("small.toml: "));
    std::fs::remove_dir_all(dir).unwrap();
}

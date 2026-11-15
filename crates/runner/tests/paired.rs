//! One runner path (07 M3) and paired results (07 M1, M4): `runner brain`
//! and the experiment runner agree seed by seed, and experiments write every
//! run, a reference arm, the commit and the config hash.

use cognitive_core::agent::BrainConfig;
use runner::brain::{floored, run_brain};
use runner::experiment::{header, parse, reference, run, run_arm};
use std::process::Command;

const TWO: &str = r#"
id = "TWO"
question = "Paired?"
scenarios = ["first_night"]
seeds = 3

[[arms]]
label = "C"
arm = "c"
expected = "-"

[[arms]]
label = "A"
arm = "a"
expected = "-"
"#;

fn survived_line(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    text.lines().last().unwrap_or_default().to_string()
}

#[test]
fn runner_brain_and_the_experiment_runner_agree_seed_by_seed() {
    let mut e = parse(TWO).unwrap();
    for floor in [true, false] {
        e.floor = floor;
        let mut alive = 0;
        for seed in 0..3 {
            let r = run_brain(
                floored("first_night", seed, floor),
                BrainConfig::default(),
                seed,
                |_| {},
            );
            let (m, _) = run_arm(&e, &e.arms[0], "first_night", seed, None).unwrap();
            let got = (m.survived, m.ticks, m.decisions, m.new_things);
            assert_eq!(got, (r.survived, r.ticks, r.decisions, r.new_things));
            alive += usize::from(r.survived);
        }
        let mut args = vec!["brain", "first_night", "3"];
        let on = if floor { "on" } else { "off" };
        if !floor {
            args.push("--no-floor");
        }
        let want = format!("brain first_night: {alive}/3 survived (survival floor {on})");
        assert_eq!(survived_line(&args), want);
    }
}

#[test]
fn experiments_pair_every_arm_with_the_reference() {
    let e = parse(TWO).unwrap();
    assert_eq!(reference(&e), "C", "the first arm by default");
    assert_eq!(header(&e)["reference"], "C");
    let res = run(&e, 2, None).unwrap();
    assert_eq!(res.runs.len(), 6);
    let order: Vec<(&str, u64)> = res.runs.iter().map(|r| (r.arm.as_str(), r.seed)).collect();
    assert_eq!(
        order,
        [("C", 0), ("C", 1), ("C", 2), ("A", 0), ("A", 1), ("A", 2)]
    );
    let md = runner::experiment::markdown(&e, &res);
    assert!(md.contains("Paired by seed against **C**"), "{md}");
    assert!(md.contains("| A | first_night | 3 |"), "{md}");
    let named = parse(&TWO.replace("seeds = 3", "seeds = 3\nreference = \"A\"")).unwrap();
    assert_eq!(reference(&named), "A");
    let bad = parse(&TWO.replace("seeds = 3", "seeds = 3\nreference = \"Z\""));
    assert_eq!(bad.unwrap_err(), "reference \"Z\" is not an arm");
}

#[test]
fn the_run_command_writes_runs_commit_and_config_hash() {
    let dir = std::env::temp_dir().join(format!("paired-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("two.toml");
    std::fs::write(&file, TWO).unwrap();
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
    let runs = std::fs::read_to_string(out_dir.join("TWO.runs.csv")).unwrap();
    let lines: Vec<&str> = runs.lines().collect();
    assert_eq!(lines.len(), 3, "{runs}");
    assert!(lines[0].starts_with("arm,scenario,seed,survived"));
    assert!(lines[1].starts_with("\"C\",first_night,0,"), "{runs}");
    let head: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("TWO.header.json")).unwrap())
            .unwrap();
    assert_eq!(head["config_hash"], runner::paired::config_hash(TWO));
    // The same answer git gives here (a checkout, or not).
    let git = |a: &[&str]| {
        Command::new("git")
            .args(a)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let want = runner::paired::commit_label(
        git(&["rev-parse", "HEAD"]),
        git(&["status", "--porcelain", "--untracked-files=no"]),
    );
    assert_eq!(head["commit"], want);
    std::fs::remove_dir_all(dir).unwrap();
}

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
        .env("OLLAMA_MODEL", "tiny:1b")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty(), "no LLM arm, no progress line");
    let runs = std::fs::read_to_string(out_dir.join("TWO.runs.csv")).unwrap();
    let lines: Vec<&str> = runs.lines().collect();
    assert_eq!(lines.len(), 3, "{runs}");
    assert!(lines[0].starts_with("arm,scenario,seed,survived"));
    assert!(lines[1].starts_with("\"C\",first_night,0,"), "{runs}");
    let head: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("TWO.header.json")).unwrap())
            .unwrap();
    assert_eq!(head["config_hash"], runner::paired::config_hash(TWO));
    assert_eq!(head["llm"], "openai/tiny:1b", "the model the LLM arms ask");
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

#[test]
fn llm_runs_print_progress_on_stderr() {
    let one_b = r#"
id = "ONE_B"
question = "Progress?"
scenarios = ["first_night"]
seeds = 1

[[arms]]
label = "C"
arm = "c"
expected = "-"

[[arms]]
label = "B"
arm = "b"
expected = "-"
"#;
    let e = parse(one_b).unwrap();
    assert_eq!(runner::experiment::llm_runs(&e), 1);
    assert_eq!(runner::experiment::llm_runs(&parse(TWO).unwrap()), 0);
    let dir = std::env::temp_dir().join(format!("progress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("one_b.toml");
    std::fs::write(&file, one_b).unwrap();
    // Nothing listens on port 9, so every call fails fast and falls back.
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args([
            "run",
            "--experiment",
            file.to_str().unwrap(),
            "--tier",
            "local",
        ])
        .env("OLLAMA_BASE_URL", "http://127.0.0.1:9")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("\rLLM run 1/1, call 1 | "), "{err}");
    assert!(err.ends_with("after the first run   \n"), "{err}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("LLM run 1/1"), "results stay clean");
    std::fs::remove_dir_all(dir).unwrap();
}

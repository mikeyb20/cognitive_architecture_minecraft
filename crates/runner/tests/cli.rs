//! Smoke test: the runner binary starts and reports the versions it was built
//! against.

use std::process::Command;

#[test]
fn runner_prints_versions() {
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .output()
        .expect("runner should start");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("defs 01 v3"), "{text}");
    assert!(text.contains("spec 0.2"), "{text}");
}

#[test]
fn runner_first_night_reports_survival() {
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["first-night", "3"])
        .output()
        .expect("runner should start");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("arm A first-night survival: 3/3"), "{text}");
    assert!(
        !text.contains("died"),
        "survivors are not listed as deaths: {text}"
    );
}

#[test]
fn runner_reports_deaths() {
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["first-night", "x"])
        .output()
        .expect("runner should start");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("/100"), "bad count falls back to 100: {text}");
}

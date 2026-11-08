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
    assert!(text.contains("defs 01 v2"), "{text}");
    assert!(text.contains("spec 0.2"), "{text}");
}

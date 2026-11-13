//! The `runner bench` command (benchmarks spec v0.1).

use std::process::Command;

#[test]
fn bench_command_writes_tables() {
    let dir = std::env::temp_dir().join(format!("bench-cli-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["bench", "habituation", "--out", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("## BENCH habituation: 4 of 6"), "{stdout}");
    let md = std::fs::read_to_string(dir.join("BENCH-habituation.md")).unwrap();
    assert!(md.contains("H5"));
    let c = std::fs::read_to_string(dir.join("BENCH-habituation.csv")).unwrap();
    assert_eq!(c.lines().count(), 7);
    std::fs::remove_dir_all(&dir).unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["bench", "nope"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

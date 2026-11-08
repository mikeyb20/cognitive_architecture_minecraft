//! Theory tests (spec §29, step 5d): the registry, the probes by hand, the
//! cross-cell lines, the tables and the CLI.

use cognitive_core::agent::{BrainConfig, TickLog};
use cognitive_core::modes::Mode;
use runner::brain::BrainResult;
use runner::theory::probes::{
    Behaviour, Insight, Kind, Mean, ModeSuccess, Perform, Probe, STUCK_STREAK, Streaks, Urgency,
    Windows, kind,
};
use runner::theory::report::{loo_accuracy, num};
use runner::theory::{
    Cell, FORCE_BIAS, claim, csv, forced, ids, independence, markdown, mode_accuracy, run_claim,
};
use sim::scenario::{DAY, NAMES};
use std::process::Command;

fn result(inventory: &[(&str, u32)]) -> BrainResult {
    BrainResult {
        scenario: "first_night",
        seed: 0,
        survived: true,
        ticks: 10,
        death: None,
        decisions: 0,
        idle: 0,
        thoughts: 0,
        reflexes: 0,
        new_things: 9,
        inventory: inventory.iter().map(|(i, n)| (i.to_string(), *n)).collect(),
    }
}

fn row(chosen: &str) -> TickLog {
    TickLog {
        chosen: chosen.into(),
        ..TickLog::default()
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn every_claim_is_registered_once_with_known_scenarios() {
    let want = [
        "T-IND", "KICK", "T-MODE", "SUBC", "EFFORT", "HARSH", "BETA-U",
    ];
    assert_eq!(ids(), want);
    for id in want {
        let c = claim(&id.to_lowercase()).unwrap();
        assert_eq!(c.id, id);
        assert!(c.claim.contains("00 §"), "{}", c.claim);
        assert!(!c.reading.is_empty() && !c.columns.is_empty() && !c.variants.is_empty());
        assert!(c.scenarios.iter().all(|s| NAMES.contains(s)), "{id}");
        let labels: Vec<&str> = c.variants.iter().map(|v| v.label.as_str()).collect();
        let mut dedup = labels.clone();
        dedup.dedup();
        assert_eq!(labels, dedup, "{id}: labels are distinct");
    }
    assert!(claim("T-NOPE").is_none());
}

#[test]
fn forced_pins_one_mode_and_stops_the_oracle_learning() {
    let base = BrainConfig {
        oracle_bias: [1.0; 4],
        ..BrainConfig::default()
    };
    let c = forced(Mode::Exploring, base);
    assert_eq!(c.oracle_bias, [0.0, FORCE_BIAS, 0.0, 0.0]);
    assert_eq!(c.oracle_eta, 0.0);
    assert_eq!(FORCE_BIAS, 8.0);
}

#[test]
fn each_claim_turns_the_right_knob() {
    let d = BrainConfig::default();
    let t = claim("T-IND").unwrap().variants;
    assert_eq!(t.len(), 6);
    assert_eq!(t[0].config.lens_frozen, [true; 4]);
    assert_eq!(t[2].label, "only Exploring learns");
    assert_eq!(t[2].config.lens_frozen, [true, false, true, true]);
    assert_eq!(t[5].config, d);
    let k = claim("KICK").unwrap().variants;
    assert!(k[0].config.kick_enabled && !k[1].config.kick_enabled);
    let m = claim("T-MODE").unwrap().variants;
    for (v, mode) in m.iter().zip(Mode::ALL) {
        assert_eq!(v.config, forced(mode, BrainConfig::default()));
    }
    let s = claim("SUBC").unwrap().variants;
    assert_eq!(s[0].config, d);
    assert_eq!(s[1].config.subconscious.b_base, 0.0);
    let e = claim("EFFORT").unwrap().variants;
    let bases: Vec<f32> = e.iter().map(|v| v.config.curiosity.base).collect();
    assert_eq!(bases, [0.2, 0.4, 0.6, 0.8]);
    assert!(
        e.iter()
            .all(|v| v.config.oracle_bias[Mode::Performing.index()] == FORCE_BIAS)
    );
    let b = claim("BETA-U").unwrap().variants;
    let betas: Vec<f32> = b.iter().map(|v| v.config.beta_u).collect();
    assert_eq!(betas, [0.0, 0.2, 0.4]);
    assert_eq!(b[1].label, "β_U 0.2 (default)");
    assert_eq!(claim("HARSH").unwrap().variants[0].config, d);
}

#[test]
fn kinds_follow_the_logged_label() {
    let cases = [
        ("Explore:1,2", Kind::Travel),
        ("MoveTo:3,4", Kind::Travel),
        ("FollowPlayer", Kind::Travel),
        ("Chop:1,1", Kind::Gather),
        ("Mine:1,1", Kind::Gather),
        ("Farm:1,1", Kind::Gather),
        ("Attack:1,1", Kind::Gather),
        ("Craft:stick", Kind::Craft),
        ("Place", Kind::Craft),
        ("Eat:bread", Kind::Survive),
        ("Flee:2,2", Kind::Survive),
        ("Shelter", Kind::Survive),
        ("Rest", Kind::Survive),
        ("Plan", Kind::Think),
        ("Reflect", Kind::Think),
        ("Imagine", Kind::Think),
        ("Rehearse", Kind::Think),
        ("Consolidate", Kind::Think),
        ("idle", Kind::Idle),
        ("Chat", Kind::Other),
        ("", Kind::Other),
    ];
    for (label, k) in cases {
        assert_eq!(kind(label), k, "{label}");
    }
}

#[test]
fn mean_is_nan_when_empty() {
    let mut m = Mean::default();
    assert!(m.get().is_nan());
    m.add(1.0);
    m.add(2.0);
    assert!(close(m.get(), 1.5));
}

#[test]
fn mode_success_skips_idle() {
    let mut p = ModeSuccess::default();
    let mut a = row("Chop:1,1");
    a.success = [1.0, 0.5, 0.0, -1.0];
    let mut b = row("Plan");
    b.success = [0.0, 0.5, 1.0, 0.0];
    let mut idle = row("idle");
    idle.success = [9.0; 4];
    for r in [&a, &b, &idle] {
        p.push(r);
    }
    assert_eq!(p.finish(&result(&[])), [0.5, 0.5, 0.5, -0.5]);
}

#[test]
fn streaks_count_stuck_runs_by_hand() {
    assert_eq!(STUCK_STREAK, 50);
    let mut p = Streaks::default();
    let mut feed = |label: &str, n: u32| {
        for _ in 0..n {
            p.push(&row(label));
        }
    };
    feed("idle", 60);
    feed("Chop:1,1", 3);
    feed("idle", 49);
    feed("Plan", 50);
    let mut k = row("Plan");
    k.stuck_kick = true;
    p.push(&k);
    // Streaks: 60 (stuck), 3, 49 (not), 51 (stuck, still open at the end).
    assert_eq!(p.finish(&result(&[])), [2.0, 55.5, 60.0, 1.0]);
    let empty = Streaks::default().finish(&result(&[]));
    assert_eq!(empty[0], 0.0);
    assert!(empty[1].is_nan() && empty[2] == 0.0);
}

#[test]
fn behaviour_profile_by_hand() {
    let mut p = Behaviour::default();
    let mut feed = |label: &str, effort: f32| {
        let mut r = row(label);
        r.effort = effort;
        p.push(&r);
    };
    feed("Explore:1,1", 0.4);
    feed("Chop:1,1", 0.6);
    feed("Craft:stick", 0.2);
    feed("Plan", 0.9);
    feed("idle", 0.9);
    feed("Eat:bread", 0.8);
    let got = p.finish(&result(&[]));
    let sixth = 1.0 / 6.0;
    let want = [sixth, sixth, sixth, sixth, sixth, 0.5];
    assert!(got.iter().zip(want).all(|(g, w)| close(*g, w)), "{got:?}");
    let none = Behaviour::default().finish(&result(&[]));
    assert_eq!(none[..5], [0.0; 5]);
}

#[test]
fn insight_perform_windows_and_urgency_by_hand() {
    let mut i = Insight::default();
    let mut r = row("Imagine");
    (r.formed, r.sub_processed) = (2, 5);
    i.push(&r);
    i.push(&r);
    let inv = [("wooden_pickaxe", 1), ("furnace", 0), ("stone_pickaxe", 2)];
    assert_eq!(i.finish(&result(&inv)), [2.0, 4.0, 10.0, 9.0]);

    let mut p = Perform::default();
    let mut act = row("Chop:1,1");
    (act.c, act.effort) = (0.5, 0.4);
    act.success[Mode::Performing.index()] = 1.0;
    let mut idle = row("idle");
    (idle.c, idle.effort) = (0.1, 0.9);
    idle.success = [0.0; 4];
    p.push(&act);
    p.push(&idle);
    let got = p.finish(&result(&[("iron_pickaxe", 1)]));
    assert!(close(got[0], 0.3) && close(got[1], 0.4) && close(got[2], 1.0));
    assert_eq!(got[3], 1.0);

    let mut w = Windows::default();
    for (tick, c, d) in [
        (0, 9.0, 9.0),
        (DAY / 2, 0.2, 0.6),
        (DAY - 1, 0.4, 0.2),
        (DAY, 0.5, 0.1),
        (2 * DAY - 1, 0.7, 0.3),
        (2 * DAY, 9.0, 9.0),
    ] {
        let mut r = row("idle");
        (r.tick, r.c, r.d) = (tick, c, d);
        w.push(&r);
    }
    let got = w.finish(&result(&[]));
    let want = [0.3, 0.6, 0.4, 0.2];
    assert!(got.iter().zip(want).all(|(g, w)| close(*g, w)), "{got:?}");
    assert!(
        Windows::default()
            .finish(&result(&[]))
            .iter()
            .all(|x| x.is_nan())
    );

    let mut u = Urgency::default();
    for (label, c, uu, we) in [("Explore:1,1", 0.2, 0.1, 0.4), ("idle", 0.4, 0.3, 0.2)] {
        let mut r = row(label);
        (r.c, r.u) = (c, uu);
        r.w[Mode::Exploring.index()] = we;
        u.push(&r);
    }
    let got = u.finish(&result(&[]));
    let want = [0.3, 0.2, 0.3, 0.5];
    assert!(got.iter().zip(want).all(|(g, w)| close(*g, w)), "{got:?}");
}

fn cell(variant: &str, scenario: &str, runs: Vec<Vec<f64>>) -> Cell {
    Cell {
        variant: variant.into(),
        scenario: scenario.into(),
        runs,
    }
}

#[test]
fn cell_means_leave_out_missing_data() {
    let c = cell(
        "a",
        "s",
        vec![vec![1.0, f64::NAN, 2.0], vec![0.0, f64::NAN, 4.0]],
    );
    let m = c.means();
    assert!(close(m[0], 0.5) && m[1].is_nan() && close(m[2], 3.0));
    assert!(cell("a", "s", vec![]).means().is_empty());
    assert_eq!(
        (num(0.125), num(f64::NAN)),
        ("0.12".to_string(), "–".to_string())
    );
}

#[test]
fn nearest_centroid_by_hand() {
    let pts = vec![
        (0, vec![0.0, 0.0]),
        (0, vec![0.1, 0.0]),
        (1, vec![1.0, 1.0]),
        (1, vec![0.9, 1.0]),
    ];
    assert_eq!(loo_accuracy(&pts), 1.0);
    // Class 1's point at 0.1 sits on class 0's centre; its point at 1.0 is
    // equally far from both centres, and a tie goes to the first class.
    let mixed = vec![
        (0, vec![0.0]),
        (0, vec![0.2]),
        (1, vec![0.1]),
        (1, vec![1.0]),
    ];
    assert_eq!(loo_accuracy(&mixed), 0.5);
    assert!(loo_accuracy(&pts[..1]).is_nan());
}

#[test]
fn cross_cell_lines_by_hand() {
    let base = vec![vec![1.0, 0.5, 0.5, 0.5, 0.5]];
    let rows = vec![
        cell("all frozen", "open_day", base),
        cell(
            "only Learning learns",
            "open_day",
            vec![vec![1.0, 0.9, 0.55, 0.5, 0.45]],
        ),
        cell(
            "only Exploring learns",
            "open_day",
            vec![vec![1.0, 0.5, 0.0, 0.5, 0.5]],
        ),
    ];
    let line = independence(&rows);
    assert!(
        line.contains("open_day: Learning 10.0%, Exploring 0.0%"),
        "{line}"
    );
    let zero = vec![
        cell("all frozen", "x", vec![vec![1.0, 0.0, 0.0, 0.0, 0.0]]),
        cell(
            "only Learning learns",
            "x",
            vec![vec![1.0, 0.0, 0.0, 0.0, 0.0]],
        ),
    ];
    assert!(independence(&zero).contains("x: Learning –"));
    assert!(independence(&rows[1..]).ends_with(": "), "no base row");

    let modes = vec![
        cell("forced Learning", "s", vec![vec![0.0, 0.0], vec![0.0, 0.1]]),
        cell(
            "forced Exploring",
            "s",
            vec![vec![0.0, 1.0], vec![0.0, f64::NAN]],
        ),
    ];
    // The NaN reads as 0, so the second Exploring run looks like Learning,
    // and without it the first one's own class centre sits at 0.
    let line = mode_accuracy(&modes);
    assert!(line.ends_with("s 0.50"), "{line}");
}

#[test]
fn claims_replay_and_tabulate() {
    let c = claim("HARSH").unwrap();
    let one = run_claim(&c, 1, 1);
    assert_eq!(one, run_claim(&c, 1, 3));
    assert_eq!(one.len(), 2);
    assert_eq!(
        (one[0].scenario.as_str(), one[1].scenario.as_str()),
        ("harsh", "open_day")
    );
    assert!(
        one.iter()
            .all(|r| r.runs.len() == 1 && r.runs[0].len() == 5)
    );
    let md = markdown(&c, &one, 1);
    assert!(md.starts_with("## HARSH: Hostile environments drop C"));
    assert!(md.contains("| variant | scenario | survival | C_during |"));
    assert_eq!(md.lines().filter(|l| l.starts_with('|')).count(), 4);
    let mut t = claim("T-MODE").unwrap();
    t.scenarios = &["first_night"];
    let rows = run_claim(&t, 2, 2);
    assert_eq!(rows.len(), 4);
    let md = markdown(&t, &rows, 2);
    assert!(md.contains("Nearest-centroid accuracy"), "{md}");
    let text = csv(&c, &one);
    assert!(text.starts_with("claim,variant,scenario,survival,C_during"));
    assert!(
        text.lines()
            .nth(1)
            .unwrap()
            .starts_with("HARSH,default,harsh,")
    );
    let quoted = csv(&c, &[cell("a, b", "s", vec![vec![1.0; 5]])]);
    assert!(quoted.contains("HARSH,\"a, b\",s,1.00"), "{quoted}");
}

#[test]
fn theory_command_writes_tables() {
    let dir = std::env::temp_dir().join(format!("theory-cli-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["theory", "harsh", "--seeds", "1"])
        .args(["--out", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("## HARSH"));
    assert!(
        std::fs::read_to_string(dir.join("HARSH.md"))
            .unwrap()
            .contains("1 seeds")
    );
    let c = std::fs::read_to_string(dir.join("HARSH.csv")).unwrap();
    assert_eq!(c.lines().count(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_runner"))
        .args(["theory", "T-NOPE"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

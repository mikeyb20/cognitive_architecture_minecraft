//! P1 and P2 personality tests (plan §5.3; spec §19): the traits probe, the
//! standardising and spread maths by hand, and per-agent draws.

use cognitive_core::agent::TickLog;
use runner::archetype::{ARCHETYPES, agent, drawn, means};
use runner::brain::{BrainResult, run_brain, setup};
use runner::theory::probes::{Probe, Traits};
use runner::theory::report::{personality_lines, spread, standardise};
use runner::theory::{Cell, claim, run_probe};
use sim::scenario::DAY;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn result(ticks: u64, new_things: usize) -> BrainResult {
    BrainResult {
        scenario: "open_day",
        seed: 0,
        survived: true,
        ticks,
        death: None,
        decisions: 0,
        idle: 0,
        thoughts: 0,
        reflexes: 0,
        new_things,
        inventory: vec![],
    }
}

#[test]
fn traits_by_hand() {
    let mut p = Traits::default();
    for (label, effort, commanded) in [
        ("Explore:1,1", 0.4, false),
        ("Chop:1,1", 0.6, true),
        ("Craft:stick", 0.2, true),
        ("Flee:2,2", 0.8, false),
        ("Plan", 0.9, false),
        ("idle", 0.9, false),
        ("Chat", 0.0, false),
        ("idle", 0.9, false),
    ] {
        p.push(&TickLog {
            chosen: label.into(),
            effort,
            external_drive: commanded,
            ..TickLog::default()
        });
    }
    let got = p.finish(&result(2 * DAY, 10));
    // 8 decisions; effort over the five that acted (not idle, not think).
    let want = [
        0.125,
        0.125,
        0.125,
        0.125,
        0.125,
        0.25,
        0.25,
        (0.4 + 0.6 + 0.2 + 0.8 + 0.0) / 5.0,
        5.0,
    ];
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-6, "{got:?}");
    }
    let empty = Traits::default().finish(&result(0, 3));
    assert_eq!(empty[..7], [0.0; 7]);
    assert!(empty[7].is_nan(), "no actions, no effort");
    assert!(
        close(empty[8], 3.0 * DAY as f64),
        "a zero-tick run counts as one tick"
    );
}

#[test]
fn standardise_by_hand() {
    let mut pts = vec![(0, vec![1.0, 5.0, 2.0]), (1, vec![3.0, 5.0, 6.0])];
    standardise(&mut pts);
    assert_eq!(pts[0].1, [-1.0, 0.0, -1.0], "a constant column becomes 0");
    assert_eq!(pts[1].1, [1.0, 0.0, 1.0]);
    let mut three = vec![(0, vec![0.0]), (0, vec![0.0]), (1, vec![3.0])];
    standardise(&mut three);
    // mean 1, sd √2.
    let s = 2f64.sqrt();
    assert!(close(three[0].1[0], -1.0 / s) && close(three[2].1[0], 2.0 / s));
}

#[test]
fn spread_by_hand() {
    // Class centroids 1 and 11, overall 6: within 1, between 25.
    let pts = vec![
        (0, vec![0.0]),
        (0, vec![2.0]),
        (1, vec![10.0]),
        (1, vec![12.0]),
    ];
    assert_eq!(spread(&pts), (1.0, 25.0));
    // Unequal classes: centroids 0 and 3, overall 1.
    let uneven = vec![(0, vec![-1.0]), (0, vec![1.0]), (1, vec![3.0])];
    let (w, b) = spread(&uneven);
    assert!(close(w, 2.0 / 3.0) && close(b, (1.0 + 1.0 + 4.0) / 3.0));
}

#[test]
fn personality_lines_by_hand() {
    // Two well-separated classes in one scenario, mixed ones in another.
    let cell = |v: &str, sc: &str, xs: &[f64]| Cell {
        variant: v.into(),
        scenario: sc.into(),
        runs: xs.iter().map(|x| vec![1.0, *x, f64::NAN]).collect(),
    };
    let rows = [
        cell("a", "s1", &[0.0, 1.0]),
        cell("b", "s1", &[10.0, 11.0]),
        cell("a", "s2", &[0.0, 10.0]),
        cell("b", "s2", &[1.0, 11.0]),
    ];
    let text = personality_lines(&rows);
    // s1: within 0.25 and between 25 before scaling; the ratio is scale-free.
    assert_eq!(
        text,
        "P1 nearest-centroid accuracy (leave one out, standardised; chance 0.25): s1 1.00, s2 0.00\n\n\
         P2 within/between spread (individual if above 0, distinct if below 1): s1 0.01, s2 100.00"
    );
}

#[test]
fn each_seed_is_a_different_agent_of_the_archetype() {
    let c = claim("P1-P2").unwrap();
    let names: Vec<&str> = c.variants.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(names, ARCHETYPES);
    assert!(c.per_agent);
    assert_eq!(c.variants[3].config, means("helper").unwrap());
    assert_eq!(
        drawn(&means("helper").unwrap(), 2),
        agent("helper", 2).unwrap()
    );
    // The probe runs the drawn agent, not the means.
    let mut p = Traits::default();
    let r = run_brain(
        setup("first_night", 2),
        agent("helper", 2).unwrap(),
        2,
        |row| p.push(row),
    );
    let mut want = vec![f64::from(u8::from(r.survived))];
    want.extend(p.finish(&r));
    let mut c = c;
    c.scenarios = &["first_night"];
    let got = run_probe(&c, 3, "first_night", 2);
    assert_eq!(format!("{got:?}"), format!("{want:?}"));
    c.per_agent = false;
    assert_ne!(
        format!("{:?}", run_probe(&c, 3, "first_night", 2)),
        format!("{want:?}")
    );
}

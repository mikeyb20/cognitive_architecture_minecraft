//! Build step 4 gates (spec §27): drift, shutdown and freeze reproducible on
//! demand (01 §1.5), the stuck kick escapes a loop, and the golden trace is
//! stable. The speed gate runs in CI as `runner speed` in a release build.

use cognitive_core::activation::{CuriosityParams, SignalWeights};
use cognitive_core::agent::digest::{float_sums, sums_close, trace_digest};
use cognitive_core::agent::{Brain, BrainConfig, NeedParams, TickLog};
use runner::brain::{BrainResult, run_with, setup};

fn no_signals() -> SignalWeights {
    SignalWeights {
        novelty: 0.0,
        learning_progress: 0.0,
        energy: 0.0,
        threat: 0.0,
        habituation: 0.0,
        social: 0.0,
    }
}

/// Drift (01 §1.5): C ≈ 0 with nothing to raise it, V ≈ 0, no D. Only
/// commands push it; hunger's push (01 §1.6) is checked on its own below.
fn drift_config() -> BrainConfig {
    BrainConfig {
        curiosity: CuriosityParams {
            base: 0.0,
            rate: 0.1,
        },
        k: no_signals(),
        kick_enabled: false,
        needs: NeedParams {
            on: false,
            ..NeedParams::default()
        },
        ..BrainConfig::default()
    }
}

fn run(scenario: &str, seed: u64, mut brain: Brain) -> (Vec<TickLog>, BrainResult) {
    let mut rows = Vec::new();
    let r = run_with(setup(scenario, seed), &mut brain, seed, &mut |row| {
        rows.push(row.clone())
    });
    (rows, r)
}

fn self_generated(rows: &[TickLog]) -> usize {
    rows.iter()
        .filter(|r| r.chosen != "idle" && !r.external_drive)
        .count()
}

#[test]
fn drift_goes_where_commands_push_it() {
    for seed in 0..3 {
        let (rows, r) = run("commanded", seed, Brain::new(drift_config(), seed));
        assert!(rows.iter().all(|r| r.c < 0.01), "C stays near 0");
        assert!(rows.iter().all(|r| r.d < 0.1), "no suppression");
        let mean_v = rows.iter().map(|r| r.v.abs()).sum::<f32>() / rows.len() as f32;
        assert!(mean_v < 0.15, "V near 0: {mean_v}");
        assert_eq!(self_generated(&rows), 0, "no self-generated movement");
        assert!(rows.iter().filter(|r| r.external_drive).count() > 20);
        let has = |item: &str| r.inventory.iter().any(|(i, _)| i == item);
        assert!(has("wooden_pickaxe"), "commands still get done: {r:?}");
    }
    // With no one to push it, a drifting agent does nothing at all by day.
    let (rows, _) = run("first_night", 0, Brain::new(drift_config(), 0));
    let day: Vec<&TickLog> = rows.iter().filter(|r| r.tick < 1200).collect();
    assert!(day.len() > 1000 && day.iter().all(|r| r.chosen == "idle"));
}

#[test]
fn hunger_pushes_a_drifting_agent_like_a_command() {
    // 01 §1.6: a felt need is an outside-like force, so a drifting agent
    // (C ≈ 0) still goes where hunger pushes it, and starts nothing itself.
    let fed = BrainConfig {
        needs: NeedParams::default(),
        ..drift_config()
    };
    let (rows, r) = run("commanded", 0, Brain::new(fed, 0));
    let (_, unfed) = run("commanded", 0, Brain::new(drift_config(), 0));
    assert_eq!(self_generated(&rows), 0, "still no self-generated movement");
    let by_need = |r: &&TickLog| r.chosen != "idle" && r.need_push > 0.0 && r.need_push < 1.0;
    assert!(rows.iter().filter(by_need).count() > 20, "hunger pushes");
    assert!(
        rows.iter().any(|r| r.chosen.starts_with("Eat")),
        "and it eats"
    );
    assert!(r.ticks > unfed.ticks, "{} vs {}", r.ticks, unfed.ticks);
    assert!(rows.iter().all(|r| r.hunger == 0.0 || r.food < 0.75));
}

#[test]
fn shutdown_barely_responds() {
    // On demand: high D and negative V.
    let early =
        |rows: &[TickLog]| -> f32 { rows.iter().filter(|r| r.tick < 300).map(|r| r.d_goal).sum() };
    for seed in 0..3 {
        let mut b = Brain::new(drift_config(), seed);
        b.force_state(Some(0.0), Some((-0.6, 0.95)));
        let (shut, _) = run("commanded", seed, b);
        let first = &shut[0];
        assert!(first.c < 0.01 && first.v < 0.0 && first.d > 0.9);
        assert!(
            shut.iter()
                .filter(|r| r.tick < 300 && r.external_drive)
                .all(|r| r.effort < 0.05),
            "commanded effort ε_ext·(1 − D) is below the sim's movement floor"
        );
        assert_eq!(early(&shut), 0.0, "no progress on the command yet");
        let (drift, _) = run("commanded", seed, Brain::new(drift_config(), seed));
        assert!(early(&drift) > 0.0, "a drifting agent does respond");
    }
    // Naturally: the harsh episode drives D up and C down (seeds 0 and 2;
    // in seed 1 the agent shelters early and stays calm).
    for seed in [0, 2] {
        let (rows, _) = run("harsh", seed, Brain::new(BrainConfig::default(), seed));
        let after = rows.iter().find(|r| r.tick >= 2400).unwrap();
        assert!(after.d > 0.7, "D after the harsh night: {}", after.d);
        assert!(after.c < 0.3, "C after the harsh night: {}", after.c);
    }
}

#[test]
fn stuck_kick_escapes_a_loop() {
    // A drifting agent with no player is stuck idling. The stuck trigger
    // (N = 100 here) boosts C and flips the dominant mode, so it moves.
    let config = |kick| BrainConfig {
        kick_enabled: kick,
        kick: cognitive_core::control::kick::KickParams {
            n: 100,
            ..Default::default()
        },
        ..drift_config()
    };
    for seed in 0..3 {
        let (on, _) = run("first_night", seed, Brain::new(config(true), seed));
        let (off, _) = run("first_night", seed, Brain::new(config(false), seed));
        assert!(on.iter().filter(|r| r.stuck_kick).count() >= 5);
        assert!(
            self_generated(&on) > 0,
            "seed {seed}: the kick gets it moving"
        );
        assert_eq!(self_generated(&off), 0, "seed {seed}: without it, stuck");
        assert!(off.iter().all(|r| !r.stuck_kick));
    }
}

#[test]
fn golden_trace_is_stable() {
    let (rows, r) = run("first_night", 0, Brain::new(BrainConfig::default(), 0));
    let (again, _) = run("first_night", 0, Brain::new(BrainConfig::default(), 0));
    assert_eq!(
        trace_digest(&rows),
        trace_digest(&again),
        "same seed, same run"
    );
    // Pinned. Change only on a deliberate behaviour change, and say why in
    // the PR (CLAUDE.md "Testing").
    assert_eq!((r.decisions, r.idle, r.thoughts), GOLDEN_COUNTS, "{r:?}");
    assert_eq!(trace_digest(&rows), GOLDEN_HASH);
    let sums = float_sums(&rows);
    assert!(sums_close(sums, GOLDEN_SUMS, 1e-3), "{sums:?}");
}

/// A commanded run with a reflex, pinned whole: counts, reflexes and new
/// things. Change only on a deliberate behaviour change.
#[test]
fn golden_commanded_run_counts_reflexes_and_new_things() {
    let (_, r) = run("commanded", 0, Brain::new(BrainConfig::default(), 0));
    let got = (r.decisions, r.idle, r.thoughts, r.reflexes, r.new_things);
    assert_eq!(got, GOLDEN_COMMANDED, "{r:?}");
}

const GOLDEN_COMMANDED: (u32, u32, u32, u32, usize) = (1316, 1196, 66, 0, 54);
const GOLDEN_COUNTS: (u32, u32, u32) = (641, 584, 26);
const GOLDEN_HASH: u64 = 3_702_787_714_657_621_692;
/// Σ C, Σ D, Σ V, Σ R, Σ w_P (checked to 1e-3, see `digest`).
const GOLDEN_SUMS: [f64; 5] = [84.9208, 0.0, 36.5736, 32.4975, 473.5020];

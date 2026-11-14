//! B3 Triadic helplessness (benchmarks spec v0.1): D and V (01 §1.2, §1.3;
//! spec §12) after equal shock that the agent could or could not stop.
//! Design from Seligman & Maier (1967); reading from Maier & Seligman
//! (2016), Psychological Review 123, 349–367: only uncontrollable shock
//! leaves later passivity, and control is what is learned.

use super::Check;
use crate::brain::{BrainResult, run_brain, setup};
use cognitive_core::agent::BrainConfig;
use sim::scenario::triadic::{SHOCK_MAX_LEN, TRIADIC_PRE, yoked};

pub const ID: &str = "triadic";

/// Smallest D gap (yoked − none) that counts as a helplessness effect;
/// below it the control share is undefined (proposed).
pub const MIN_D_EFFECT: f64 = 0.05;
/// "Escapable ≈ none" on test escape means within this share (B3), and
/// "yoked < escapable" means lower by more than it, so noise cannot pass.
pub const SAME_SHARE: f64 = 0.1;

/// One arm's measures, averaged over seeds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmStats {
    /// D and V at the last decision of pretreatment.
    pub d_pre: f64,
    pub v_pre: f64,
    /// Share of test bouts ended by fleeing, and mean ticks of damage per
    /// test bout (escape latency; `SHOCK_MAX_LEN` when not escaped).
    pub escaped: f64,
    pub latency: f64,
}

/// Escaped share and mean damage ticks of the test bouts in `bouts`.
pub fn test_escape(bouts: &[(u64, u64)]) -> (f64, f64) {
    let test: Vec<u64> = bouts
        .iter()
        .filter(|(t, _)| *t >= TRIADIC_PRE)
        .map(|b| b.1)
        .collect();
    if test.is_empty() {
        return (0.0, 0.0);
    }
    let n = test.len() as f64;
    let escaped = test.iter().filter(|&&k| k < SHOCK_MAX_LEN).count() as f64;
    (escaped / n, test.iter().sum::<u64>() as f64 / n)
}

/// D and V at the last decision row (tick, D, V) before the test phase;
/// zero when there is none.
pub fn pre_state(rows: &[(u64, f32, f32)]) -> (f64, f64) {
    rows.iter()
        .rfind(|r| r.0 < TRIADIC_PRE)
        .map_or((0.0, 0.0), |r| (f64::from(r.1), f64::from(r.2)))
}

/// The pretreatment bouts of a run, which a yoked arm replays.
pub fn pretreatment(bouts: &[(u64, u64)]) -> Vec<(u64, u64)> {
    bouts
        .iter()
        .copied()
        .filter(|(t, _)| *t < TRIADIC_PRE)
        .collect()
}

/// One arm of one seed: the brain on `triadic`, with the pretreatment shock
/// replaced by `pre` (yoked or none) when given. Returns the stats and the
/// run, whose pretreatment bouts a yoked arm replays.
fn run_arm(seed: u64, pre: Option<Vec<(u64, u64)>>) -> (ArmStats, BrainResult) {
    let mut s = setup(ID, seed);
    if let Some(schedule) = pre {
        s.world.shock = Some(yoked(schedule));
    }
    let mut rows = Vec::new();
    let r = run_brain(s, BrainConfig::default(), seed, |row| {
        rows.push((row.tick, row.d, row.v));
    });
    let (d_pre, v_pre) = pre_state(&rows);
    let (escaped, latency) = test_escape(&r.shocks);
    let stats = ArmStats {
        d_pre,
        v_pre,
        escaped,
        latency,
    };
    (stats, r)
}

/// The three arms on one seed: escapable, yoked to it, and none.
pub fn triad(seed: u64) -> [ArmStats; 3] {
    let (esc, run) = run_arm(seed, None);
    let (yok, _) = run_arm(seed, Some(pretreatment(&run.shocks)));
    let (none, _) = run_arm(seed, Some(Vec::new()));
    [esc, yok, none]
}

/// Mean of each field over seeds.
pub fn mean(arms: &[ArmStats]) -> ArmStats {
    let n = arms.len().max(1) as f64;
    let sum = |f: fn(&ArmStats) -> f64| arms.iter().map(f).sum::<f64>() / n;
    ArmStats {
        d_pre: sum(|a| a.d_pre),
        v_pre: sum(|a| a.v_pre),
        escaped: sum(|a| a.escaped),
        latency: sum(|a| a.latency),
    }
}

fn row(name: &str, measured: f64, target: &str, pass: bool) -> Check {
    Check {
        bench: ID,
        name: name.into(),
        measured,
        target: target.into(),
        pass,
    }
}

/// The B3 judgments on seed-averaged arms (escapable, yoked, none).
pub fn judge(e: ArmStats, y: ArmStats, n: ArmStats) -> Vec<Check> {
    let effect = y.d_pre - n.d_pre;
    let share = if effect > 0.0 {
        (y.d_pre - e.d_pre) / effect
    } else {
        0.0
    };
    vec![
        row(
            "D after pretreatment, yoked − none",
            effect,
            "≥ 0.05 (a helplessness effect)",
            effect >= MIN_D_EFFECT,
        ),
        row(
            "control share of the D gap, (yoked − escapable) ÷ (yoked − none)",
            share,
            "≥ 0.5",
            effect >= MIN_D_EFFECT && share >= 0.5,
        ),
        row(
            "test escape share, escapable − yoked",
            e.escaped - y.escaped,
            "> 0.1",
            e.escaped - y.escaped > SAME_SHARE,
        ),
        row(
            "test escape share, escapable − none (absolute)",
            (e.escaped - n.escaped).abs(),
            "≤ 0.1",
            (e.escaped - n.escaped).abs() <= SAME_SHARE,
        ),
        row(
            "test escape share, escapable (reported)",
            e.escaped,
            "reported",
            true,
        ),
        row(
            "V after pretreatment, yoked (reported)",
            y.v_pre,
            "reported",
            true,
        ),
        row(
            "V after pretreatment, escapable − yoked (reported)",
            e.v_pre - y.v_pre,
            "reported",
            true,
        ),
        row(
            "test damage ticks per bout, yoked − escapable (reported)",
            y.latency - e.latency,
            "reported",
            true,
        ),
    ]
}

/// B3 over seeds 0..`seeds`.
pub fn checks(seeds: u64) -> Vec<Check> {
    let runs: Vec<[ArmStats; 3]> = (0..seeds).map(triad).collect();
    let arm = |i: usize| mean(&runs.iter().map(|r| r[i]).collect::<Vec<_>>());
    judge(arm(0), arm(1), arm(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(d_pre: f64, v_pre: f64, escaped: f64, latency: f64) -> ArmStats {
        ArmStats {
            d_pre,
            v_pre,
            escaped,
            latency,
        }
    }

    #[test]
    fn test_escape_counts_only_test_bouts() {
        // Pretreatment bout ignored; test bouts of 2, 10 and 4 ticks.
        let (share, lat) = test_escape(&[(30, 1), (600, 2), (630, 10), (660, 4)]);
        assert!((share - 2.0 / 3.0).abs() < 1e-12);
        assert!((lat - 16.0 / 3.0).abs() < 1e-12);
        assert_eq!(test_escape(&[(30, 1)]), (0.0, 0.0));
        assert_eq!(test_escape(&[(599, 1), (600, 10)]), (0.0, 10.0));
    }

    #[test]
    fn mean_averages_each_field() {
        let m = mean(&[stats(0.2, -0.4, 1.0, 2.0), stats(0.4, 0.0, 0.0, 6.0)]);
        assert_eq!(m, stats(0.30000000000000004, -0.2, 0.5, 4.0));
        assert_eq!(mean(&[]), ArmStats::default());
    }

    #[test]
    fn the_textbook_triad_passes() {
        // Yoked D 0.6, escapable 0.2, none 0.1: control explains 0.8 of the
        // gap; yoked escapes 0.2 of test bouts against 0.9 and 0.85.
        let rows = judge(
            stats(0.2, -0.1, 0.9, 3.0),
            stats(0.6, -0.5, 0.2, 9.0),
            stats(0.1, 0.0, 0.85, 3.5),
        );
        let m: Vec<f64> = rows.iter().map(|r| r.measured).collect();
        let want = [0.5, 0.8, 0.7, 0.05, 0.9, -0.5, 0.4, 6.0];
        for (got, w) in m.iter().zip(want) {
            assert!((got - w).abs() < 1e-9, "{m:?}");
        }
        assert!(rows.iter().all(|r| r.pass));
    }

    #[test]
    fn no_effect_fails_without_dividing_by_zero() {
        let a = stats(0.0, 0.0, 0.0, 10.0);
        let rows = judge(a, a, a);
        let pass: Vec<bool> = rows.iter().map(|r| r.pass).collect();
        assert_eq!(pass, [false, false, false, true, true, true, true, true]);
        assert_eq!(rows[1].measured, 0.0);
    }

    #[test]
    fn edges_of_each_bar() {
        // Effect exactly at the bar, share exactly half.
        let r = judge(
            stats(0.125, 0.0, 0.5, 0.0),
            stats(0.15, 0.0, 0.5, 0.0),
            stats(0.1, 0.0, 0.6, 0.0),
        );
        assert!((r[0].measured - 0.05).abs() < 1e-12);
        assert!(r[0].pass == (r[0].measured >= MIN_D_EFFECT));
        assert!((r[1].measured - 0.5).abs() < 1e-9);
        assert!(!r[2].pass, "equal escape is not less");
        // Less by exactly the bar is still noise; by more passes.
        let gap = |e: f64| {
            judge(
                stats(0.0, 0.0, e, 0.0),
                ArmStats::default(),
                ArmStats::default(),
            )[2]
            .pass
        };
        assert!(!gap(0.1));
        assert!(gap(0.11));
        // A large share with too small an effect still fails.
        let r = judge(
            stats(0.0, 0.0, 0.0, 0.0),
            stats(0.01, 0.0, 0.0, 0.0),
            stats(0.0, 0.0, 0.0, 0.0),
        );
        assert!((r[1].measured - 1.0).abs() < 1e-12);
        assert!(!r[1].pass);
        // Escapable 0.3 above none fails the "same" bar; 0.1 passes.
        let far = judge(
            stats(0.0, 0.0, 0.9, 0.0),
            ArmStats::default(),
            stats(0.0, 0.0, 0.6, 0.0),
        );
        assert!(!far[3].pass);
        assert!(far[2].pass);
    }

    #[test]
    fn pre_state_is_the_last_row_before_the_test() {
        let rows = [(10, 0.1, 0.2), (599, 0.3, -0.4), (600, 0.9, -0.9)];
        let (d, v) = pre_state(&rows);
        assert!((d - 0.3).abs() < 1e-6 && (v + 0.4).abs() < 1e-6);
        assert_eq!(pre_state(&rows[2..]), (0.0, 0.0));
        assert_eq!(pre_state(&[]), (0.0, 0.0));
    }

    #[test]
    fn pretreatment_keeps_bouts_before_the_test() {
        assert_eq!(
            pretreatment(&[(30, 10), (570, 3), (600, 10), (630, 1)]),
            [(30, 10), (570, 3)]
        );
    }

    #[test]
    fn the_registry_runs_b3() {
        assert_eq!(super::super::run(ID, 1).map(|r| r.len()), Some(8));
    }

    #[test]
    fn a_seed_runs_all_three_arms() {
        let [e, y, n] = triad(4);
        for a in [e, y, n] {
            assert!((0.0..=1.0).contains(&a.d_pre));
            assert!((-1.0..=1.0).contains(&a.v_pre));
            assert!((0.0..=1.0).contains(&a.escaped));
            assert!((0.0..=SHOCK_MAX_LEN as f64).contains(&a.latency));
        }
        // Seed 4's arms all meet test bouts, so latency is measured, and its
        // yoked arm replays the escapable arm's 19 pretreatment bouts.
        assert!(e.latency > 0.0 && y.latency > 0.0 && n.latency > 0.0);
        let (_, esc) = run_arm(4, None);
        let pre = pretreatment(&esc.shocks);
        assert_eq!(pre.len(), 19);
        let (_, yok) = run_arm(4, Some(pre.clone()));
        assert_eq!(pretreatment(&yok.shocks), pre);
        let (_, none) = run_arm(4, Some(Vec::new()));
        assert!(pretreatment(&none.shocks).is_empty());
        assert_eq!(checks(1).len(), 8);
    }
}

//! B1 Matching law (benchmarks spec v0.1): the Oracle (01 §4.1, §4.6;
//! spec §8) on a concurrent variable-interval task. Herrnstein (1961): choice
//! shares match reinforcement shares; Baum (1974): `log(B_i/B_j) =
//! s·log(R_i/R_j) + log b`, with s typically about 0.8.

use super::Check;
use cognitive_core::control::oracle::{CONTEXT_DIM, Context, Exp3, Oracle, WeightAdjust};
use cognitive_core::modes::Mode;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub const ID: &str = "matching";

/// Per-trial arming probability of each alternative (spec B1).
pub const ARMING: [f64; 4] = [0.08, 0.04, 0.02, 0.01];
pub const TRIALS: u32 = 20_000;
pub const BURN_IN: u32 = 5_000;
/// EXP3 step, as in the OQ18 sweep.
pub const EXP3_GAMMA: f32 = 0.05;
/// Pass band for the sensitivity s, and the minimum fit.
pub const S_BAND: (f64, f64) = (0.6, 1.1);
pub const MIN_R2: f64 = 0.9;

/// How the Oracle is trained and how a mode is chosen (spec B1 arms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// Gradient bandit; the chosen mode is drawn from w.
    BanditSampled,
    /// Gradient bandit; the chosen mode is the heaviest one, as the brain's
    /// argmax credit works today (spec §8).
    BanditArgmax,
    /// EXP3 (OQ18); the chosen mode is drawn from w.
    Exp3Sampled,
}

impl Arm {
    pub const ALL: [Arm; 3] = [Arm::BanditSampled, Arm::BanditArgmax, Arm::Exp3Sampled];

    pub fn label(self) -> &'static str {
        match self {
            Arm::BanditSampled => "bandit, sampled choice",
            Arm::BanditArgmax => "bandit, argmax choice",
            Arm::Exp3Sampled => "EXP3, sampled choice",
        }
    }

    fn oracle(self) -> Oracle {
        let mut o = Oracle::default();
        if self == Arm::Exp3Sampled {
            o.exp3 = Some(Exp3::new(EXP3_GAMMA));
        }
        o
    }
}

/// Choices and rewards per alternative after burn-in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tally {
    pub choices: [u32; 4],
    pub rewards: [u32; 4],
}

/// Fixed context: bias plus one task (spec B1).
fn context() -> Context {
    let mut x = [0.0; CONTEXT_DIM];
    x[0] = 1.0;
    x[1] = 1.0;
    x
}

fn pick(arm: Arm, w: &[f32; 4], u: f32) -> usize {
    if arm == Arm::BanditArgmax {
        return (0..4).fold(0, |b, i| if w[i] > w[b] { i } else { b });
    }
    let mut acc = 0.0;
    for (i, wi) in w.iter().enumerate() {
        acc += wi;
        if u < acc {
            return i;
        }
    }
    3
}

/// One seed of the concurrent VI task. A reward, once armed, waits until its
/// alternative is chosen (the VI rule; ratio schedules would test
/// maximizing, not matching).
pub fn run_seed(arm: Arm, seed: u64) -> Tally {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut oracle = arm.oracle();
    let x = context();
    let mut armed = [false; 4];
    let mut t = Tally::default();
    for trial in 0..TRIALS {
        for (a, q) in armed.iter_mut().zip(ARMING) {
            *a |= rng.random::<f64>() < q;
        }
        let w = oracle.weights(&x, WeightAdjust::default());
        let c = pick(arm, &w, rng.random());
        let r = if std::mem::take(&mut armed[c]) {
            1.0
        } else {
            0.0
        };
        oracle.update(&x, &w, Mode::ALL[c], r, &[0.0; 4]);
        if trial >= BURN_IN {
            t.choices[c] += 1;
            t.rewards[c] += u32::from(r > 0.0);
        }
    }
    t
}

/// Least-squares fit of `y = s·x + c` over every pair of alternatives with
/// non-zero counts: returns (s, c, r²). x = log reward ratio, y = log
/// choice ratio (Baum 1974).
pub fn fit(tallies: &[Tally]) -> Option<(f64, f64, f64)> {
    let mut pts = Vec::new();
    for t in tallies {
        for i in 0..4 {
            for j in (i + 1)..4 {
                let (bi, bj, ri, rj) = (t.choices[i], t.choices[j], t.rewards[i], t.rewards[j]);
                if bi > 0 && bj > 0 && ri > 0 && rj > 0 {
                    let x = (f64::from(ri) / f64::from(rj)).ln();
                    let y = (f64::from(bi) / f64::from(bj)).ln();
                    pts.push((x, y));
                }
            }
        }
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    // Σ(x − x̄)·y equals Σ(x − x̄)(y − ȳ), since Σ(x − x̄) = 0.
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * p.1).sum();
    let syy: f64 = pts.iter().map(|p| (p.1 - my).powi(2)).sum();
    // Fewer than two distinct x values (including no points) has no slope.
    if sxx <= 0.0 {
        return None;
    }
    let s = sxy / sxx;
    // Flat y explains nothing: r² = 0, not 1.
    let r2 = if syy > 0.0 {
        sxy * sxy / (sxx * syy)
    } else {
        0.0
    };
    Some((s, my - s * mx, r2))
}

/// The B1 signatures for every arm.
pub fn checks(seeds: u64) -> Vec<Check> {
    Arm::ALL
        .into_iter()
        .flat_map(|arm| {
            let tallies: Vec<Tally> = (0..seeds).map(|s| run_seed(arm, s)).collect();
            arm_checks(arm, &tallies)
        })
        .collect()
}

/// Sensitivity, fit and the richest alternative's share for one arm.
pub fn arm_checks(arm: Arm, tallies: &[Tally]) -> Vec<Check> {
    let (s, r2) = fit(tallies).map_or((0.0, 0.0), |(s, _, r2)| (s, r2));
    let total: u32 = tallies.iter().flat_map(|t| t.choices).sum();
    let top: u32 = tallies.iter().map(|t| t.choices[0]).sum();
    let row = |name: &str, measured: f64, target: String, pass: bool| Check {
        bench: ID,
        name: format!("{}: {name}", arm.label()),
        measured,
        target,
        pass,
    };
    vec![
        row(
            "sensitivity s",
            s,
            format!("{}–{} (Baum 1974: about 0.8)", S_BAND.0, S_BAND.1),
            (S_BAND.0..=S_BAND.1).contains(&s),
        ),
        row("fit r²", r2, format!("≥ {MIN_R2}"), r2 >= MIN_R2),
        row(
            "choice share of the richest alternative",
            f64::from(top) / f64::from(total.max(1)),
            "reported (matching ≈ 0.53)".into(),
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(choices: [u32; 4], rewards: [u32; 4]) -> Tally {
        Tally { choices, rewards }
    }

    #[test]
    fn strict_matching_fits_slope_one() {
        // Choices exactly proportional to rewards: s = 1, bias 0, r² = 1.
        let t = tally([800, 400, 200, 100], [80, 40, 20, 10]);
        let (s, c, r2) = fit(&[t]).unwrap();
        assert!((s - 1.0).abs() < 1e-9 && c.abs() < 1e-9 && (r2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn undermatching_fits_slope_below_one() {
        // Choice ratios are the square root of reward ratios: s = 0.5.
        let t = tally([800, 400, 200, 100], [64, 16, 4, 1]);
        let (s, _, _) = fit(&[t]).unwrap();
        assert!((s - 0.5).abs() < 1e-9, "{s}");
    }

    #[test]
    fn fit_needs_spread() {
        assert!(fit(&[]).is_none());
        assert!(fit(&[tally([10, 0, 0, 0], [1, 0, 0, 0])]).is_none());
        assert!(
            fit(&[tally([10, 10, 0, 0], [2, 1, 0, 0])]).is_none(),
            "one point"
        );
        assert!(
            fit(&[tally([10, 10, 10, 10], [1, 1, 1, 1])]).is_none(),
            "no spread"
        );
    }

    #[test]
    fn two_points_are_enough() {
        let a = tally([40, 20, 0, 0], [4, 2, 0, 0]);
        let b = tally([90, 10, 0, 0], [9, 1, 0, 0]);
        let (s, _, _) = fit(&[a, b]).unwrap();
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_zero_in_any_count_drops_that_pair() {
        // Alternatives 0..2 match strictly; one zero count elsewhere must
        // leave the fit unchanged.
        let base = tally([800, 300, 100, 0], [80, 30, 10, 0]);
        let (s0, c0, r0) = fit(&[base]).unwrap();
        for (b3, r3) in [(0, 5), (50, 0)] {
            let t = tally([800, 300, 100, b3], [80, 30, 10, r3]);
            let (s, c, r) = fit(&[t]).unwrap();
            assert!((s - s0).abs() < 1e-9 && (c - c0).abs() < 1e-9 && (r - r0).abs() < 1e-9);
        }
        for (b0, r0_) in [(0, 5), (50, 0)] {
            let t = tally([b0, 800, 300, 100], [r0_, 80, 30, 10]);
            assert!((fit(&[t]).unwrap().0 - s0).abs() < 1e-9);
        }
    }

    #[test]
    fn noisy_points_fit_hand_worked_values() {
        // x = ln 2, y = ln 4 and x = ln 4, y = ln 4: y is flat, so s = 0,
        // intercept = 2 ln 2 and r² = 0.
        let l2 = 2f64.ln();
        let a = tally([80, 20, 0, 0], [20, 10, 0, 0]);
        let b = tally([80, 20, 0, 0], [40, 10, 0, 0]);
        let (s, c, r2) = fit(&[a, b]).unwrap();
        assert!(
            s.abs() < 1e-9 && (c - 2.0 * l2).abs() < 1e-9 && r2.abs() < 1e-9,
            "{s} {c} {r2}"
        );
        // In units of ln 2: x = (1, 2, 3), y = (2, 2, 4). x̄ = 2, ȳ = 8/3,
        // sxx = 2, sxy = 2, syy = 8/3, so s = 1, c = 2/3 ln 2, r² = 0.75.
        let t1 = tally([40, 10, 0, 0], [20, 10, 0, 0]);
        let t2 = tally([40, 10, 0, 0], [40, 10, 0, 0]);
        let t3 = tally([160, 10, 0, 0], [80, 10, 0, 0]);
        let (s, c, r2) = fit(&[t1, t2, t3]).unwrap();
        assert!((s - 1.0).abs() < 1e-9, "{s}");
        assert!((c - (8.0 / 3.0 - 2.0) * l2).abs() < 1e-9, "{c}");
        assert!((r2 - 0.75).abs() < 1e-9, "{r2}");
    }

    #[test]
    fn arm_rows_judge_s_and_fit() {
        let strict = tally([800, 400, 200, 100], [80, 40, 20, 10]);
        let rows = arm_checks(Arm::BanditSampled, &[strict]);
        assert_eq!(rows[0].name, "bandit, sampled choice: sensitivity s");
        assert!(rows.iter().all(|r| r.pass && r.bench == ID));
        assert!((rows[2].measured - 800.0 / 1500.0).abs() < 1e-9);
        let under = tally([800, 400, 200, 100], [64, 16, 4, 1]);
        assert!(!arm_checks(Arm::Exp3Sampled, &[under])[0].pass, "s = 0.5");
        let collapse = arm_checks(Arm::BanditArgmax, &[tally([100, 0, 0, 0], [10, 0, 0, 0])]);
        assert!(!collapse[0].pass && !collapse[1].pass);
        assert_eq!(collapse[0].measured, 0.0);
        assert_eq!(collapse[2].measured, 1.0);
        let empty = arm_checks(Arm::BanditSampled, &[Tally::default()]);
        assert_eq!(empty[2].measured, 0.0);
        // r² of 0.75 fails the 0.9 bar even with s = 1.
        let t1 = tally([40, 10, 0, 0], [20, 10, 0, 0]);
        let t2 = tally([40, 10, 0, 0], [40, 10, 0, 0]);
        let t3 = tally([160, 10, 0, 0], [80, 10, 0, 0]);
        let noisy = arm_checks(Arm::BanditSampled, &[t1, t2, t3]);
        assert!(noisy[0].pass && !noisy[1].pass);
    }

    #[test]
    fn labels_and_oracles_differ_by_arm() {
        let labels: Vec<_> = Arm::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(
            labels,
            [
                "bandit, sampled choice",
                "bandit, argmax choice",
                "EXP3, sampled choice"
            ]
        );
        assert!(Arm::Exp3Sampled.oracle().exp3.is_some());
        assert!(Arm::BanditSampled.oracle().exp3.is_none());
        assert!(Arm::BanditArgmax.oracle().exp3.is_none());
        let x = context();
        assert_eq!((x[0], x[1]), (1.0, 1.0));
        assert_eq!(x.iter().sum::<f32>(), 2.0);
    }

    #[test]
    fn argmax_breaks_ties_low_and_sampling_edges_go_up() {
        assert_eq!(pick(Arm::BanditArgmax, &[0.3, 0.3, 0.2, 0.2], 0.0), 0);
        assert_eq!(pick(Arm::BanditArgmax, &[0.2, 0.3, 0.3, 0.2], 0.0), 1);
        assert_eq!(pick(Arm::BanditSampled, &[0.25, 0.25, 0.25, 0.25], 0.25), 1);
    }

    #[test]
    fn rewards_arrive_at_the_arming_rate() {
        // Arming rates sum to 0.15 per trial, so rewards are at most that
        // share of trials, and close to it when every alternative is visited.
        let t = run_seed(Arm::BanditSampled, 1);
        let choices: u32 = t.choices.iter().sum();
        let rewards: u32 = t.rewards.iter().sum();
        let rate = f64::from(rewards) / f64::from(choices);
        assert!((0.10..=0.16).contains(&rate), "{rate}");
        assert!(t.rewards.iter().all(|&r| r > 0));
    }

    #[test]
    fn fit_is_invariant_under_relabelling() {
        let a = tally([800, 300, 150, 90], [70, 30, 12, 9]);
        let b = tally([90, 150, 300, 800], [9, 12, 30, 70]);
        let (sa, _, ra) = fit(&[a]).unwrap();
        let (sb, _, rb) = fit(&[b]).unwrap();
        assert!((sa - sb).abs() < 1e-9 && (ra - rb).abs() < 1e-9);
    }

    #[test]
    fn argmax_pick_takes_the_heaviest_and_sampling_follows_w() {
        let w = [0.1, 0.6, 0.2, 0.1];
        assert_eq!(pick(Arm::BanditArgmax, &w, 0.0), 1);
        assert_eq!(pick(Arm::BanditSampled, &w, 0.05), 0);
        assert_eq!(pick(Arm::BanditSampled, &w, 0.65), 1);
        assert_eq!(pick(Arm::BanditSampled, &w, 0.85), 2);
        assert_eq!(pick(Arm::Exp3Sampled, &w, 0.999), 3);
    }

    #[test]
    fn runs_are_seeded_and_count_only_after_burn_in() {
        let a = run_seed(Arm::BanditSampled, 3);
        assert_eq!(a, run_seed(Arm::BanditSampled, 3));
        let total: u32 = a.choices.iter().sum();
        assert_eq!(total, TRIALS - BURN_IN);
        for i in 0..4 {
            assert!(a.rewards[i] <= a.choices[i]);
        }
    }

    #[test]
    fn checks_report_three_rows_per_arm() {
        let rows = checks(1);
        assert_eq!(rows.len(), 9);
        assert!(rows.iter().all(|r| r.bench == ID));
        // The sampled bandit matches; argmax choice collapses (spec B1).
        assert!(rows[0].pass && rows[1].pass);
        assert!(!rows[3].pass && (rows[5].measured - 1.0).abs() < 1e-9);
    }
}

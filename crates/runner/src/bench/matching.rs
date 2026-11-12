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
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let syy: f64 = pts.iter().map(|p| (p.1 - my).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let s = sxy / sxx;
    let r2 = if syy > 0.0 {
        sxy * sxy / (sxx * syy)
    } else {
        1.0
    };
    Some((s, my - s * mx, r2))
}

/// The B1 signatures for every arm.
pub fn checks(seeds: u64) -> Vec<Check> {
    let mut out = Vec::new();
    for arm in Arm::ALL {
        let tallies: Vec<Tally> = (0..seeds).map(|s| run_seed(arm, s)).collect();
        let (s, _, r2) = fit(&tallies).unwrap_or((0.0, 0.0, 0.0));
        out.push(Check {
            bench: ID,
            name: format!("{}: sensitivity s", arm.label()),
            measured: s,
            target: format!("{}–{} (Baum 1974: about 0.8)", S_BAND.0, S_BAND.1),
            pass: (S_BAND.0..=S_BAND.1).contains(&s),
        });
        out.push(Check {
            bench: ID,
            name: format!("{}: fit r²", arm.label()),
            measured: r2,
            target: format!("≥ {MIN_R2}"),
            pass: r2 >= MIN_R2,
        });
        let total: u32 = tallies.iter().flat_map(|t| t.choices).sum();
        let top: u32 = tallies.iter().map(|t| t.choices[0]).sum();
        out.push(Check {
            bench: ID,
            name: format!("{}: choice share of the richest alternative", arm.label()),
            measured: f64::from(top) / f64::from(total.max(1)),
            target: "reported (matching ≈ 0.53)".into(),
            pass: true,
        });
    }
    out
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
    fn fit_skips_empty_pairs_and_needs_two_points() {
        assert!(fit(&[tally([10, 0, 0, 0], [1, 0, 0, 0])]).is_none());
        assert!(fit(&[tally([10, 10, 0, 0], [2, 1, 0, 0])]).is_none());
        assert!(
            fit(&[tally([10, 10, 10, 10], [1, 1, 1, 1])]).is_none(),
            "no spread"
        );
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
    }
}

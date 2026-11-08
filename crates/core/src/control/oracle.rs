//! Oracle (allocator), 01 §4.1 and spec §8: a contextual gradient bandit
//! (Sutton & Barto 2018 §2.8). OQ18 alternatives plug in behind the same API.

use crate::modes::{Mode, f_sign};
use serde::{Deserialize, Serialize};

/// Context x(S) = [1, onehot(task)×7, competence_t, retrieval_hit, stakes,
/// threat, energy, novelty, U, fatigue] (spec §8).
pub const CONTEXT_DIM: usize = 16;
pub type Context = [f32; CONTEXT_DIM];

/// Adjustments applied after the learned preferences (spec §8, §11, §15).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeightAdjust {
    /// `β_U·U`, added to Exploring (01 §4.9).
    pub urgency_bias: f32,
    /// `δ·max(0, π − π_choke)`, moved from P to O when P leads (01 §4.5).
    pub choke_shift: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Oracle {
    /// Θ: one row of context weights per mode (L, E, P, O).
    pub theta: [Context; 4],
    /// Minimum weight per mode (01 §4.1, proposed 0.02).
    pub floor: f32,
    /// Step size η.
    pub eta: f32,
    /// Running mean R̄ used as the bandit baseline.
    pub r_bar: f32,
    /// EMA rate for R̄.
    pub r_bar_rate: f32,
}

impl Default for Oracle {
    fn default() -> Self {
        Self {
            theta: [[0.0; CONTEXT_DIM]; 4],
            floor: 0.02,
            eta: 0.05,
            r_bar: 0.0,
            r_bar_rate: 0.01,
        }
    }
}

impl Oracle {
    /// `[w_L, w_E, w_P, w_O]`, Σw = 1 and each w ≥ floor (01 §4.1).
    pub fn weights(&self, x: &Context, adj: WeightAdjust) -> [f32; 4] {
        let pref: [f32; 4] = std::array::from_fn(|i| dot(&self.theta[i], x));
        let mut v = softmax(&pref);
        v[Mode::Exploring.index()] += adj.urgency_bias.max(0.0);
        let p = Mode::Performing.index();
        let o = Mode::Optimizing.index();
        let p_leads = (0..4).all(|i| v[p] >= v[i]);
        if p_leads && adj.choke_shift > 0.0 {
            let moved = adj.choke_shift.min(v[p]);
            v[p] -= moved;
            v[o] += moved;
        }
        with_floor(v, self.floor)
    }

    /// Credited mode `i* = argmax_i w_i·m_i·|m_i|` for the chosen action (spec §8).
    pub fn credited(w: &[f32; 4], m: &[f32; 4]) -> Mode {
        let mut best = 0;
        for i in 1..4 {
            if w[i] * f_sign(m[i]) > w[best] * f_sign(m[best]) {
                best = i;
            }
        }
        Mode::ALL[best]
    }

    /// `Θ_i += η·(R − R̄)·(1[i = i*] − w_i)·x`. When `R − R̄ < 0`, mode i's
    /// step is scaled by `(1 − T[i, task])`, so high-tenacity modes lose
    /// weight more slowly after setbacks (01 §4.4).
    pub fn update(
        &mut self,
        x: &Context,
        w: &[f32; 4],
        credited: Mode,
        r: f32,
        tenacity: &[f32; 4],
    ) {
        let adv = r - self.r_bar;
        for (i, row) in self.theta.iter_mut().enumerate() {
            let ind = if i == credited.index() { 1.0 } else { 0.0 };
            let mut step = self.eta * adv * (ind - w[i]);
            if adv < 0.0 {
                step *= 1.0 - tenacity[i].clamp(0.0, 1.0);
            }
            for (t, xi) in row.iter_mut().zip(x) {
                *t += step * xi;
            }
        }
        self.r_bar += self.r_bar_rate * (r - self.r_bar);
    }
}

fn dot(a: &Context, b: &Context) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn softmax(z: &[f32; 4]) -> [f32; 4] {
    let max = z.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e = z.map(|v| (v - max).exp());
    let s: f32 = e.iter().sum();
    e.map(|v| v / s)
}

/// Map non-negative raw weights to `floor + (1 − 4·floor)·v/Σv`, which gives
/// Σw = 1 and w ≥ floor exactly (spec §8). Clipping after renormalising would
/// break one of the two.
pub fn with_floor(v: [f32; 4], floor: f32) -> [f32; 4] {
    let v = v.map(|x| x.max(0.0));
    let s: f32 = v.iter().sum();
    let share = 1.0 - 4.0 * floor;
    if s <= 0.0 {
        return [0.25; 4];
    }
    v.map(|x| floor + share * x / s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ctx(vals: [f32; CONTEXT_DIM]) -> Context {
        vals
    }

    proptest! {
        /// Spec §27 gate: Σw = 1 with w ≥ floor, whatever Θ, context, urgency
        /// or choking do.
        #[test]
        fn weights_sum_to_one_above_floor(
            theta in proptest::array::uniform4(proptest::array::uniform16(-10.0f32..10.0)),
            x in proptest::array::uniform16(-1.0f32..1.0),
            ub in 0.0f32..1.0,
            ch in 0.0f32..1.0,
        ) {
            let o = Oracle { theta, ..Oracle::default() };
            let w = o.weights(&ctx(x), WeightAdjust { urgency_bias: ub, choke_shift: ch });
            let s: f32 = w.iter().sum();
            prop_assert!((s - 1.0).abs() < 1e-5, "sum {s}");
            for wi in w {
                prop_assert!(wi >= 0.02 - 1e-6, "w {wi}");
            }
        }
    }

    #[test]
    fn credited_mode_rewarded_gains_weight() {
        let mut o = Oracle::default();
        let mut x = [0.0; CONTEXT_DIM];
        x[0] = 1.0;
        let w0 = o.weights(&x, WeightAdjust::default());
        for _ in 0..200 {
            let w = o.weights(&x, WeightAdjust::default());
            o.update(&x, &w, Mode::Performing, 1.0, &[0.5; 4]);
            o.update(&x, &w, Mode::Learning, -1.0, &[0.5; 4]);
        }
        let w1 = o.weights(&x, WeightAdjust::default());
        assert!(w1[Mode::Performing.index()] > w0[Mode::Performing.index()]);
    }

    #[test]
    fn high_tenacity_loses_weight_more_slowly() {
        let mut x = [0.0; CONTEXT_DIM];
        x[0] = 1.0;
        let run = |t: f32| {
            let mut o = Oracle::default();
            for _ in 0..50 {
                let w = o.weights(&x, WeightAdjust::default());
                // Repeated setbacks while Performing is credited.
                o.update(&x, &w, Mode::Performing, -1.0, &[t; 4]);
            }
            o.weights(&x, WeightAdjust::default())[Mode::Performing.index()]
        };
        assert!(run(0.9) > run(0.1));
    }

    #[test]
    fn choking_moves_weight_from_p_to_o() {
        let mut o = Oracle::default();
        o.theta[Mode::Performing.index()][0] = 3.0; // P leads
        let mut x = [0.0; CONTEXT_DIM];
        x[0] = 1.0;
        let calm = o.weights(&x, WeightAdjust::default());
        let choke = o.weights(
            &x,
            WeightAdjust {
                urgency_bias: 0.0,
                choke_shift: 0.2,
            },
        );
        assert!(choke[Mode::Performing.index()] < calm[Mode::Performing.index()]);
        assert!(choke[Mode::Optimizing.index()] > calm[Mode::Optimizing.index()]);
    }

    #[test]
    fn credit_goes_to_mode_that_drove_the_choice() {
        let w = [0.1, 0.2, 0.6, 0.1];
        assert_eq!(
            Oracle::credited(&w, &[0.9, 0.3, 0.5, -0.4]),
            Mode::Performing
        );
    }
}

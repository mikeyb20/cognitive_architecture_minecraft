//! Modes as learnable lenses (spec §7; decision #39).
//!
//! `m_i(a) = tanh(θ_i · φ(a,S))`, learning
//! `θ_i ← θ_i + lr·(success_i − m_i)·(1 − m_i²)·φ`. All four lenses learn from
//! every executed action, from the true (unflipped) score (00 §7.7; 01 §4.3).

use super::Mode;
use serde::{Deserialize, Serialize};

/// Number of per-candidate features φ (spec §3: f0..f12).
pub const N_FEATURES: usize = 13;

/// Per-candidate features φ(a, S) (spec §3). Every mode reads the same φ
/// (01 §2.1: one shared S).
pub type Features = [f32; N_FEATURES];

/// Feature indices (spec §3).
pub mod f {
    pub const BIAS: usize = 0;
    pub const FRESH: usize = 1;
    pub const UNKNOWN: usize = 2;
    pub const UNCERTAINTY: usize = 3;
    pub const COMPETENCE: usize = 4;
    pub const RELEVANCE: usize = 5;
    pub const SAVING: usize = 6;
    pub const RISK: usize = 7;
    pub const VALENCE_TAG: usize = 8;
    pub const INTERNAL: usize = 9;
    pub const TM_LINK: usize = 10;
    pub const RECENT_FAIL: usize = 11;
    pub const COST: usize = 12;
}

/// A mode's processing style: swappable per mode (00 §7.4).
pub trait ModeNetwork {
    /// m_i ∈ [−1, 1] (01 §3.2).
    fn score(&self, phi: &Features) -> f32;
    /// Learn from the true outcome. `m_true` must be the unflipped score.
    fn learn(&mut self, phi: &Features, m_true: f32, success: f32, lr: f32);
    /// Randomness (01 §3.1).
    fn temperature(&self) -> f32;
}

/// Learning rate and randomness per mode (01 §3.1; spec §7, proposed).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LensParams {
    pub base_lr: f32,
    pub temperature: f32,
}

impl LensParams {
    pub fn default_for(mode: Mode) -> Self {
        let (base_lr, temperature) = match mode {
            Mode::Learning => (0.05, 0.5),
            Mode::Exploring => (0.02, 1.0),
            Mode::Performing => (0.005, 0.1),
            Mode::Optimizing => (0.01, 0.2),
        };
        Self {
            base_lr,
            temperature,
        }
    }
}

/// v1 lens: a linear layer and tanh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lens {
    pub mode: Mode,
    pub theta: Features,
    pub params: LensParams,
    /// `frozen` sets the learning rate to 0, for "train one sub-network while
    /// the others stay frozen" (00 §7.4; spec §29 T-IND) and the frozen-lens
    /// ablation.
    pub frozen: bool,
}

impl Lens {
    /// Starting weights = the #35 code rules (spec §7 priors).
    pub fn with_priors(mode: Mode) -> Self {
        let mut theta = [0.0; N_FEATURES];
        let set = |t: &mut Features, pairs: &[(usize, f32)]| {
            for &(i, w) in pairs {
                t[i] = w;
            }
        };
        match mode {
            Mode::Exploring => set(
                &mut theta,
                &[
                    (f::BIAS, -0.6),
                    (f::FRESH, 2.0),
                    (f::UNCERTAINTY, 0.3),
                    (f::RISK, -1.5),
                    (f::VALENCE_TAG, 0.5),
                ],
            ),
            Mode::Learning => set(
                &mut theta,
                &[
                    (f::BIAS, -0.5),
                    (f::UNKNOWN, 1.5),
                    (f::UNCERTAINTY, 1.0),
                    (f::COMPETENCE, -0.5),
                ],
            ),
            Mode::Performing => set(
                &mut theta,
                &[
                    (f::COMPETENCE, 1.5),
                    (f::RELEVANCE, 1.0),
                    (f::RISK, -0.5),
                    (f::VALENCE_TAG, 0.3),
                ],
            ),
            Mode::Optimizing => set(
                &mut theta,
                &[
                    (f::BIAS, -0.3),
                    (f::SAVING, 2.0),
                    (f::RECENT_FAIL, 0.8),
                    (f::RISK, -2.0),
                    (f::VALENCE_TAG, 0.8),
                ],
            ),
        }
        Self {
            mode,
            theta,
            params: LensParams::default_for(mode),
            frozen: false,
        }
    }
}

impl ModeNetwork for Lens {
    fn score(&self, phi: &Features) -> f32 {
        let z: f32 = self.theta.iter().zip(phi).map(|(t, x)| t * x).sum();
        z.tanh()
    }

    fn learn(&mut self, phi: &Features, m_true: f32, success: f32, lr: f32) {
        if self.frozen {
            return;
        }
        let m = m_true.clamp(-1.0, 1.0);
        let step = lr * (success.clamp(-1.0, 1.0) - m) * (1.0 - m * m);
        for (t, x) in self.theta.iter_mut().zip(phi) {
            *t += step * x;
        }
    }

    fn temperature(&self) -> f32 {
        self.params.temperature
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn phi(fresh: f32, risk: f32) -> Features {
        let mut p = [0.0; N_FEATURES];
        p[f::BIAS] = 1.0;
        p[f::FRESH] = fresh;
        p[f::RISK] = risk;
        p
    }

    #[test]
    fn priors_have_the_intended_character() {
        let e = Lens::with_priors(Mode::Exploring);
        // Exploring likes the new and vetoes the dangerous (spec §7 table).
        assert!(e.score(&phi(1.0, 0.0)) > 0.5);
        assert!(e.score(&phi(0.0, 0.8)) < -0.5);
        let o = Lens::with_priors(Mode::Optimizing);
        assert!(o.score(&phi(0.0, 0.8)) < -0.8);
    }

    #[test]
    fn learning_moves_score_toward_success() {
        let mut l = Lens::with_priors(Mode::Exploring);
        let x = phi(0.3, 0.2);
        let before = l.score(&x);
        for _ in 0..200 {
            let m = l.score(&x);
            l.learn(&x, m, 1.0, 0.05);
        }
        assert!(l.score(&x) > before);
        assert!(l.score(&x) > 0.8);
    }

    #[test]
    fn frozen_lens_does_not_learn() {
        let mut l = Lens::with_priors(Mode::Performing);
        l.frozen = true;
        let before = l.theta;
        l.learn(&phi(1.0, 1.0), 0.0, 1.0, 0.5);
        assert_eq!(l.theta, before);
    }

    /// Spec §27 gate: lenses learn from the unflipped m (01 §4.3). Learning with
    /// the true m from a kicked tick must equal learning on an un-kicked tick.
    #[test]
    fn kick_does_not_change_what_is_learned() {
        let x = phi(0.6, 0.1);
        let mut a = Lens::with_priors(Mode::Exploring);
        let mut b = a.clone();
        let m_true = a.score(&x);
        let _m_kicked = -m_true; // used for aggregation only, never for learning
        a.learn(&x, m_true, 0.5, 0.05);
        b.learn(&x, b.score(&x), 0.5, 0.05);
        assert_eq!(a.theta, b.theta);
    }

    proptest! {
        #[test]
        fn score_in_range(xs in proptest::array::uniform13(-1.0f32..=1.0)) {
            for mode in Mode::ALL {
                let s = Lens::with_priors(mode).score(&xs);
                prop_assert!((-1.0..=1.0).contains(&s));
            }
        }
    }
}

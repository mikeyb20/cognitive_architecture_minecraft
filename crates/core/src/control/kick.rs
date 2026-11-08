//! Contrarian kick, 01 §4.3 and spec §9. Applied by the Oracle to mode scores
//! before aggregation, for one tick (00 §3.5.4).

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Personality parameters θ, p, N, κ (01 §4.3; spec §19 defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KickParams {
    /// Extreme-score threshold θ.
    pub theta: f32,
    /// Base fire probability (scheduled by ρ, spec §14).
    pub p_base: f32,
    /// Stuck window N in ticks.
    pub n: usize,
    /// Stuck-trigger C boost κ (dampened by D at the call site, decision #6).
    pub kappa: f32,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            theta: 0.9,
            p_base: 0.05,
            n: 300,
            kappa: 0.15,
        }
    }
}

/// Trigger 1: each extreme score (|m| > θ) is flipped with probability p.
/// Returns how many scores flipped. Flipping is `m ← −m`, so scores stay in
/// [−1, 1] (01 §4.3). The caller keeps the unflipped scores for learning.
pub fn kick_extremes(m: &mut [f32], theta: f32, p: f32, rng: &mut impl Rng) -> usize {
    let mut fired = 0;
    for mi in m.iter_mut() {
        if mi.abs() > theta && rng.random::<f32>() < p {
            *mi = -*mi;
            fired += 1;
        }
    }
    fired
}

/// Trigger 2: "stuck" means that over the last N ticks the L1 change in w was
/// < 0.05, the change in C was < 0.05, and action-type entropy was < 0.5 bits
/// (spec §9).
#[derive(Debug, Clone, Default)]
pub struct StuckDetector {
    window: usize,
    history: VecDeque<([f32; 4], f32, u32)>,
}

impl StuckDetector {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            history: VecDeque::with_capacity(window + 1),
        }
    }

    /// Record one tick: Oracle weights, C, and an id for the chosen action type.
    pub fn record(&mut self, w: [f32; 4], c: f32, action_type: u32) {
        self.history.push_back((w, c, action_type));
        while self.history.len() > self.window {
            self.history.pop_front();
        }
    }

    pub fn is_stuck(&self) -> bool {
        if self.window == 0 || self.history.len() < self.window {
            return false;
        }
        let (w0, c0, _) = self.history[0];
        let mut max_w = 0.0f32;
        let mut max_c = 0.0f32;
        for (w, c, _) in &self.history {
            let l1: f32 = w.iter().zip(&w0).map(|(a, b)| (a - b).abs()).sum();
            max_w = max_w.max(l1);
            max_c = max_c.max((c - c0).abs());
        }
        max_w < 0.05 && max_c < 0.05 && self.action_entropy_bits() < 0.5
    }

    fn action_entropy_bits(&self) -> f32 {
        let mut counts: Vec<(u32, u32)> = Vec::new();
        for (_, _, a) in &self.history {
            match counts.iter_mut().find(|(k, _)| k == a) {
                Some((_, n)) => *n += 1,
                None => counts.push((*a, 1)),
            }
        }
        let total = self.history.len() as f32;
        counts
            .iter()
            .map(|(_, n)| {
                let p = *n as f32 / total;
                -p * p.log2()
            })
            .sum()
    }

    /// After a stuck kick fires, start a fresh window.
    pub fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    proptest! {
        /// Spec §27 gate: the kick keeps m ∈ [−1, 1].
        #[test]
        fn kick_keeps_scores_in_range(
            m in proptest::collection::vec(-1.0f32..=1.0, 1..40),
            seed in any::<u64>(),
        ) {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut k = m.clone();
            kick_extremes(&mut k, 0.5, 0.9, &mut rng);
            for (a, b) in k.iter().zip(&m) {
                prop_assert!((-1.0..=1.0).contains(a));
                prop_assert!(*a == *b || *a == -*b);
            }
        }
    }

    /// Spec §27 gate: the kick fires at rate p on extreme scores, and never on
    /// scores within θ.
    #[test]
    fn fires_at_rate_p() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let n = 100_000;
        let mut m = vec![0.95f32; n];
        let fired = kick_extremes(&mut m, 0.9, 0.05, &mut rng);
        let rate = fired as f32 / n as f32;
        // 5σ band for a binomial with p = 0.05.
        let sd = (0.05f32 * 0.95 / n as f32).sqrt();
        assert!((rate - 0.05).abs() < 5.0 * sd, "rate {rate}");
        let mut calm = vec![0.5f32; 1000];
        assert_eq!(kick_extremes(&mut calm, 0.9, 1.0, &mut rng), 0);
    }

    #[test]
    fn stuck_detection() {
        let mut s = StuckDetector::new(10);
        for _ in 0..10 {
            s.record([0.25; 4], 0.3, 1);
        }
        assert!(s.is_stuck());
        s.record([0.25; 4], 0.3, 2);
        s.record([0.25; 4], 0.3, 3);
        s.record([0.25; 4], 0.3, 4);
        assert!(!s.is_stuck(), "varied actions are not stuck");
        let mut moving = StuckDetector::new(10);
        for i in 0..10 {
            moving.record([0.25; 4], 0.3 + 0.01 * i as f32, 1);
        }
        assert!(!moving.is_stuck(), "C moved by 0.09");
    }

    #[test]
    fn stuck_needs_a_full_window_and_resets() {
        let mut s = StuckDetector::new(5);
        for _ in 0..4 {
            s.record([0.25; 4], 0.3, 1);
        }
        assert!(!s.is_stuck(), "window not full yet");
        s.record([0.25; 4], 0.3, 1);
        assert!(s.is_stuck());
        s.reset();
        assert!(!s.is_stuck());
        assert!(
            !StuckDetector::new(0).is_stuck(),
            "N = 0 disables the trigger"
        );
    }

    #[test]
    fn weights_moving_is_not_stuck() {
        let mut s = StuckDetector::new(5);
        for i in 0..5 {
            let x = 0.25 + 0.02 * i as f32;
            s.record([x, 0.25, 0.25, 0.5 - x], 0.3, 1);
        }
        assert!(!s.is_stuck());
    }
}

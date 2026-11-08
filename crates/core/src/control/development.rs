//! Development level ρ and its continuous schedules (01 §4.7; spec §14).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Development {
    useful: u64,
    total: u64,
    /// Pretraining offset ρ_pre (0.5 when an LLM tier is attached; spec §14).
    pub rho_pre: f32,
}

/// Stage labels for logs only (spec §14). No jumps between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Initialization,
    RapidDevelopment,
    Maturation,
}

impl Development {
    pub fn new(rho_pre: f32) -> Self {
        Self {
            useful: 0,
            total: 0,
            rho_pre,
        }
    }

    /// One experience = one executed action (spec §14).
    pub fn record_experience(&mut self) {
        self.total += 1;
    }

    /// A memory became useful (retrieved within 20 ticks before a tick with R > 0).
    pub fn record_useful(&mut self) {
        self.useful = (self.useful + 1).min(self.total);
    }

    /// ρ = useful retained ÷ total experiences ∈ [0, 1] (01 §4.7).
    pub fn rho(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.useful as f32 / self.total as f32
        }
    }

    /// `ρ_eff = max(ρ, ρ_pre)`.
    pub fn rho_eff(&self) -> f32 {
        self.rho().max(self.rho_pre)
    }

    pub fn stage(&self) -> Stage {
        match self.rho_eff() {
            r if r < 0.2 => Stage::Initialization,
            r if r < 0.6 => Stage::RapidDevelopment,
            _ => Stage::Maturation,
        }
    }

    /// Learning-rate multiplier `s_lr = 1 + (1 − ρ_eff)`.
    pub fn lr_multiplier(&self) -> f32 {
        1.0 + (1.0 - self.rho_eff())
    }

    /// Kick rate `p = p_base·(1 + 3·(1 − ρ_eff))`: a "baby" agent kicks far more.
    pub fn kick_rate(&self, p_base: f32) -> f32 {
        (p_base * (1.0 + 3.0 * (1.0 - self.rho_eff()))).clamp(0.0, 1.0)
    }

    /// Success threshold `thr = thr0 + (thr1 − thr0)·ρ_eff` (success evolves).
    pub fn threshold(&self, thr0: f32, thr1: f32) -> f32 {
        thr0 + (thr1 - thr0) * self.rho_eff()
    }

    /// Subconscious budget `B = B_base·(1 + (1 − ρ_eff))·(1 + 2·[resting])`.
    pub fn subconscious_budget(&self, b_base: f32, resting: bool) -> f32 {
        b_base * (1.0 + (1.0 - self.rho_eff())) * if resting { 3.0 } else { 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_are_monotonic_in_rho() {
        // Schedules are smooth functions of ρ_eff (01 §4.7: no stage jumps).
        let mut prev: Option<Development> = None;
        for i in 0..=100 {
            let d = Development::new(i as f32 / 100.0);
            if let Some(p) = prev {
                assert!(d.lr_multiplier() <= p.lr_multiplier());
                assert!(d.kick_rate(0.05) <= p.kick_rate(0.05));
                assert!(d.threshold(0.3, 0.9) >= p.threshold(0.3, 0.9));
                assert!(d.subconscious_budget(1.0, false) <= p.subconscious_budget(1.0, false));
                assert!(p.lr_multiplier() - d.lr_multiplier() < 0.011);
            }
            prev = Some(d);
        }
    }

    #[test]
    fn rho_is_a_fraction() {
        let mut d = Development::new(0.0);
        for i in 0..100 {
            d.record_experience();
            if i % 3 == 0 {
                d.record_useful();
            }
        }
        assert!((d.rho() - 34.0 / 100.0).abs() < 1e-6);
    }

    #[test]
    fn llm_agents_skip_stage_one() {
        assert_eq!(Development::new(0.0).stage(), Stage::Initialization);
        assert_eq!(Development::new(0.5).stage(), Stage::RapidDevelopment);
    }

    #[test]
    fn young_agent_kicks_more() {
        let young = Development::new(0.0);
        let mut old = Development::new(0.0);
        for _ in 0..10 {
            old.record_experience();
            old.record_useful();
        }
        assert!(young.kick_rate(0.05) > old.kick_rate(0.05));
        assert!((young.kick_rate(0.05) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn maturation_band() {
        assert_eq!(Development::new(0.7).stage(), Stage::Maturation);
        assert_eq!(Development::new(0.0).rho(), 0.0, "no experiences yet");
    }

    #[test]
    fn useful_never_exceeds_total() {
        let mut d = Development::new(0.0);
        d.record_useful();
        d.record_useful();
        assert_eq!(d.rho(), 0.0);
        d.record_experience();
        d.record_useful();
        d.record_useful();
        assert_eq!(d.rho(), 1.0);
        assert_eq!(
            d.subconscious_budget(1.0, true),
            3.0,
            "rest triples the budget"
        );
    }
}

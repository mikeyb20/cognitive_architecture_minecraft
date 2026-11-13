//! 01 §5 Memory: items, tiers, strength σ, priority P and habituation h
//! (spec §17), the store with its association index, and the skill table.

pub mod skills;
pub mod store;

pub use skills::{ActionKey, InternalAction, SkillTable};
pub use store::{MemoryStore, Source};

use serde::{Deserialize, Serialize};

/// Memory tiers (01 §5.2). Only the decay order is fixed by 01; the rates
/// are per-agent config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tier {
    Procedural,
    Semantic,
    Episodic,
    Sensory,
}

/// Per-tier decay λ (per tick) and base retention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TierParams {
    pub lambda: f32,
    pub base: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryParams {
    pub procedural: TierParams,
    pub semantic: TierParams,
    pub episodic: TierParams,
    pub sensory: TierParams,
    /// Reinforcement coefficient c_r in `c_r·ln(1+n)`.
    pub c_r: f32,
    /// Flat memory (plan §5.2 ablation): every item gets the same tier
    /// retention, the same priority and no decay.
    #[serde(default)]
    pub flat: bool,
}

/// Retention of every item under flat memory: no decay, full base.
pub const FLAT_TIER: TierParams = TierParams {
    lambda: 0.0,
    base: 1.0,
};

/// Priority of every item under flat memory: the middle of [0, 1]
/// (proposed).
pub const FLAT_PRIORITY: f32 = 0.5;

impl Default for MemoryParams {
    /// Spec §17 (proposed).
    fn default() -> Self {
        let t = |lambda, base| TierParams { lambda, base };
        Self {
            procedural: t(1e-6, 1.0),
            semantic: t(1e-5, 0.8),
            episodic: t(1e-4, 0.6),
            sensory: t(1e-2, 0.2),
            c_r: 0.05,
            flat: false,
        }
    }
}

impl MemoryParams {
    /// Decay and retention of a tier (01 §5.2), or `FLAT_TIER` for all
    /// tiers under flat memory.
    pub fn tier(&self, tier: Tier) -> TierParams {
        if self.flat {
            return FLAT_TIER;
        }
        match tier {
            Tier::Procedural => self.procedural,
            Tier::Semantic => self.semantic,
            Tier::Episodic => self.episodic,
            Tier::Sensory => self.sensory,
        }
    }

    /// The priority memory acts on (01 §5.3): the item's own, or
    /// `FLAT_PRIORITY` under flat memory.
    pub fn priority(&self, own: f32) -> f32 {
        if self.flat { FLAT_PRIORITY } else { own }
    }

    /// Check the one ordering 01 §5.2 fixes: procedural decays slowest,
    /// sensory fastest. Configs that break it are rejected.
    pub fn respects_tier_order(&self) -> bool {
        let l = [
            self.procedural.lambda,
            self.semantic.lambda,
            self.episodic.lambda,
            self.sensory.lambda,
        ];
        l.windows(2).all(|w| w[0] < w[1])
    }
}

/// A memory item: exactly the fields of 01 §5.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryItem {
    pub content: String,
    pub tier: Tier,
    /// Priority P ∈ [0, 1].
    pub priority: f32,
    /// Encoding valence V_enc ∈ [−1, 1].
    pub v_enc: f32,
    /// Habituation h ∈ [0, 1].
    pub habituation: f32,
    pub t_created: u64,
    pub t_last_access: u64,
    pub access_count: u32,
}

impl MemoryItem {
    /// `σ(t) = P·base(tier)·e^(−λ(tier)·(t − t_last_access)) + c_r·ln(1 + n)`
    /// (01 §5.3; spec §17).
    pub fn strength(&self, now: u64, p: &MemoryParams) -> f32 {
        let tp = p.tier(self.tier);
        let dt = now.saturating_sub(self.t_last_access) as f32;
        p.priority(self.priority) * tp.base * (-tp.lambda * dt).exp()
            + p.c_r * (1.0 + self.access_count as f32).ln()
    }

    /// Exposure with no new information: `h ← h + 0.1·(1 − h)` (spec §17).
    pub fn habituate(&mut self) {
        self.habituation += 0.1 * (1.0 - self.habituation);
    }

    /// Per tick while the target is absent: `h ← h·(1 − 0.001)`.
    pub fn dishabituate(&mut self) {
        self.habituation *= 1.0 - 0.001;
    }
}

/// Inputs to initial priority (01 §5.3, §4.9).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PriorityInputs {
    pub v_enc: f32,
    /// 1 if the item concerns damage, threat or food.
    pub survival: f32,
    /// min(1, exposures during encoding / 3).
    pub repetition: f32,
    /// links / (links + 3).
    pub connection: f32,
    /// C at encoding.
    pub c_enc: f32,
    /// Urgency U.
    pub urgency: f32,
}

/// `P_init = clamp(0.1 + 0.3|V_enc| + 0.25·survival + 0.1·repetition
///          + 0.15·conn + 0.1·C_enc + 0.2·U·|V_enc|, 0, 1)` (spec §17, proposed).
pub fn initial_priority(x: PriorityInputs) -> f32 {
    let v = x.v_enc.abs();
    (0.1 + 0.3 * v
        + 0.25 * x.survival
        + 0.1 * x.repetition
        + 0.15 * x.connection
        + 0.1 * x.c_enc
        + 0.2 * x.urgency * v)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn item(tier: Tier) -> MemoryItem {
        MemoryItem {
            content: String::new(),
            tier,
            priority: 0.7,
            v_enc: 0.0,
            habituation: 0.0,
            t_created: 0,
            t_last_access: 0,
            access_count: 0,
        }
    }

    proptest! {
        /// Spec §27 gate: σ order = tier order. With equal P and history, after
        /// any delay, procedural ≥ semantic ≥ episodic ≥ sensory (01 §5.2).
        #[test]
        fn strength_follows_tier_order(dt in 0u64..10_000_000) {
            let p = MemoryParams::default();
            let s: Vec<f32> = [Tier::Procedural, Tier::Semantic, Tier::Episodic, Tier::Sensory]
                .iter()
                .map(|t| item(*t).strength(dt, &p))
                .collect();
            for w in s.windows(2) {
                prop_assert!(w[0] >= w[1], "{s:?}");
            }
        }

        #[test]
        fn priority_in_range(
            v in -1.0f32..=1.0, s in 0.0f32..=1.0, r in 0.0f32..=1.0,
            c in 0.0f32..=1.0, ce in 0.0f32..=1.0, u in 0.0f32..=1.0,
        ) {
            let p = initial_priority(PriorityInputs {
                v_enc: v, survival: s, repetition: r, connection: c, c_enc: ce, urgency: u,
            });
            prop_assert!((0.0..=1.0).contains(&p));
        }
    }

    #[test]
    fn flat_memory_has_one_tier_one_priority_and_no_decay() {
        let p = MemoryParams {
            flat: true,
            ..MemoryParams::default()
        };
        let mut low = item(Tier::Sensory);
        low.priority = 0.05;
        // σ = 0.5·1·e^0 + 0.05·ln(1) at any age, whatever the tier and P.
        for it in [item(Tier::Procedural), item(Tier::Episodic), low] {
            assert_eq!(it.strength(10_000_000, &p), 0.5);
        }
        assert_eq!((p.tier(Tier::Sensory), p.priority(0.9)), (FLAT_TIER, 0.5));
        let d = MemoryParams::default();
        assert_eq!((d.tier(Tier::Sensory), d.priority(0.9)), (d.sensory, 0.9));
        // Hand-worked default: 0.7·0.2·e^(−0.01·100) = 0.14·0.3679.
        let s = item(Tier::Sensory).strength(100, &d);
        assert!((s - 0.051_503).abs() < 1e-5, "{s}");
    }

    #[test]
    fn default_rates_respect_tier_order() {
        assert!(MemoryParams::default().respects_tier_order());
        let mut bad = MemoryParams::default();
        bad.sensory.lambda = 1e-7;
        assert!(!bad.respects_tier_order());
    }

    #[test]
    fn strong_valence_raises_priority_more_under_urgency() {
        let base = PriorityInputs {
            v_enc: -0.9,
            ..Default::default()
        };
        let calm = initial_priority(base);
        let urgent = initial_priority(PriorityInputs {
            urgency: 1.0,
            ..base
        });
        assert!(urgent > calm && calm > initial_priority(PriorityInputs::default()));
    }

    #[test]
    fn habituation_rises_and_fades() {
        let mut m = item(Tier::Semantic);
        for _ in 0..30 {
            m.habituate();
        }
        assert!(m.habituation > 0.9);
        for _ in 0..1000 {
            m.dishabituate();
        }
        assert!(m.habituation < 0.4);
    }
}

//! The subconscious loop (01 §4.2; spec §17): background jobs over memory
//! under a budget B(ρ, rest). Each tick it walks the next B items:
//! consolidate (re-score P, prune, reinforce), curate TM (flag unresolved or
//! important items) and nudge C when something significant turns up.

use crate::memory::MemoryStore;
use serde::{Deserialize, Serialize};

/// Spec §17 (all proposed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubconsciousParams {
    /// Items processed per tick before the ρ and rest multipliers (spec §14).
    pub b_base: f32,
    /// Prune below this strength σ.
    pub sigma_min: f32,
    /// Flag for TM above this significance.
    pub flag_threshold: f32,
    /// Nudge C above this significance (θ_sig).
    pub nudge_threshold: f32,
    /// Nudge size per unit significance (s).
    pub nudge: f32,
    /// Selection mix (OQ12): unresolved, |V_enc|, P.
    pub mix: [f32; 3],
    /// P re-scoring rate toward recent usefulness.
    pub rescore_rate: f32,
}

impl Default for SubconsciousParams {
    fn default() -> Self {
        Self {
            b_base: 4.0,
            sigma_min: 0.05,
            flag_threshold: 0.5,
            nudge_threshold: 0.6,
            nudge: 0.05,
            mix: [0.4, 0.3, 0.3],
            rescore_rate: 0.05,
        }
    }
}

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub processed: u32,
    pub pruned: u32,
    pub reinforced: u32,
    /// Items flagged for TM (admitted at rate π by the caller).
    pub flagged: Vec<usize>,
    /// C nudge (the caller scales it by π and dampens rises by D).
    pub nudge: f32,
}

/// Fade of an item's recent usefulness per scan (proposed).
pub const USEFUL_FADE: f32 = 0.99;
/// Usefulness above which consolidation reinforces an item (proposed).
pub const REINFORCE_AT: f32 = 0.5;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subconscious {
    pub params: SubconsciousParams,
    cursor: usize,
    carry: f32,
    /// EMA of the share of scanned items that are prune or merge candidates.
    weak_share: f32,
}

impl Subconscious {
    pub fn new(params: SubconsciousParams) -> Self {
        Self {
            params,
            ..Self::default()
        }
    }

    /// Share of recently scanned items near the prune line (σ < 2·σ_min),
    /// the `consolidate` feature (spec §6).
    pub fn weak_share(&self) -> f32 {
        self.weak_share
    }

    /// Significance `mix·[unresolved, |V_enc|, P]` (OQ12 default).
    pub fn significance(&self, unresolved: bool, v_enc: f32, p: f32) -> f32 {
        let m = self.params.mix;
        m[0] * f32::from(u8::from(unresolved)) + m[1] * v_enc.abs() + m[2] * p
    }

    /// Process `budget` items (fractions carry over between ticks).
    pub fn run(&mut self, mem: &mut MemoryStore, budget: f32, now: u64) -> Report {
        let mut r = Report::default();
        self.carry += budget.max(0.0);
        let n = self.carry.floor() as usize;
        self.carry -= n as f32;
        let cap = mem.capacity();
        if cap == 0 {
            return r;
        }
        for _ in 0..n.min(cap) {
            let id = self.cursor % cap;
            self.cursor = (self.cursor + 1) % cap;
            if mem.is_alive(id) {
                self.visit(mem, id, now, &mut r);
            }
        }
        r
    }

    fn visit(&mut self, mem: &mut MemoryStore, id: usize, now: u64, r: &mut Report) {
        let p = &self.params;
        r.processed += 1;
        let sigma = mem.strength(id, now);
        let weak = f32::from(u8::from(sigma < 2.0 * p.sigma_min));
        self.weak_share += 0.05 * (weak - self.weak_share);
        if sigma < p.sigma_min {
            mem.prune(id);
            r.pruned += 1;
            return;
        }
        let (useful, unresolved, v_enc, prio) = {
            let e = mem.entry_mut(id);
            e.useful *= USEFUL_FADE;
            e.item.priority += p.rescore_rate * (e.useful - e.item.priority);
            e.item.priority = e.item.priority.clamp(0.0, 1.0);
            (e.useful, e.unresolved, e.item.v_enc, e.item.priority)
        };
        if useful > REINFORCE_AT {
            mem.access(id, now);
            r.reinforced += 1;
        }
        let sig = self.significance(unresolved, v_enc, prio);
        if sig > p.flag_threshold {
            r.flagged.push(id);
        }
        if sig > p.nudge_threshold {
            r.nudge = r.nudge.max(p.nudge * sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryParams, PriorityInputs, Source, Tier};

    fn mem_with(items: &[(&str, Tier, f32)]) -> MemoryStore {
        let mut m = MemoryStore::new(MemoryParams::default());
        for (k, t, v) in items {
            let x = PriorityInputs {
                v_enc: *v,
                ..PriorityInputs::default()
            };
            m.encode(k, *t, x, 0, Source::Perception);
        }
        m
    }

    #[test]
    fn budget_carries_fractions_and_wraps() {
        let mut m = mem_with(&[("a", Tier::Semantic, 0.0), ("b", Tier::Semantic, 0.0)]);
        let mut s = Subconscious::default();
        assert_eq!(s.run(&mut m, 0.5, 0).processed, 0);
        assert_eq!(s.run(&mut m, 0.5, 0).processed, 1);
        assert_eq!(
            s.run(&mut m, 5.0, 0).processed,
            2,
            "at most every item once"
        );
        let mut empty = MemoryStore::default();
        assert_eq!(s.run(&mut empty, 3.0, 0), Report::default());
        assert_eq!(s.run(&mut m, -1.0, 0).processed, 0);
    }

    #[test]
    fn weak_items_are_pruned() {
        // Episodic P = 0.1, base 0.6: σ = 0.06·e^(−1e-4·Δt). Below 0.05
        // after about 1,823 ticks.
        let mut m = mem_with(&[("ep", Tier::Episodic, 0.0)]);
        let mut s = Subconscious::default();
        assert_eq!(s.run(&mut m, 1.0, 1000).pruned, 0);
        assert!(s.weak_share() > 0.0, "σ < 0.1 counts as weak");
        let r = s.run(&mut m, 1.0, 2000);
        assert_eq!((r.pruned, m.len()), (1, 0));
        assert_eq!(s.run(&mut m, 1.0, 2000).processed, 0, "dead items skipped");
    }

    #[test]
    fn useful_items_are_rescored_and_reinforced() {
        let mut m = mem_with(&[("a", Tier::Semantic, 0.0)]);
        m.entry_mut(0).useful = 1.0;
        let mut s = Subconscious::default();
        let r = s.run(&mut m, 1.0, 5);
        assert_eq!(r.reinforced, 1);
        let e = m.entry(0);
        assert!((e.useful - 0.99).abs() < 1e-6);
        assert!((e.item.priority - (0.1 + 0.05 * (0.99 - 0.1))).abs() < 1e-6);
        assert_eq!((e.item.access_count, e.item.t_last_access), (1, 5));
        assert!((s.weak_share() - 0.05).abs() < 1e-6, "σ = 0.08 < 2·σ_min");
    }

    #[test]
    fn significant_items_are_flagged_and_nudge() {
        let mut m = mem_with(&[
            ("calm", Tier::Semantic, 0.0),
            ("scary", Tier::Episodic, -1.0),
        ]);
        m.entry_mut(1).unresolved = true;
        let mut s = Subconscious::default();
        let r = s.run(&mut m, 2.0, 0);
        assert_eq!(r.flagged, vec![1]);
        // sig = 0.4 + 0.3 + 0.3·P, P ≈ 0.4 (|V| = 1) re-scored toward 0.
        let p = m.entry(1).item.priority;
        let sig = 0.7 + 0.3 * p;
        assert!((r.nudge - 0.05 * sig).abs() < 1e-6);
        assert!((s.significance(false, -0.5, 1.0) - 0.45).abs() < 1e-6);
    }
}

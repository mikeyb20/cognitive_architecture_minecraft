//! The memory store (01 §5; spec §17): items keyed by content, plus the
//! store-level indexes that are not item fields: associations, provenance,
//! remembered danger and cheapest routes.
//!
//! Item ids are stable: a pruned item keeps its slot (marked dead) so ids held
//! in thought momentum never point at the wrong item.

use super::{MemoryItem, MemoryParams, PriorityInputs, Tier, initial_priority};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where an item came from (00 §6.7; spec §17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Perception,
    PlayerChat,
    Llm,
    Reflection,
}

impl Source {
    /// Starting confidence: LLM-made items start at 0.6 (spec §17).
    pub fn confidence(self) -> f32 {
        match self {
            Source::Llm => 0.6,
            _ => 1.0,
        }
    }
}

/// One stored item and its store-level data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub item: MemoryItem,
    pub source: Source,
    pub confidence: f32,
    pub alive: bool,
    /// Last tick the target was perceived (for lazy dishabituation).
    pub last_seen: u64,
    /// Association index: (other id, weight).
    pub links: Vec<(usize, f32)>,
    /// Remembered danger of the target, 0..1 (spec §3 f7).
    pub danger: f32,
    /// Recent usefulness for priority re-scoring (spec §17).
    pub useful: f32,
    /// Last tick this item was in thought momentum (spec §14 usefulness).
    pub last_tm: Option<u64>,
    /// Counted as useful toward ρ already (each item counts once).
    pub counted_useful: bool,
    /// Episodic items: turned into semantic items by `reflect` yet.
    pub reflected: bool,
    /// Episodic items with V_enc < 0 and no later success on the same
    /// target (spec §17 "unresolved").
    pub unresolved: bool,
}

/// Weight added to an association per co-activation (proposed).
pub const LINK_STEP: f32 = 0.1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryStore {
    pub params: MemoryParams,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
    /// Cheapest remembered cost per result key (spec §3 f6).
    routes: BTreeMap<String, f32>,
    alive: usize,
}

impl MemoryStore {
    pub fn new(params: MemoryParams) -> Self {
        Self {
            params,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.alive
    }

    pub fn is_empty(&self) -> bool {
        self.alive == 0
    }

    /// Slots ever used, alive or not (ids are `0..capacity()`).
    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }

    pub fn entry(&self, id: usize) -> &Entry {
        &self.entries[id]
    }

    pub fn entry_mut(&mut self, id: usize) -> &mut Entry {
        &mut self.entries[id]
    }

    pub fn is_alive(&self, id: usize) -> bool {
        self.entries.get(id).is_some_and(|e| e.alive)
    }

    /// Store a new item, or return the existing id for this content
    /// (bool = newly created). A pruned item with the same content is
    /// re-learned in its old slot.
    pub fn encode(
        &mut self,
        content: &str,
        tier: Tier,
        x: PriorityInputs,
        now: u64,
        source: Source,
    ) -> (usize, bool) {
        if let Some(id) = self.get(content) {
            if self.entries[id].alive {
                return (id, false);
            }
            self.entries[id] = Self::fresh(content, tier, x, now, source);
            self.alive += 1;
            return (id, true);
        }
        let id = self.entries.len();
        self.entries
            .push(Self::fresh(content, tier, x, now, source));
        self.index.insert(content.to_string(), id);
        self.alive += 1;
        (id, true)
    }

    fn fresh(content: &str, tier: Tier, x: PriorityInputs, now: u64, source: Source) -> Entry {
        Entry {
            item: MemoryItem {
                content: content.to_string(),
                tier,
                priority: initial_priority(x),
                v_enc: x.v_enc.clamp(-1.0, 1.0),
                habituation: 0.0,
                t_created: now,
                t_last_access: now,
                access_count: 0,
            },
            source,
            confidence: source.confidence(),
            alive: true,
            last_seen: now,
            links: Vec::new(),
            danger: 0.0,
            useful: 0.0,
            last_tm: None,
            counted_useful: false,
            reflected: false,
            unresolved: false,
        }
    }

    /// Retrieval: one more access at `now` (raises σ through `c_r·ln(1+n)`).
    pub fn access(&mut self, id: usize, now: u64) {
        let it = &mut self.entries[id].item;
        it.access_count += 1;
        it.t_last_access = now;
    }

    /// Habituation h at `now`, applying `h·(1 − 0.001)` for every tick the
    /// target was absent (01 §5.1; spec §17), without touching every item
    /// every tick.
    pub fn habituation(&self, id: usize, now: u64) -> f32 {
        let e = &self.entries[id];
        let absent = now.saturating_sub(e.last_seen).min(i32::MAX as u64) as i32;
        e.item.habituation * (1.0f32 - 0.001).powi(absent)
    }

    /// Exposure to the target. With no new information it habituates
    /// (`h ← h + 0.1·(1 − h)`, spec §17).
    pub fn expose(&mut self, id: usize, now: u64, informative: bool) {
        let h = self.habituation(id, now);
        let e = &mut self.entries[id];
        e.item.habituation = h;
        if !informative {
            e.item.habituate();
        }
        e.last_seen = now;
    }

    /// Novelty of a target: 1 when there is no memory of it, falling with
    /// each retrieval (proposed: `1/(1 + n)`).
    pub fn novelty(&self, key: &str) -> f32 {
        match self.get(key) {
            Some(id) if self.entries[id].alive => {
                1.0 / (1.0 + self.entries[id].item.access_count as f32)
            }
            _ => 1.0,
        }
    }

    /// σ at `now` (01 §5.3).
    pub fn strength(&self, id: usize, now: u64) -> f32 {
        self.entries[id].item.strength(now, &self.params)
    }

    /// Strengthen the association between two items (co-activation in TM).
    pub fn link(&mut self, a: usize, b: usize, w: f32) {
        if a == b {
            return;
        }
        for (x, y) in [(a, b), (b, a)] {
            let links = &mut self.entries[x].links;
            match links.iter_mut().find(|(o, _)| *o == y) {
                Some((_, lw)) => *lw += w,
                None => links.push((y, w)),
            }
        }
    }

    /// Live links of an item.
    pub fn links(&self, id: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.entries[id]
            .links
            .iter()
            .copied()
            .filter(|(o, _)| self.entries[*o].alive)
    }

    pub fn linked(&self, a: usize, b: usize) -> bool {
        self.links(a).any(|(o, _)| o == b)
    }

    /// Connection term `links/(links + 3)` (spec §17).
    pub fn connection(&self, id: usize) -> f32 {
        let n = self.links(id).count() as f32;
        n / (n + 3.0)
    }

    /// Remove an item (σ fell below σ_min). Its slot stays, marked dead.
    pub fn prune(&mut self, id: usize) -> bool {
        let e = &mut self.entries[id];
        if !e.alive {
            return false;
        }
        e.alive = false;
        e.links.clear();
        self.alive -= 1;
        true
    }

    /// Remember the cost of reaching a result; returns the previous cheapest.
    pub fn record_route(&mut self, key: &str, cost: f32) -> Option<f32> {
        let prev = self.routes.get(key).copied();
        let best = prev.map_or(cost, |p| p.min(cost));
        self.routes.insert(key.to_string(), best);
        prev
    }

    /// Drop in expected cost vs the cheapest remembered route (spec §3 f6).
    pub fn saving(&self, key: &str, cost: f32) -> f32 {
        match self.routes.get(key) {
            Some(&best) if best > 0.0 => ((best - cost) / best).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// Σσ per tier, in the order procedural, semantic, episodic, sensory
    /// (the forgetting signature, 00 §6.5; spec §17).
    pub fn strength_by_tier(&self, now: u64) -> [f32; 4] {
        let mut out = [0.0; 4];
        for e in self.entries.iter().filter(|e| e.alive) {
            let i = match e.item.tier {
                Tier::Procedural => 0,
                Tier::Semantic => 1,
                Tier::Episodic => 2,
                Tier::Sensory => 3,
            };
            out[i] += e.item.strength(now, &self.params);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::new(MemoryParams::default())
    }

    fn enc(s: &mut MemoryStore, key: &str, now: u64) -> usize {
        s.encode(
            key,
            Tier::Semantic,
            PriorityInputs::default(),
            now,
            Source::Perception,
        )
        .0
    }

    #[test]
    fn encode_is_idempotent_and_prune_keeps_ids() {
        let mut s = store();
        assert!(s.is_empty());
        let (a, new) = s.encode(
            "block:stone",
            Tier::Semantic,
            PriorityInputs::default(),
            5,
            Source::Llm,
        );
        assert!(new);
        assert_eq!(s.entry(a).confidence, 0.6);
        assert_eq!(s.entry(a).item.priority, 0.1);
        let (a2, new2) = s.encode(
            "block:stone",
            Tier::Episodic,
            PriorityInputs::default(),
            9,
            Source::Perception,
        );
        assert_eq!((a2, new2), (a, false));
        let b = enc(&mut s, "block:oak_log", 6);
        assert_eq!((b, s.len(), s.capacity()), (1, 2, 2));
        assert!(s.prune(a));
        assert!(!s.prune(a), "already pruned");
        assert!(!s.is_alive(a) && s.is_alive(b) && !s.is_alive(99));
        assert_eq!((s.len(), s.capacity()), (1, 2));
        let (a3, new3) = s.encode(
            "block:stone",
            Tier::Semantic,
            PriorityInputs::default(),
            20,
            Source::Perception,
        );
        assert_eq!((a3, new3, s.len()), (a, true, 2), "re-learned in its slot");
        assert_eq!(s.entry(a).item.t_created, 20);
        assert_eq!(s.entry_mut(b).item.content, "block:oak_log");
        assert_eq!(s.get("nope"), None);
    }

    #[test]
    fn novelty_falls_with_access_and_returns_after_pruning() {
        let mut s = store();
        assert_eq!(s.novelty("block:stone"), 1.0);
        let a = enc(&mut s, "block:stone", 0);
        assert_eq!(s.novelty("block:stone"), 1.0);
        s.access(a, 3);
        assert_eq!(s.novelty("block:stone"), 0.5);
        assert_eq!(s.entry(a).item.t_last_access, 3);
        s.prune(a);
        assert_eq!(s.novelty("block:stone"), 1.0);
    }

    #[test]
    fn habituation_rises_on_dull_exposure_and_fades_lazily() {
        let mut s = store();
        let a = enc(&mut s, "block:stone", 0);
        s.expose(a, 0, false);
        assert!((s.habituation(a, 0) - 0.1).abs() < 1e-6);
        s.expose(a, 0, false);
        assert!((s.habituation(a, 0) - 0.19).abs() < 1e-6);
        let later = s.habituation(a, 100);
        assert!((later - 0.19 * 0.999f32.powi(100)).abs() < 1e-6);
        s.expose(a, 100, true);
        assert!((s.habituation(a, 100) - later).abs() < 1e-6, "informative");
        assert_eq!(s.entry(a).last_seen, 100);
    }

    #[test]
    fn links_are_symmetric_and_skip_dead_items() {
        let mut s = store();
        let (a, b, c) = (
            enc(&mut s, "a", 0),
            enc(&mut s, "b", 0),
            enc(&mut s, "c", 0),
        );
        s.link(a, a, 1.0);
        assert_eq!(s.links(a).count(), 0, "no self links");
        s.link(a, b, LINK_STEP);
        s.link(b, a, LINK_STEP);
        s.link(a, c, 0.5);
        assert!(s.linked(a, b) && s.linked(b, a) && !s.linked(b, c));
        assert_eq!(s.links(a).collect::<Vec<_>>(), vec![(b, 0.2), (c, 0.5)]);
        assert!((s.connection(a) - 0.4).abs() < 1e-6);
        s.prune(c);
        assert_eq!(s.links(a).count(), 1);
        assert_eq!(s.connection(c), 0.0);
    }

    #[test]
    fn routes_and_saving() {
        let mut s = store();
        assert_eq!(s.saving("chop:oak_log", 0.2), 0.0, "nothing remembered");
        assert_eq!(s.record_route("chop:oak_log", 0.4), None);
        assert_eq!(s.record_route("chop:oak_log", 0.5), Some(0.4));
        assert!((s.saving("chop:oak_log", 0.1) - 0.75).abs() < 1e-6);
        assert_eq!(s.saving("chop:oak_log", 0.6), 0.0, "dearer: no saving");
        s.record_route("free", 0.0);
        assert_eq!(s.saving("free", 0.0), 0.0);
    }

    #[test]
    fn strength_by_tier_sums_live_items() {
        let mut s = store();
        let x = PriorityInputs {
            v_enc: 1.0,
            ..PriorityInputs::default()
        };
        for (k, t) in [
            ("p", Tier::Procedural),
            ("s", Tier::Semantic),
            ("e", Tier::Episodic),
            ("x", Tier::Sensory),
        ] {
            s.encode(k, t, x, 0, Source::Perception);
        }
        let by = s.strength_by_tier(0);
        // P = 0.4; σ = P·base at Δt = 0.
        let want = [0.4, 0.32, 0.24, 0.08];
        for (g, w) in by.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{by:?}");
        }
        assert_eq!(s.strength(0, 0), by[0]);
        s.prune(3);
        assert_eq!(s.strength_by_tier(0)[3], 0.0);
        assert_eq!(s.entry(1).item.v_enc, 1.0);
        assert_eq!(Source::Reflection.confidence(), 1.0);
    }
}

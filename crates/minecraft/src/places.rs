//! Where things were seen (v0.2 R4.9; Step 20 test 4: "mine more iron"
//! goes straight back to stored iron). The host keeps this, not the brain:
//! it is how the body finds its way back, and every arm gets the same map.

use std::collections::BTreeMap;

use host_api::command::Whereabouts;
use host_api::craft::same_block;
use host_api::{Observation, Pos, SeenKind};

use crate::dist;

/// Place memory limits (proposed until the pilot).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaceParams {
    /// Spots kept per thing; the oldest goes first.
    pub max_per_thing: usize,
    /// A remembered spot this close that is no longer seen is forgotten.
    pub confirm_radius: f32,
}

impl Default for PlaceParams {
    fn default() -> Self {
        Self {
            max_per_thing: 32,
            confirm_radius: 4.0,
        }
    }
}

/// Blocks by memory key ("block:iron_ore"), oldest spot first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Places {
    params: PlaceParams,
    spots: BTreeMap<String, Vec<Pos>>,
}

impl Places {
    pub fn new(params: PlaceParams) -> Self {
        Self {
            params,
            spots: BTreeMap::new(),
        }
    }

    /// Remember that `thing` is at `p` (refreshing it if known).
    pub fn remember(&mut self, thing: &str, p: Pos) {
        let spots = self.spots.entry(thing.to_string()).or_default();
        spots.retain(|q| *q != p);
        spots.push(p);
        if spots.len() > self.params.max_per_thing {
            spots.remove(0);
        }
    }

    /// The thing is gone from `p` (mined, or missing on arrival).
    pub fn forget(&mut self, thing: &str, p: Pos) {
        if let Some(spots) = self.spots.get_mut(thing) {
            spots.retain(|q| *q != p);
        }
    }

    /// Update from one observation: remember every block in view, and
    /// forget nearby spots that should be in view but are not. The bot
    /// lists only the nearest blocks, so a spot counts as checked only if
    /// it is nearer than the farthest block listed.
    pub fn see(&mut self, obs: &Observation) {
        let blocks: Vec<_> = obs
            .visible
            .iter()
            .filter(|s| s.kind == SeenKind::Block)
            .collect();
        let horizon = blocks
            .iter()
            .map(|s| s.distance)
            .fold(0.0_f32, f32::max)
            .min(self.params.confirm_radius);
        let me = obs.position;
        for spots in self.spots.values_mut() {
            spots.retain(|p| dist(me, *p) >= horizon);
        }
        for s in blocks {
            self.remember(&s.thing.0, s.pos);
        }
    }

    /// The nearest stored spot of `thing` (or its deepslate form) to
    /// `from`; ties go to the newer spot.
    pub fn nearest(&self, thing: &str, from: Pos) -> Option<Pos> {
        let mut best: Option<(f32, Pos)> = None;
        for (key, spots) in &self.spots {
            if !same_block(key, thing) {
                continue;
            }
            for p in spots {
                let d = dist(from, *p);
                if best.is_none_or(|(b, _)| d <= b) {
                    best = Some((d, *p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// Spots stored for exactly `thing`.
    pub fn count(&self, thing: &str) -> usize {
        self.spots.get(thing).map_or(0, Vec::len)
    }
}

impl Whereabouts for Places {
    fn is_at(&self, p: Pos, thing: &str) -> bool {
        self.spots
            .iter()
            .any(|(key, spots)| same_block(key, thing) && spots.contains(&p))
    }
}

//! `SimWorld`: the seeded gridworld behind [`World`] and [`Host`] (spec §2, §21).
//!
//! One agent for now (#31 keeps multi-agent in the types). Everything random
//! goes through one ChaCha8 stream, so a seed replays exactly.

use std::collections::HashSet;

use host_api::{Inventory, Pos, Scenario, ThingId, World};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::craft::{self};
use crate::geom::{cheb, dist, div_ceil, square};
use crate::map::{Biome, Map, Tile};
use crate::mob::Mob;
use crate::rules::Rules;

/// Most candidates offered per decision (spec §6: K = 20).
pub use host_api::MAX_CANDIDATES;
/// Most block entries in one observation; entities are always listed.
pub const MAX_VISIBLE_BLOCKS: usize = 48;
pub use host_api::ontology::REGION;
/// Furthest a path search looks from the agent, in tiles.
pub const PATH_RADIUS: i32 = 20;

mod danger;
mod host;
mod mobs;
mod path;
mod ranged;
mod shelter;
mod shock;
pub use danger::MobTest;
pub use path::Reach;
pub use shelter::ShelterTest;
pub use shock::Shock;

/// What killed the agent, as the world reports it. Outcome first (#43): the
/// cause behind the cause is worked out later from the log, never guessed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Death {
    Starved,
    /// A melee hit (a zombie).
    Mob,
    /// A skeleton's arrow (added 15 Nov 2026; earlier runs had none).
    Arrow,
    /// A shock bout (benchmark B3).
    Shock,
}

/// Food items and how much each restores.
pub fn food_value(rules: &Rules, item: &str) -> Option<f32> {
    match item {
        "bread" => Some(rules.bread_food),
        "porkchop" => Some(rules.meat_food),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub pos: Pos,
    pub health: f32,
    pub food: f32,
    pub inv: Inventory,
    pub resting: bool,
    pub death: Option<Death>,
}

#[derive(Debug, Clone)]
pub struct SimWorld {
    pub rules: Rules,
    pub(crate) rng: ChaCha8Rng,
    pub map: Map,
    pub tick: u64,
    pub body: Body,
    pub mobs: Vec<Mob>,
    /// Shock bouts, if the scenario has them (benchmark B3).
    pub shock: Option<Shock>,
    /// Something cut the current action short (a shock bout began, B3).
    interrupted: bool,
    /// The current action began with no hostile near (see `danger`).
    calm_start: bool,
    known: HashSet<String>,
    region_visits: Vec<u32>,
    pending_new: Vec<ThingId>,
    damage_since_perceive: f32,
    action_damage: f32,
}

impl SimWorld {
    pub fn new(rules: Rules, seed: u64) -> Self {
        let mut w = Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            map: Map::generate(8, rules.tree_chops, &mut ChaCha8Rng::seed_from_u64(0)),
            rules,
            tick: 0,
            body: Body {
                pos: Pos::default(),
                health: 1.0,
                food: 1.0,
                inv: Inventory::default(),
                resting: false,
                death: None,
            },
            mobs: Vec::new(),
            shock: None,
            interrupted: false,
            calm_start: false,
            known: HashSet::new(),
            region_visits: Vec::new(),
            pending_new: Vec::new(),
            damage_since_perceive: 0.0,
            action_damage: 0.0,
        };
        w.reset(
            seed,
            &Scenario {
                name: "first_night".into(),
            },
        );
        w
    }

    /// A world on a given map with the agent at `start` and no mobs. For
    /// hand-built scenarios and tests.
    pub fn with_map(rules: Rules, seed: u64, map: Map, start: Pos) -> Self {
        let mut w = Self::new(rules, seed);
        w.map = map;
        w.mobs.clear();
        w.body.pos = start;
        w.start_at(start);
        w
    }

    pub fn alive(&self) -> bool {
        self.body.death.is_none()
    }

    /// 0..1 through the day.
    pub fn day_phase(&self) -> f32 {
        (self.tick % self.rules.day_ticks) as f32 / self.rules.day_ticks as f32
    }

    pub fn is_night(&self) -> bool {
        self.day_phase() >= 0.5
    }

    pub fn sheltered(&self) -> bool {
        self.map.get(self.body.pos) == Tile::Shelter
    }

    /// Forget what was seen and visited, and stand at `start`.
    fn start_at(&mut self, start: Pos) {
        let per_row = div_ceil(self.map.size, REGION);
        self.region_visits = vec![0; (per_row * per_row) as usize];
        self.known.clear();
        self.enter(start);
        self.pending_new.clear();
    }

    fn region_index(&self, p: Pos) -> usize {
        let per_row = div_ceil(self.map.size, REGION);
        ((p.z / REGION) * per_row + p.x / REGION) as usize
    }

    /// Times the agent has stepped into this tile's 8×8 region.
    pub fn region_visits(&self, p: Pos) -> u32 {
        if self.map.in_bounds(p) {
            self.region_visits[self.region_index(p)]
        } else {
            u32::MAX
        }
    }

    /// Record a thing as seen; queue it as new the first time.
    pub(crate) fn notice(&mut self, thing: &str) {
        if !self.known.contains(thing) {
            self.known.insert(thing.to_string());
            self.pending_new.push(ThingId(thing.to_string()));
        }
    }

    fn enter(&mut self, p: Pos) {
        self.body.pos = p;
        let i = self.region_index(p);
        self.region_visits[i] += 1;
        if self.region_visits[i] == 1 {
            self.notice(&host_api::ontology::region_of(p).0);
        }
        self.scan();
    }

    /// Notice every block and entity type in view.
    fn scan(&mut self) {
        let r = self.rules.view_radius;
        let c = self.body.pos;
        let mut names: Vec<String> = square(c, r)
            .filter(|p| self.map.in_bounds(*p))
            .map(|p| self.map.get(p))
            .filter(|t| t.notable())
            .map(|t| format!("block:{}", t.name()))
            .collect();
        for m in &self.mobs {
            if cheb(m.pos, c) <= r {
                names.push(format!("entity:{}", m.kind.name()));
            }
        }
        for n in names {
            self.notice(&n);
        }
    }

    fn hurt(&mut self, amount: f32, cause: Death) {
        if self.body.death.is_some() {
            return;
        }
        let dealt = amount.min(self.body.health);
        self.body.health -= dealt;
        self.damage_since_perceive += dealt;
        self.action_damage += dealt;
        if self.body.health <= 0.0 {
            self.body.death = Some(cause);
        }
    }

    fn tick_once(&mut self) {
        self.tick += 1;
        self.shock_tick();
        let food_decay = self.rules.food_decay;
        self.body.food = (self.body.food - food_decay).max(0.0);
        if self.body.food <= 0.0 {
            self.hurt(self.rules.starve_damage, Death::Starved);
        } else if self.body.food >= self.rules.regen_food_min {
            self.body.health = (self.body.health + self.rules.regen).min(1.0);
        }
        let night = self.is_night();
        let in_cave = self.map.biome(self.body.pos) == Biome::Cave;
        let p = if night {
            self.rules.night_spawn
        } else if in_cave {
            self.rules.cave_spawn
        } else {
            0.0
        };
        if self.hostile_count() < self.rules.max_hostiles && self.chance(p) {
            self.spawn_hostile();
        }
        if !night {
            let map = &self.map;
            self.mobs
                .retain(|m| !m.kind.hostile() || map.biome(m.pos) == Biome::Cave);
        }
        let sheltered = self.sheltered();
        // One path search per tick, and only when a chaser moves.
        let mut reach: Option<Reach> = None;
        for i in 0..self.mobs.len() {
            if self.tick >= self.mobs[i].next_move {
                let chase = if self.chases(i) {
                    Some(&*reach.get_or_insert_with(|| self.reach()))
                } else {
                    None
                };
                self.move_mob(i, chase);
                self.mobs[i].next_move = self.tick
                    + if self.mobs[i].kind.hostile() {
                        self.rules.mob_move_ticks
                    } else {
                        self.rules.mob_move_ticks * 4
                    };
            }
            if self.mobs[i].kind.ranged() {
                self.shoot(i, sheltered);
                continue;
            }
            let m = &self.mobs[i];
            if m.kind.hostile()
                && !sheltered
                && cheb(m.pos, self.body.pos) <= 1
                && self.tick >= m.next_hit
            {
                self.mobs[i].next_hit = self.tick + self.rules.mob_hit_ticks;
                self.hurt(self.rules.mob_damage, Death::Mob);
            }
        }
        self.check_danger();
    }

    /// Success roll: base × focus, less under threat (spec §21).
    pub fn success_chance(&self, focus: f32) -> f32 {
        let threatened = !self.sheltered()
            && self
                .mobs
                .iter()
                .any(|m| m.kind.hostile() && cheb(m.pos, self.body.pos) <= 2);
        let threat = if threatened {
            self.rules.threat_penalty
        } else {
            1.0
        };
        self.rules.base_success * focus.clamp(0.0, 1.0) * threat
    }

    /// True with probability `p`.
    fn chance(&mut self, p: f32) -> bool {
        self.rng.random::<f32>() < p
    }

    pub(crate) fn roll(&mut self, focus: f32) -> bool {
        let p = self.success_chance(focus);
        self.chance(p)
    }

    pub(crate) fn give(&mut self, item: &str, n: u32) {
        craft::give(&mut self.body.inv, item, n);
        self.notice(&format!("item:{item}"));
    }

    /// Risk estimate for a candidate around `p`, 0..1.
    pub(crate) fn risk_at(&self, p: Pos) -> f32 {
        let mut r = 0.0;
        if self.is_night() && !self.sheltered() {
            r += 0.3;
        }
        let near = self
            .mobs
            .iter()
            .filter(|m| m.kind.hostile() && cheb(m.pos, p) <= self.rules.view_radius)
            .count();
        r += 0.15 * near as f32;
        if square(p, 1).any(|q| self.map.get(q) == Tile::Lava) {
            r += 0.3;
        }
        r.min(1.0)
    }

    /// Cost estimate: a base for the act plus the walk, 0..1.
    pub(crate) fn cost(&self, steps: u16, base: f32) -> f32 {
        (base + f32::from(steps) / (4 * self.rules.max_move) as f32).min(1.0)
    }

    /// The in-view tile matching `want` that is fewest steps away, counting
    /// steps to stand next to it.
    pub(crate) fn nearest_tile(
        &self,
        reach: &Reach,
        want: impl Fn(Tile) -> bool,
    ) -> Option<(Pos, u16)> {
        square(self.body.pos, self.rules.view_radius)
            .filter(|p| self.map.in_bounds(*p) && want(self.map.get(*p)))
            .filter_map(|p| reach.steps_adjacent(p).map(|d| (p, d)))
            .min_by_key(|(_, d)| *d)
    }

    pub(crate) fn nearest_mob(&self, want: impl Fn(&Mob) -> bool, within: i32) -> Option<usize> {
        self.mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| want(m) && cheb(m.pos, self.body.pos) <= within)
            .min_by(|a, b| dist(a.1.pos, self.body.pos).total_cmp(&dist(b.1.pos, self.body.pos)))
            .map(|(i, _)| i)
    }
}

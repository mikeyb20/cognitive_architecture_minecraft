//! `SimWorld`: the seeded gridworld behind [`World`] and [`Host`] (spec §2, §21).
//!
//! One agent for now (#31 keeps multi-agent in the types). Everything random
//! goes through one ChaCha8 stream, so a seed replays exactly.

use std::collections::HashSet;

use host_api::{
    AgentId, Candidate, Host, Intensity, Inventory, Observation, Outcome, Pos, Scenario, Seen,
    SeenKind, ThingId, World,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::craft::{self};
use crate::geom::{cheb, dist, div_ceil, offset, square};
use crate::map::{Biome, Map, Tile};
use crate::mob::{Mob, MobKind, chase_steps};
use crate::rules::Rules;

/// Most candidates offered per decision (spec §6: K = 20).
pub use host_api::MAX_CANDIDATES;
/// Most block entries in one observation; entities are always listed.
pub const MAX_VISIBLE_BLOCKS: usize = 48;
pub use host_api::ontology::REGION;
/// Furthest a path search looks from the agent, in tiles.
pub const PATH_RADIUS: i32 = 20;

mod path;
mod shelter;
mod shock;
pub use path::Reach;
pub use shelter::ShelterTest;
pub use shock::Shock;

/// What killed the agent, as the world reports it. Outcome first (#43): the
/// cause behind the cause is worked out later from the log, never guessed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Death {
    Starved,
    Mob,
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

    fn walkable_free(&self, p: Pos) -> bool {
        self.map.get(p).mob_walkable()
    }

    /// Whether mob `i` is a hostile within `chase_radius` of the agent.
    fn chases(&self, i: usize) -> bool {
        let m = &self.mobs[i];
        m.kind.hostile() && cheb(m.pos, self.body.pos) <= self.rules.chase_radius
    }

    fn hostile_count(&self) -> usize {
        self.mobs.iter().filter(|m| m.kind.hostile()).count()
    }

    fn spawn_hostile(&mut self) {
        let (lo, hi) = (self.rules.spawn_min, self.rules.spawn_max);
        for _ in 0..10 {
            let dx = self.rng.random_range(-hi..=hi);
            let dz = self.rng.random_range(-hi..=hi);
            if dx.abs().max(dz.abs()) < lo {
                continue;
            }
            let p = offset(self.body.pos, dx, dz);
            if self.walkable_free(p) {
                self.mobs.push(Mob::new(MobKind::Zombie, p, self.tick));
                return;
            }
        }
    }

    /// A chaser (`reach` given: a hostile within `chase_radius`) follows a
    /// shortest walkable path to the agent (#62), or the greedy steps when
    /// no path exists; every other mob wanders.
    fn move_mob(&mut self, i: usize, reach: Option<&Reach>) {
        let m = &self.mobs[i];
        let me = self.body.pos;
        let steps: Vec<(i32, i32)> = if let Some(reach) = reach {
            if cheb(m.pos, me) <= 1 {
                return;
            }
            let greedy = chase_steps(m.pos, me);
            reach
                .step_toward_origin(m.pos, greedy)
                .map_or_else(|| greedy.to_vec(), |s| vec![s])
        } else {
            let d = [(1, 0), (-1, 0), (0, 1), (0, -1)][self.rng.random_range(0..4)];
            vec![d]
        };
        for (dx, dz) in steps {
            if dx == 0 && dz == 0 {
                continue;
            }
            let p = offset(self.mobs[i].pos, dx, dz);
            // By day, hostiles keep to the dark: they never step out of a cave.
            let sunlit = !self.is_night() && self.map.biome(p) != Biome::Cave;
            if self.walkable_free(p) && p != me && !(self.mobs[i].kind.hostile() && sunlit) {
                self.mobs[i].pos = p;
                return;
            }
        }
    }

    /// One world tick: body, shock, spawns, despawns, mobs.
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

impl World for SimWorld {
    fn reset(&mut self, seed: u64, scenario: &Scenario) {
        let _ = scenario;
        self.rng = ChaCha8Rng::seed_from_u64(seed);
        self.map = Map::generate(self.rules.map_size, self.rules.tree_chops, &mut self.rng);
        self.tick = 0;
        self.mobs.clear();
        self.damage_since_perceive = 0.0;
        self.action_damage = 0.0;
        self.body = Body {
            pos: self.map.start_pos(),
            health: 1.0,
            food: 1.0,
            inv: Inventory::default(),
            resting: false,
            death: None,
        };
        // Pigs on plains, by density.
        for z in 0..self.map.size {
            for x in 0..self.map.size {
                let p = Pos { x, y: 0, z };
                if self.map.biome(p) == Biome::Plains
                    && self.map.get(p) == Tile::Grass
                    && self.chance(self.rules.animal_density / 100.0)
                {
                    self.mobs.push(Mob::new(MobKind::Pig, p, 0));
                }
            }
        }
        let start = self.body.pos;
        self.start_at(start);
    }

    fn step_world(&mut self, ticks: u32) {
        for _ in 0..ticks {
            if !self.alive() {
                return;
            }
            self.tick_once();
        }
    }

    fn host(&mut self, agent: AgentId) -> &mut dyn Host {
        assert_eq!(agent, AgentId(0), "the sim runs one agent for now (#31)");
        self
    }
}

impl Host for SimWorld {
    fn perceive(&mut self) -> Observation {
        let r = self.rules.view_radius;
        let c = self.body.pos;
        let mut blocks: Vec<Seen> = square(c, r)
            .filter(|p| self.map.in_bounds(*p) && self.map.get(*p).notable())
            .map(|p| Seen {
                thing: ThingId(format!("block:{}", self.map.get(p).name())),
                kind: SeenKind::Block,
                pos: p,
                distance: dist(c, p),
            })
            .collect();
        blocks.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        blocks.truncate(MAX_VISIBLE_BLOCKS);
        let mut visible: Vec<Seen> = self
            .mobs
            .iter()
            .filter(|m| cheb(m.pos, c) <= r)
            .map(|m| Seen {
                thing: ThingId(format!("entity:{}", m.kind.name())),
                kind: SeenKind::Entity {
                    hostile: m.kind.hostile(),
                },
                pos: m.pos,
                distance: dist(c, m.pos),
            })
            .collect();
        visible.extend(blocks);
        visible.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        Observation {
            agent: AgentId(0),
            tick: self.tick,
            day_phase: self.day_phase(),
            health: self.body.health,
            food: self.body.food,
            position: c,
            inventory: self.body.inv.clone(),
            visible,
            chat: Vec::new(),
            damage_taken: std::mem::take(&mut self.damage_since_perceive),
            resting: self.body.resting,
            sheltered: self.sheltered(),
        }
    }

    fn candidate_actions(&mut self, _obs: &Observation) -> Vec<Candidate> {
        crate::skills::candidates(self)
    }

    fn execute(&mut self, a: &Candidate, it: Intensity) -> Outcome {
        self.action_damage = 0.0;
        self.body.resting = false;
        let start_tick = self.tick;
        let before = self.body.inv.clone();
        let (success, progress, blocked) = if !self.alive() {
            (false, 0.0, true)
        } else if it.effort <= 0.0 {
            // g(0) = 0: no effort, no action (01 §1.4). Below `min_effort`
            // the agent still acts where it stands (spec §16), see `walk_to`.
            self.step_world(1);
            (false, 0.0, true)
        } else {
            let r = crate::skills::execute(self, a, it);
            if self.tick == start_tick {
                // Every action takes time, so any loop of actions moves the
                // world on.
                self.step_world(if r.2 { self.rules.blocked_ticks } else { 1 });
            }
            r
        };
        let mut gained = Inventory::default();
        for (item, _) in &self.body.inv.items {
            let (had, now) = (before.count(item), self.body.inv.count(item));
            if now > had && gained.count(item) == 0 {
                gained.items.push((item.clone(), now - had));
            }
        }
        Outcome {
            success,
            progress,
            items_gained: gained,
            damage: self.action_damage,
            new_things: std::mem::take(&mut self.pending_new),
            blocked,
            duration_ticks: (self.tick - start_tick) as u32,
        }
    }
}

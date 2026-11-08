//! `SimWorld`: the seeded gridworld behind [`World`] and [`Host`] (spec §2, §21).
//!
//! One agent for now (#31 keeps multi-agent in the types). Everything random
//! goes through one ChaCha8 stream, so a seed replays exactly.

use std::collections::{HashSet, VecDeque};

use host_api::{
    AgentId, Candidate, Host, Intensity, Inventory, Observation, Outcome, Pos, Scenario, Seen,
    SeenKind, Skill, Target, TaskType, ThingId, World,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::craft::{self, RECIPES};
use crate::geom::{cheb, dist, div_ceil, neighbours4, offset, ring, square};
use crate::map::{Biome, Map, Tile};
use crate::mob::{Mob, MobKind, chase_steps};
use crate::rules::Rules;

/// Most candidates offered per decision (spec §6: K = 20).
pub const MAX_CANDIDATES: usize = 20;
/// Most block entries in one observation; entities are always listed.
pub const MAX_VISIBLE_BLOCKS: usize = 48;
pub use host_api::ontology::REGION;
/// Furthest a path search looks from the agent, in tiles.
pub const PATH_RADIUS: i32 = 20;

/// What killed the agent, as the world reports it. Outcome first (#43): the
/// cause behind the cause is worked out later from the log, never guessed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Death {
    Starved,
    Mob,
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

/// Path lengths from one origin (see [`SimWorld::reach`]).
#[derive(Debug, Clone)]
pub struct Reach {
    origin: Pos,
    side: i32,
    steps: Vec<u16>,
}

impl Reach {
    fn idx(&self, p: Pos) -> Option<usize> {
        let x = p.x - self.origin.x + PATH_RADIUS;
        let z = p.z - self.origin.z + PATH_RADIUS;
        if x < 0 || z < 0 || x >= self.side || z >= self.side {
            return None;
        }
        Some((z * self.side + x) as usize)
    }

    /// Steps to stand on `p`, if reachable.
    pub fn steps(&self, p: Pos) -> Option<u16> {
        self.idx(p)
            .map(|i| self.steps[i])
            .filter(|d| *d != u16::MAX)
    }

    /// Steps to stand on or next to `p` (king move), if reachable.
    pub fn steps_adjacent(&self, p: Pos) -> Option<u16> {
        square(p, 1).filter_map(|q| self.steps(q)).min()
    }
}

#[derive(Debug, Clone)]
pub struct SimWorld {
    pub rules: Rules,
    rng: ChaCha8Rng,
    pub map: Map,
    pub tick: u64,
    pub body: Body,
    pub mobs: Vec<Mob>,
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
    fn notice(&mut self, thing: &str) {
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
        let t = self.map.get(p);
        t.walkable() && t != Tile::Shelter
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

    fn move_mob(&mut self, i: usize) {
        let m = &self.mobs[i];
        let me = self.body.pos;
        let steps: Vec<(i32, i32)> =
            if m.kind.hostile() && cheb(m.pos, me) <= self.rules.chase_radius {
                if cheb(m.pos, me) <= 1 {
                    return;
                }
                chase_steps(m.pos, me).to_vec()
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

    /// One world tick: body, spawns, despawns, mobs.
    fn tick_once(&mut self) {
        self.tick += 1;
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
        for i in 0..self.mobs.len() {
            if self.tick >= self.mobs[i].next_move {
                self.move_mob(i);
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

    /// Path lengths from the agent to every walkable tile within
    /// `PATH_RADIUS` (4-way moves).
    pub fn reach(&self) -> Reach {
        let o = self.body.pos;
        let side = 2 * PATH_RADIUS + 1;
        let mut r = Reach {
            origin: o,
            side,
            steps: vec![u16::MAX; (side * side) as usize],
        };
        let start = r.idx(o).expect("origin is inside its own window");
        r.steps[start] = 0;
        let mut q = VecDeque::from([o]);
        while let Some(p) = q.pop_front() {
            let d = r.steps[r.idx(p).expect("queued tiles are inside")];
            for s in neighbours4(p) {
                let Some(i) = r.idx(s) else { continue };
                if r.steps[i] == u16::MAX && self.map.in_bounds(s) && self.map.get(s).walkable() {
                    r.steps[i] = d + 1;
                    q.push_back(s);
                }
            }
        }
        r
    }

    /// A reachable tile within 3 of `p` (nearest ring first), not the
    /// agent's own tile.
    fn reachable_near(&self, reach: &Reach, p: Pos) -> Option<Pos> {
        (0..=3)
            .flat_map(|r| ring(p, r))
            .find(|q| *q != self.body.pos && reach.steps(*q).is_some())
    }

    /// Shortest walkable path (4-way) from the agent to `goal`, or to a tile
    /// next to it (king move) when `adjacent`. Excludes the start tile.
    pub fn path_to(&self, goal: Pos, adjacent: bool) -> Option<Vec<Pos>> {
        let start = self.body.pos;
        let done = |p: Pos| {
            if adjacent {
                cheb(p, goal) <= 1
            } else {
                p == goal
            }
        };
        if done(start) {
            return Some(Vec::new());
        }
        let n = self.map.size;
        let mut parent = vec![-1i32; (n * n) as usize];
        let idx = |p: Pos| (p.z * n + p.x) as usize;
        let mut q = VecDeque::from([start]);
        parent[idx(start)] = idx(start) as i32;
        while let Some(p) = q.pop_front() {
            for s in neighbours4(p) {
                if !self.map.in_bounds(s)
                    || !self.map.get(s).walkable()
                    || parent[idx(s)] >= 0
                    || cheb(s, start) > PATH_RADIUS
                {
                    continue;
                }
                parent[idx(s)] = idx(p) as i32;
                if done(s) {
                    let mut path = vec![s];
                    let mut cur = idx(p);
                    while cur != idx(start) {
                        path.push(Pos {
                            x: cur as i32 % n,
                            y: 0,
                            z: cur as i32 / n,
                        });
                        cur = parent[cur] as usize;
                    }
                    path.reverse();
                    return Some(path);
                }
                q.push_back(s);
            }
        }
        None
    }

    fn tile_ticks(&self, effort: f32) -> u32 {
        if effort >= self.rules.sprint_effort {
            self.rules.sprint_ticks
        } else {
            self.rules.walk_ticks
        }
    }

    /// Walk to `goal` (or next to it). Returns whether it got there alive.
    fn walk_to(&mut self, goal: Pos, adjacent: bool, effort: f32) -> bool {
        let Some(path) = self.path_to(goal, adjacent) else {
            return false;
        };
        let per = self.tile_ticks(effort);
        for p in path {
            self.enter(p);
            self.step_world(per);
            if !self.alive() {
                return false;
            }
        }
        true
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

    fn roll(&mut self, focus: f32) -> bool {
        let p = self.success_chance(focus);
        self.chance(p)
    }

    fn give(&mut self, item: &str, n: u32) {
        craft::give(&mut self.body.inv, item, n);
        self.notice(&format!("item:{item}"));
    }

    /// Risk estimate for a candidate around `p`, 0..1.
    fn risk_at(&self, p: Pos) -> f32 {
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
    fn cost(&self, steps: u16, base: f32) -> f32 {
        (base + f32::from(steps) / (4 * self.rules.max_move) as f32).min(1.0)
    }

    /// The in-view tile matching `want` that is fewest steps away, counting
    /// steps to stand next to it.
    fn nearest_tile(&self, reach: &Reach, want: impl Fn(Tile) -> bool) -> Option<(Pos, u16)> {
        square(self.body.pos, self.rules.view_radius)
            .filter(|p| self.map.in_bounds(*p) && want(self.map.get(*p)))
            .filter_map(|p| reach.steps_adjacent(p).map(|d| (p, d)))
            .min_by_key(|(_, d)| *d)
    }

    fn nearest_mob(&self, want: impl Fn(&Mob) -> bool, within: i32) -> Option<usize> {
        self.mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| want(m) && cheb(m.pos, self.body.pos) <= within)
            .min_by(|a, b| dist(a.1.pos, self.body.pos).total_cmp(&dist(b.1.pos, self.body.pos)))
            .map(|(i, _)| i)
    }

    /// The point `max_move` tiles from the agent in direction (dx, dz),
    /// clamped to the map and snapped to a walkable tile.
    fn point_toward(&self, reach: &Reach, dx: i32, dz: i32) -> Option<Pos> {
        let m = self.rules.max_move;
        let last = self.map.size - 1;
        let p = Pos {
            x: (self.body.pos.x + dx * m).clamp(0, last),
            y: 0,
            z: (self.body.pos.z + dz * m).clamp(0, last),
        };
        self.reachable_near(reach, p)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "debt D2: one arm per skill; split into per-skill handlers before step 6"
    )]
    fn exec_inner(&mut self, a: &Candidate, it: Intensity) -> (bool, f32, bool) {
        let target_pos = match &a.target {
            Some(Target::Pos(p)) => Some(*p),
            _ => None,
        };
        let target_item = match &a.target {
            Some(Target::Item(s)) => Some(s.clone()),
            _ => None,
        };
        match a.skill {
            Skill::Explore | Skill::MoveTo | Skill::Flee => {
                let Some(goal) = target_pos else {
                    return (false, 0.0, true);
                };
                // Fleeing is always at full speed.
                let effort = if a.skill == Skill::Flee {
                    1.0
                } else {
                    it.effort
                };
                let ok = self.walk_to(goal, false, effort);
                (ok, if ok { 1.0 } else { 0.0 }, !ok && self.alive())
            }
            Skill::Chop | Skill::Mine | Skill::Farm => {
                let Some(goal) = target_pos else {
                    return (false, 0.0, true);
                };
                let tile = self.map.get(goal);
                let ticks = match (a.skill, tile) {
                    (Skill::Chop, Tile::Tree { .. }) => self.rules.chop_ticks,
                    (Skill::Farm, Tile::Wheat) => self.rules.farm_ticks,
                    (Skill::Mine, Tile::Stone | Tile::IronOre) => {
                        let tier = craft::pick_tier(&self.body.inv);
                        let need = if tile == Tile::IronOre { 2 } else { 1 };
                        if tier < need {
                            return (false, 0.0, true);
                        }
                        self.rules.mine_ticks[(tier - 1) as usize]
                    }
                    _ => return (false, 0.0, true),
                };
                if !self.walk_to(goal, true, it.effort) {
                    return (false, 0.0, self.alive());
                }
                self.step_world(ticks);
                if !self.alive() || !self.roll(it.focus) {
                    return (false, 0.5, false);
                }
                match tile {
                    Tile::Tree { chops_left } => {
                        self.give("oak_log", 1);
                        let next = if chops_left > 1 {
                            Tile::Tree {
                                chops_left: chops_left - 1,
                            }
                        } else {
                            self.map.ground(goal)
                        };
                        self.map.set(goal, next);
                    }
                    Tile::Wheat => {
                        self.give("wheat", 1);
                        self.map.set(goal, Tile::Grass);
                    }
                    Tile::Stone => {
                        self.give("cobblestone", 1);
                        self.map.set(goal, self.map.ground(goal));
                    }
                    _ => {
                        self.give("iron_ore", 1);
                        self.map.set(goal, self.map.ground(goal));
                    }
                }
                (true, 1.0, false)
            }
            Skill::Attack => {
                let Some(goal) = target_pos else {
                    return (false, 0.0, true);
                };
                let Some(i) = self.mobs.iter().position(|m| cheb(m.pos, goal) <= 1) else {
                    return (false, 0.0, true);
                };
                let kind = self.mobs[i].kind;
                let mut idx = i;
                for _ in 0..self.rules.max_swings {
                    let at = self.mobs[idx].pos;
                    if !self.walk_to(at, true, it.effort) {
                        return (false, 0.0, self.alive());
                    }
                    self.step_world(self.rules.swing_ticks);
                    if !self.alive() {
                        return (false, 0.0, false);
                    }
                    // The mob list may have changed (despawns); find it again.
                    let Some(j) = self
                        .mobs
                        .iter()
                        .position(|m| m.kind == kind && cheb(m.pos, at) <= 1)
                    else {
                        return (false, 0.0, false);
                    };
                    idx = j;
                    if self.roll(it.focus) {
                        self.mobs[idx].health -= self.rules.swing_damage;
                        if self.mobs[idx].health <= 0.0 {
                            self.mobs.remove(idx);
                            if kind == MobKind::Pig {
                                let n = self.rng.random_range(1..=2);
                                self.give("porkchop", n);
                            }
                            return (true, 1.0, false);
                        }
                    }
                }
                (false, 0.5, false)
            }
            Skill::Eat => {
                let Some(item) = target_item else {
                    return (false, 0.0, true);
                };
                let Some(v) = food_value(&self.rules, &item) else {
                    return (false, 0.0, true);
                };
                if !craft::take(&mut self.body.inv, &item, 1) {
                    return (false, 0.0, true);
                }
                self.step_world(self.rules.eat_ticks);
                self.body.food = (self.body.food + v).min(1.0);
                (true, 1.0, false)
            }
            Skill::Craft => {
                let Some(r) = target_item.as_deref().and_then(craft::recipe) else {
                    return (false, 0.0, true);
                };
                if !craft::craft(&mut self.body.inv, r) {
                    return (false, 0.0, true);
                }
                self.step_world(self.rules.craft_ticks);
                self.notice(&format!("recipe:{}", r.output));
                self.notice(&format!("item:{}", r.output));
                (true, 1.0, false)
            }
            Skill::Shelter => {
                let here = self.body.pos;
                let blocks = ["cobblestone", "oak_planks"]
                    .into_iter()
                    .find(|b| self.body.inv.count(b) >= 4);
                let ticks = match blocks {
                    Some(b) => {
                        craft::take(&mut self.body.inv, b, 4);
                        self.rules.shelter_ticks_blocks
                    }
                    None => self.rules.shelter_ticks_dig,
                };
                self.step_world(ticks);
                if !self.alive() {
                    return (false, 0.0, false);
                }
                self.map.set(here, Tile::Shelter);
                self.notice("block:shelter");
                (true, 1.0, false)
            }
            Skill::Rest => {
                if !self.sheltered() {
                    return (false, 0.0, true);
                }
                self.body.resting = true;
                let d = self.rules.day_ticks;
                let to_dawn = (d - self.tick % d) as u32;
                let ticks = if self.is_night() {
                    to_dawn.min(self.rules.rest_ticks)
                } else {
                    self.rules.rest_ticks
                };
                self.step_world(ticks);
                (true, 1.0, false)
            }
            Skill::Place | Skill::FollowPlayer | Skill::Chat => (false, 0.0, true),
        }
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

    #[expect(
        clippy::too_many_lines,
        reason = "debt D2: one block per skill; split into per-skill handlers before step 6"
    )]
    fn candidate_actions(&mut self, _obs: &Observation) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = Vec::new();
        let mut add =
            |skill: Skill, target: Option<Target>, task: TaskType, cost: f32, risk: f32| {
                out.push(Candidate {
                    id: out.len() as u32,
                    skill,
                    target,
                    task_type: task,
                    est_cost: cost.clamp(0.0, 1.0),
                    est_risk: risk.clamp(0.0, 1.0),
                    commanded: false,
                });
            };
        let me = self.body.pos;
        let sheltered = self.sheltered();
        let reach = self.reach();
        // Threats.
        if let Some(i) = self.nearest_mob(|m| m.kind.hostile(), 6) {
            let h = self.mobs[i].pos;
            let away = ((me.x - h.x).signum(), (me.z - h.z).signum());
            let away = if away == (0, 0) { (1, 0) } else { away };
            if let Some(p) = self.point_toward(&reach, away.0, away.1) {
                let steps = reach.steps(p).unwrap_or(0);
                add(
                    Skill::Flee,
                    Some(Target::Pos(p)),
                    TaskType::Travel,
                    self.cost(steps, 0.1),
                    0.2,
                );
            }
            if let Some(steps) = reach.steps_adjacent(h) {
                add(
                    Skill::Attack,
                    Some(Target::Pos(h)),
                    TaskType::Combat,
                    self.cost(steps, 0.2),
                    0.6,
                );
            }
        }
        // Food in hand.
        if self.body.food < 1.0 {
            let best = ["bread", "porkchop"]
                .into_iter()
                .find(|f| self.body.inv.count(f) > 0);
            if let Some(f) = best {
                add(
                    Skill::Eat,
                    Some(Target::Item(f.into())),
                    TaskType::Farming,
                    0.05,
                    0.0,
                );
            }
        }
        // Shelter and rest.
        if sheltered {
            add(Skill::Rest, None, TaskType::Building, 0.1, 0.0);
        } else {
            let has_blocks = ["cobblestone", "oak_planks"]
                .iter()
                .any(|b| self.body.inv.count(b) >= 4);
            let cost = if has_blocks { 0.1 } else { 0.25 };
            add(
                Skill::Shelter,
                None,
                TaskType::Building,
                cost,
                self.risk_at(me),
            );
            let shelter = self
                .nearest_tile(&reach, |t| t == Tile::Shelter)
                .and_then(|(p, _)| reach.steps(p).map(|d| (p, d)));
            if let Some((p, d)) = shelter {
                add(
                    Skill::MoveTo,
                    Some(Target::Pos(p)),
                    TaskType::Travel,
                    self.cost(d, 0.0),
                    self.risk_at(p),
                );
            }
        }
        // Resources in view.
        let tier = craft::pick_tier(&self.body.inv);
        // (skill, task type, tile kind, pickaxe tier needed)
        let resources = [
            (
                Skill::Chop,
                TaskType::Mining,
                Tile::Tree { chops_left: 0 },
                0,
            ),
            (Skill::Mine, TaskType::Mining, Tile::Stone, 1),
            (Skill::Mine, TaskType::Mining, Tile::IronOre, 2),
            (Skill::Farm, TaskType::Farming, Tile::Wheat, 0),
        ];
        for (skill, task, kind, need) in resources {
            if tier < need {
                continue;
            }
            let want = |t: Tile| std::mem::discriminant(&t) == std::mem::discriminant(&kind);
            if let Some((p, d)) = self.nearest_tile(&reach, want) {
                add(
                    skill,
                    Some(Target::Pos(p)),
                    task,
                    self.cost(d, 0.1),
                    self.risk_at(p),
                );
            }
        }
        let pig = self
            .mobs
            .iter()
            .filter(|m| m.kind == MobKind::Pig && cheb(m.pos, me) <= self.rules.view_radius)
            .filter_map(|m| reach.steps_adjacent(m.pos).map(|d| (d, m.pos)))
            .min_by_key(|(d, _)| *d);
        if let Some((d, p)) = pig {
            add(
                Skill::Attack,
                Some(Target::Pos(p)),
                TaskType::Farming,
                self.cost(d, 0.15),
                self.risk_at(p),
            );
        }
        // Crafting: skip stations already owned and pickaxes no better than held.
        for r in RECIPES {
            let owned_station = matches!(r.output, "crafting_table" | "furnace")
                && self.body.inv.count(r.output) > 0;
            let worse_pick = match r.output {
                "wooden_pickaxe" => tier >= 1,
                "stone_pickaxe" => tier >= 2,
                "iron_pickaxe" => tier >= 3,
                _ => false,
            };
            if !owned_station && !worse_pick && craft::can_craft(&self.body.inv, r) {
                add(
                    Skill::Craft,
                    Some(Target::Item(r.output.into())),
                    TaskType::Crafting,
                    0.05,
                    0.0,
                );
            }
        }
        // Explore: four directions, least-visited region first.
        let mut dirs: Vec<(u32, Pos)> = [(0, -1), (1, 0), (0, 1), (-1, 0)]
            .into_iter()
            .filter_map(|(dx, dz)| self.point_toward(&reach, dx, dz))
            .map(|p| (self.region_visits(p), p))
            .collect();
        dirs.sort_by_key(|(v, _)| *v);
        dirs.dedup_by_key(|(_, p)| *p);
        for (_, p) in dirs {
            let steps = reach.steps(p).unwrap_or(0);
            add(
                Skill::Explore,
                Some(Target::Pos(p)),
                TaskType::Travel,
                self.cost(steps, 0.0),
                self.risk_at(p),
            );
        }
        // The sim can offer at most 19 (see the `fullest` test), within K.
        debug_assert!(out.len() <= MAX_CANDIDATES);
        out
    }

    fn execute(&mut self, a: &Candidate, it: Intensity) -> Outcome {
        self.action_damage = 0.0;
        self.body.resting = false;
        let start_tick = self.tick;
        let before = self.body.inv.clone();
        let (success, progress, blocked) = if !self.alive() {
            (false, 0.0, true)
        } else if it.effort < self.rules.min_effort {
            // g(0) = 0: no self-generated movement (01 §1.4).
            self.step_world(1);
            (false, 0.0, true)
        } else {
            let r = self.exec_inner(a, it);
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

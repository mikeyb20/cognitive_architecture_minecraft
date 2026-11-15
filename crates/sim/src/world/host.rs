//! `SimWorld` as a [`World`] and a [`Host`] (spec §2): reset, stepping,
//! perception, candidates and execution.

use host_api::{
    AgentId, Candidate, Host, Intensity, Inventory, Observation, Outcome, Pos, Scenario, Seen,
    SeenKind, ThingId, World,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::{Body, MAX_VISIBLE_BLOCKS, SimWorld};
use crate::geom::{cheb, dist, square};
use crate::map::{Biome, Map, Tile};
use crate::mob::{Mob, MobKind};

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
        self.interrupted = false;
        self.calm_start = !self.in_danger();
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
            let mut r = crate::skills::execute(self, a, it);
            // Cut short is not blocked: the action could have run.
            r.2 &= !self.interrupted;
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

//! Arm A: a behaviour tree (spec §24). Priority order: survive → plan step →
//! nearest resource → explore. It uses the same candidates and code tools as
//! every other arm; it just never learns.

use host_api::{Candidate, Observation, Pos, SeenKind, Skill, Target, TaskType};
use sim::planner::{self, Resource, Step};

use crate::Policy;

/// Arm A's fixed thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmAConfig {
    /// Goal item for the plan branch.
    pub goal: String,
    /// Day phase at which it heads for shelter (night starts at 0.5).
    pub dusk: f32,
    /// A hostile this close (tiles) is dealt with first.
    pub threat_range: f32,
    /// Fight a close hostile above this health, flee below.
    pub fight_health: f32,
    /// Flee instead of fighting when more than one hostile is this close.
    pub crowd_range: f32,
    /// Eat below this food level when food is in hand.
    pub eat_below: f32,
    /// Look for food below this level when none is in hand.
    pub hunt_below: f32,
}

impl Default for ArmAConfig {
    fn default() -> Self {
        Self {
            goal: "iron_pickaxe".into(),
            dusk: 0.42,
            threat_range: 2.5,
            fight_health: 0.5,
            crowd_range: 4.0,
            eat_below: 0.6,
            hunt_below: 0.7,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ArmA {
    pub config: ArmAConfig,
}

impl ArmA {
    pub fn new(config: ArmAConfig) -> Self {
        Self { config }
    }
}

fn find(c: &[Candidate], pred: impl Fn(&Candidate) -> bool) -> Option<usize> {
    c.iter().position(pred)
}

fn target_pos(c: &Candidate) -> Option<Pos> {
    match c.target {
        Some(Target::Pos(p)) => Some(p),
        _ => None,
    }
}

/// The block name the observation shows at a position.
fn block_at(obs: &Observation, p: Pos) -> Option<&str> {
    obs.visible
        .iter()
        .find(|s| s.kind == SeenKind::Block && s.pos == p)
        .map(|s| s.thing.0.as_str())
}

fn is_craft(c: &Candidate, item: &str) -> bool {
    c.skill == Skill::Craft && c.target == Some(Target::Item(item.into()))
}

fn hostiles_within(obs: &Observation, range: f32) -> usize {
    obs.visible
        .iter()
        .filter(|s| s.kind == SeenKind::Entity { hostile: true } && s.distance <= range)
        .count()
}

fn has_food(obs: &Observation) -> bool {
    obs.inventory.count("bread") + obs.inventory.count("porkchop") > 0
}

impl ArmA {
    fn survive(&self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        let cfg = &self.config;
        if !obs.sheltered && obs.nearest_hostile().is_some_and(|d| d <= cfg.threat_range) {
            let fight = obs.health > cfg.fight_health && hostiles_within(obs, cfg.crowd_range) <= 1;
            let pick = if fight {
                find(c, |x| {
                    x.skill == Skill::Attack && x.task_type == TaskType::Combat
                })
            } else {
                find(c, |x| x.skill == Skill::Flee)
            };
            if pick.is_some() {
                return pick;
            }
        }
        if obs.food < cfg.eat_below {
            if let Some(i) = find(c, |x| x.skill == Skill::Eat) {
                return Some(i);
            }
        }
        if obs.day_phase >= cfg.dusk {
            return if obs.sheltered {
                find(c, |x| x.skill == Skill::Rest)
            } else {
                find(c, |x| x.skill == Skill::Shelter)
            };
        }
        if obs.food < cfg.hunt_below && !has_food(obs) {
            return find(c, |x| is_craft(x, "bread"))
                .or_else(|| {
                    find(c, |x| {
                        x.skill == Skill::Attack && x.task_type == TaskType::Farming
                    })
                })
                .or_else(|| find(c, |x| x.skill == Skill::Farm))
                .or_else(|| find(c, |x| x.skill == Skill::Explore));
        }
        None
    }

    fn plan_step(&self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        match planner::next_step(&obs.inventory, &self.config.goal)? {
            Step::Craft(item) => find(c, |x| is_craft(x, item)),
            Step::Gather(res) => {
                let (skill, block) = match res {
                    Resource::Log => (Skill::Chop, "block:oak_log"),
                    Resource::Stone => (Skill::Mine, "block:stone"),
                    Resource::IronOre => (Skill::Mine, "block:iron_ore"),
                    Resource::Wheat => (Skill::Farm, "block:wheat"),
                };
                find(c, |x| {
                    x.skill == skill && target_pos(x).and_then(|p| block_at(obs, p)) == Some(block)
                })
            }
        }
    }
}

impl Policy for ArmA {
    fn choose(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        self.survive(obs, c)
            .or_else(|| self.plan_step(obs, c))
            .or_else(|| {
                c.iter()
                    .enumerate()
                    .filter(|(_, x)| matches!(x.skill, Skill::Chop | Skill::Mine | Skill::Farm))
                    .min_by(|a, b| a.1.est_cost.total_cmp(&b.1.est_cost))
                    .map(|(i, _)| i)
            })
            .or_else(|| find(c, |x| x.skill == Skill::Explore))
            .or(if c.is_empty() { None } else { Some(0) })
    }
}

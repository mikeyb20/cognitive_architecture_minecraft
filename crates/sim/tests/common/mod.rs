//! Helpers shared by the sim world and skill tests: a flat hand-built map
//! and `base_success = 1`, so every number is exact.

#![allow(dead_code, reason = "each test file uses a subset")]

use host_api::{Candidate, Intensity, Pos, Skill, Target, TaskType};
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};
use sim::{Rules, SimWorld};

pub const FULL: Intensity = Intensity {
    effort: 1.0,
    focus: 1.0,
    external_drive: false,
};

pub fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

/// Exact rules: no random failure, no night spawns unless a test turns them on.
pub fn exact() -> Rules {
    Rules {
        base_success: 1.0,
        night_spawn: 0.0,
        cave_spawn: 0.0,
        regen: 0.0,
        threat_penalty: 1.0,
        ..Rules::default()
    }
}

/// A 48×48 grass plain with the agent at (24, 24).
pub fn flat_with(rules: Rules) -> SimWorld {
    SimWorld::with_map(
        rules,
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(24, 24),
    )
}

pub fn flat() -> SimWorld {
    flat_with(exact())
}

pub fn act(skill: Skill, target: Option<Target>) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target,
        task_type: TaskType::Travel,
        est_cost: 0.0,
        est_risk: 0.0,
        commanded: false,
    }
}

pub fn at(skill: Skill, x: i32, z: i32) -> Candidate {
    act(skill, Some(Target::Pos(p(x, z))))
}

pub fn item(skill: Skill, name: &str) -> Candidate {
    act(skill, Some(Target::Item(name.into())))
}

pub fn give(w: &mut SimWorld, name: &str, n: u32) {
    sim::craft::give(&mut w.body.inv, name, n);
}

pub fn zombie(w: &mut SimWorld, x: i32, z: i32) {
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Zombie, p(x, z), t));
}

pub fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

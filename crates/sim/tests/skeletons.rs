//! Skeletons (`world::ranged`): shoot from range with a clear line of
//! sight, hold while they can shoot, back off when the agent comes close,
//! and make fleeing mean getting out of sight. Flat test map, exact rules.

mod common;

use common::*;
use host_api::{Host, Skill, Target, World};
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};
use sim::world::MobTest;
use sim::{Death, Rules, SimWorld};

const NIGHT: u64 = 1200;

/// Arrows always hit, so damage is exact.
fn sure() -> Rules {
    Rules {
        arrow_hit: 1.0,
        ..exact()
    }
}

fn skeleton(w: &mut SimWorld, x: i32, z: i32) {
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Skeleton, p(x, z), t));
}

fn night(rules: Rules) -> SimWorld {
    let mut w = flat_with(rules);
    w.tick = NIGHT;
    w
}

// In sight at 10 tiles: a shot on the first tick, then one every 40.
#[test]
fn a_skeleton_in_sight_shoots_every_arrow_interval() {
    let mut w = night(sure());
    skeleton(&mut w, 34, 24);
    w.step_world(81);
    assert!(
        close(w.body.health, 0.7),
        "shots at 1, 41, 81: {}",
        w.body.health
    );
    assert_eq!(w.mobs[0].pos, p(34, 24), "held its ground");
}

// Beyond 15 tiles it walks in until it can shoot, then stops.
#[test]
fn a_far_skeleton_closes_to_range_then_holds() {
    let mut w = night(sure());
    skeleton(&mut w, 24 + 16, 24);
    w.step_world(30);
    assert_eq!(w.mobs[0].pos, p(39, 24), "one step in to 15 tiles");
    assert!(w.body.health < 1.0);
}

// A stone wall between them blocks the shot; the skeleton has to move.
#[test]
fn a_wall_blocks_the_line_of_sight() {
    let mut w = night(sure());
    for z in 20..=28 {
        w.map.set(p(28, z), Tile::Stone);
    }
    skeleton(&mut w, 33, 24);
    w.mobs[0].next_move = u64::MAX;
    assert!(!w.in_danger(), "unseen and beyond the melee threat range");
    w.step_world(100);
    assert_eq!(w.body.health, 1.0);
}

// In sight but far: in danger already (the threat range is 8).
#[test]
fn a_skeleton_that_can_shoot_is_danger_from_afar() {
    let mut w = night(sure());
    skeleton(&mut w, 36, 24);
    assert!(w.in_danger());
    let obs = w.perceive();
    let offered: Vec<Skill> = w.candidate_actions(&obs).iter().map(|c| c.skill).collect();
    assert!(offered.contains(&Skill::Flee), "{offered:?}");
    w.map.set(p(30, 24), Tile::Tree { chops_left: 3 });
    assert!(!w.in_danger(), "a trunk in the way");
}

// Inside a hut the agent is not shot.
#[test]
fn sheltered_agents_are_not_shot() {
    let mut w = night(sure());
    w.map.set(p(24, 24), Tile::Shelter);
    skeleton(&mut w, 30, 24);
    w.step_world(100);
    assert_eq!(w.body.health, 1.0);
}

// Nearer than 4 tiles it backs off, one tile per move, to 4.
#[test]
fn a_skeleton_backs_off_when_the_agent_comes_close() {
    let mut w = night(Rules {
        arrow_hit: 0.0,
        ..exact()
    });
    skeleton(&mut w, 26, 24);
    w.step_world(13);
    assert_eq!(w.mobs[0].pos, p(28, 24), "moves at 1 and 7, then holds");
    w.step_world(30);
    assert_eq!(w.mobs[0].pos, p(28, 24));
}

// It never hits from the next tile: a skeleton next to the agent shoots
// on its arrow interval and steps away, it does not melee every 20 ticks.
#[test]
fn skeletons_do_not_melee() {
    let mut w = night(sure());
    skeleton(&mut w, 25, 24);
    w.mobs[0].next_move = u64::MAX;
    w.step_world(40);
    assert!(close(w.body.health, 0.9), "one arrow: {}", w.body.health);
}

// Killed by arrows, the cause says so.
#[test]
fn arrow_deaths_are_their_own_cause() {
    let mut w = night(sure());
    w.body.health = 0.05;
    skeleton(&mut w, 30, 24);
    w.step_world(2);
    assert_eq!(w.body.death, Some(Death::Arrow));
}

// Fleeing a skeleton 4 tiles away: open ground within 8 steps is all
// still in its range, so with a wall nearby the flee goes behind it, out of
// sight; with no wall it just runs (and is still shot).
#[test]
fn fleeing_a_skeleton_heads_out_of_its_sight() {
    let shot_at = |w: &SimWorld, to| {
        let mut there = w.clone();
        there.body.pos = to;
        there.step_world(1);
        there.body.health < 1.0
    };
    let flee_to = |w: &mut SimWorld| {
        let obs = w.perceive();
        let c = w
            .candidate_actions(&obs)
            .into_iter()
            .find(|c| c.skill == Skill::Flee);
        match c.and_then(|c| c.target) {
            Some(Target::Pos(to)) => to,
            other => panic!("{other:?}"),
        }
    };
    let mut w = night(sure());
    for x in 22..=28 {
        w.map.set(p(x, 22), Tile::Stone);
    }
    skeleton(&mut w, 20, 24);
    w.mobs[0].next_move = u64::MAX;
    let to = flee_to(&mut w);
    assert!(!shot_at(&w, to), "behind the wall at {to:?}");
    let mut open = night(sure());
    skeleton(&mut open, 20, 24);
    open.mobs[0].next_move = u64::MAX;
    let to = flee_to(&mut open);
    assert!(shot_at(&open, to), "in the open at {to:?}");
}

// The spawn mix: all skeletons at share 1, all zombies at share 0.
#[test]
fn the_skeleton_share_sets_the_spawn_mix() {
    for (share, kind) in [(1.0, MobKind::Skeleton), (0.0, MobKind::Zombie)] {
        let mut w = flat_with(Rules {
            night_spawn: 1.0,
            max_hostiles: 50,
            skeleton_share: share,
            ..exact()
        });
        w.tick = NIGHT;
        w.step_world(20);
        assert!(!w.mobs.is_empty());
        assert!(w.mobs.iter().all(|m| m.kind == kind), "share {share}");
    }
    let mut w = flat_with(Rules {
        night_spawn: 1.0,
        max_hostiles: 200,
        skeleton_share: 0.5,
        ..exact()
    });
    w.tick = NIGHT;
    w.step_world(200);
    let sk = w
        .mobs
        .iter()
        .filter(|m| m.kind == MobKind::Skeleton)
        .count();
    assert!(
        sk > 0 && sk < w.mobs.len(),
        "a mix: {sk} of {}",
        w.mobs.len()
    );
}

// Beyond its follow range (16) a skeleton does not come for the agent: in
// 60 ticks a chaser would have walked straight in along z = 24 to 15 tiles
// and stopped there; a wanderer steps off that line or away.
#[test]
fn a_skeleton_follows_only_within_16_tiles() {
    let mut w = night(Rules {
        arrow_hit: 0.0,
        ..exact()
    });
    skeleton(&mut w, 24 + 17, 24);
    let mut strayed = false;
    for _ in 0..60 {
        let was = w.mobs[0].pos;
        w.step_world(1);
        let now = w.mobs[0].pos;
        strayed |= now.z != 24 || now.x > was.x;
    }
    assert!(strayed, "it wandered: {:?}", w.mobs[0].pos);
    let mut near = night(Rules {
        arrow_hit: 0.0,
        ..exact()
    });
    skeleton(&mut near, 24 + 16, 24);
    near.step_world(60);
    assert_eq!(near.mobs[0].pos, p(39, 24));
}

// With no skeletons in the mix no extra random draw is made, so zombie
// spawns land where they did before skeletons existed (pinned).
#[test]
fn a_zero_share_spawns_as_before() {
    let mut w = SimWorld::with_map(
        Rules {
            night_spawn: 1.0,
            mob_move_ticks: 1_000_000,
            ..exact()
        },
        1,
        Map::filled(96, Tile::Grass, Biome::Plains),
        p(48, 48),
    );
    w.tick = NIGHT;
    w.step_world(3);
    let at: Vec<(i32, i32)> = w.mobs.iter().map(|m| (m.pos.x, m.pos.z)).collect();
    assert_eq!(at, PINNED);
}

// Spawn spots from `main` before skeletons (checked on 424b88d), re-pinned
// when spawns got their own random stream (07 M4).
const PINNED: [(i32, i32); 3] = [(77, 80), (53, 17), (85, 80)];

// The mob lessons' tests (01 §4.11): a zombie within the threat range, a
// skeleton that can shoot; both false in a hut.
#[test]
fn the_mob_tests_see_what_threatens() {
    let mut w = night(sure());
    assert_eq!(w.mob_test(), MobTest::default());
    zombie(&mut w, 32, 24);
    assert!(w.mob_test().zombie_near && !w.mob_test().shot_at);
    w.mobs[0].pos = p(33, 24);
    assert!(!w.mob_test().zombie_near, "9 tiles: out of range");
    skeleton(&mut w, 24, 36);
    assert!(w.mob_test().shot_at);
    assert!(!w.mob_test().weak);
    w.body.health = 0.49;
    assert!(w.mob_test().weak, "below fight_health 0.5");
    w.body.health = 0.5;
    assert!(!w.mob_test().weak, "at fight_health: strong enough");
    w.body.health = 0.2;
    w.map.set(p(24, 24), Tile::Shelter);
    assert_eq!(w.mob_test(), MobTest::default());
}

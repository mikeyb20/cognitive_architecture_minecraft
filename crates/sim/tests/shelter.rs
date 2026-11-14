//! The shelter test (01 §4.11, #58; spec §27 step 12) on hand-built maps:
//! open ground fails, a sealed pit fails, walls with a door pass.

mod common;

use common::*;
use host_api::{Host, Pos, Skill, World};
use sim::SimWorld;
use sim::map::Tile;
use sim::mob::{Mob, MobKind};
use sim::world::ShelterTest;

/// A square ring of stone at Chebyshev distance `r` around the agent at
/// (24, 24), with a door at `door` if given.
fn room(r: i32, door: Option<Pos>) -> SimWorld {
    let mut w = flat();
    for x in 24 - r..=24 + r {
        for z in 24 - r..=24 + r {
            if (x - 24).abs().max((z - 24).abs()) == r {
                w.map.set(p(x, z), Tile::Stone);
            }
        }
    }
    if let Some(d) = door {
        w.map.set(d, Tile::Door);
    }
    w
}

fn test(enclosed: bool, exit: bool) -> ShelterTest {
    ShelterTest { enclosed, exit }
}

#[test]
fn open_ground_fails() {
    let w = flat();
    assert_eq!(w.shelter_test(), test(false, true));
    assert!(!w.shelter_test().passes());
}

#[test]
fn a_sealed_pit_has_no_way_out() {
    let w = room(2, None);
    assert_eq!(w.shelter_test(), test(true, false));
    assert!(!w.shelter_test().passes());
}

#[test]
fn walls_with_a_door_pass() {
    let w = room(2, Some(p(26, 24)));
    assert_eq!(w.shelter_test(), test(true, true));
    assert!(w.shelter_test().passes());
    // A gap instead of a door lets mobs in.
    let mut gap = room(2, None);
    gap.map.set(p(26, 24), Tile::Grass);
    assert_eq!(gap.shelter_test(), test(false, true));
}

#[test]
fn a_door_into_a_closed_pocket_is_no_way_out() {
    let mut w = room(2, Some(p(26, 24)));
    for q in [p(27, 23), p(28, 24), p(27, 25)] {
        w.map.set(q, Tile::Stone);
    }
    assert_eq!(w.shelter_test(), test(true, false));
}

#[test]
fn a_room_must_close_within_the_shelter_radius() {
    // The test looks out to shelter_radius (6): a room whose floor reaches
    // 6 is not closed within it.
    let near = exact().shelter_radius;
    assert_eq!(near, 6);
    let door = |r| Some(p(24 + r, 24));
    assert!(room(near, door(near)).shelter_test().passes(), "floor to 5");
    assert_eq!(
        room(near + 1, door(near + 1)).shelter_test(),
        test(false, true),
        "floor to 6"
    );
}

#[test]
fn a_hostile_inside_fails_and_a_pig_does_not() {
    let mut w = room(3, Some(p(27, 24)));
    w.mobs.push(Mob::new(MobKind::Pig, p(22, 22), 0));
    assert!(w.shelter_test().passes(), "a pig is no threat");
    w.mobs.push(Mob::new(MobKind::Zombie, p(26, 26), 0));
    assert_eq!(w.shelter_test(), test(false, true));
}

#[test]
fn the_test_runs_where_the_agent_stands() {
    // The room is at (24, 24); out on the plain at (30, 30) the test fails.
    let w = room(2, Some(p(26, 24)));
    let outside = SimWorld::with_map(exact(), 1, w.map.clone(), p(30, 30));
    assert_eq!(outside.shelter_test(), test(false, true));
}

#[test]
fn zombies_do_not_open_doors() {
    // At night a zombie outside the door waits there; it never gets in.
    let mut w = room(2, Some(p(26, 24)));
    w.tick = 1200;
    w.mobs.push(Mob::new(MobKind::Zombie, p(29, 24), 1200));
    w.step_world(600);
    let m = w.mobs[0].pos;
    assert!(m.x >= 27, "stays outside: {m:?}");
    assert_eq!(w.mobs[0].pos, p(27, 24), "waits at the door");
    assert!(w.shelter_test().passes(), "the waiting zombie is outside");
}

#[test]
fn a_hut_built_with_blocks_passes_and_a_dug_in_one_does_not() {
    // Step 13: blocks close every open side, a door on the first (east).
    let mut w = flat();
    give(&mut w, "cobblestone", 4);
    assert!(w.execute(&act(Skill::Shelter, None), FULL).success);
    assert_eq!(w.map.get(p(25, 24)), Tile::Door);
    for q in [p(23, 24), p(24, 25), p(24, 23)] {
        assert_eq!(w.map.get(q), Tile::Stone, "{q:?}");
    }
    assert_eq!(w.map.get(p(24, 24)), Tile::Shelter);
    assert!(w.shelter_test().passes());
    // Planks make plank walls; a side already walled needs no block.
    let mut w = flat();
    w.map.set(p(25, 24), Tile::Stone);
    give(&mut w, "oak_planks", 4);
    w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!(w.map.get(p(23, 24)), Tile::Door, "first open side");
    assert_eq!(w.map.get(p(25, 24)), Tile::Stone);
    assert_eq!(w.map.get(p(24, 25)), Tile::Planks);
    assert!(w.shelter_test().passes());
    // Digging in leaves every side open.
    let mut w = flat();
    w.execute(&act(Skill::Shelter, None), FULL);
    assert!(w.sheltered());
    assert_eq!(w.shelter_test(), test(false, true));
}

#[test]
fn a_hut_leaves_a_side_open_where_a_mob_stands() {
    // The pig is penned in on three sides, so it stays put while we build.
    let mut w = flat();
    for q in [p(23, 25), p(25, 25), p(24, 26)] {
        w.map.set(q, Tile::Stone);
    }
    w.mobs.push(Mob::new(MobKind::Pig, p(24, 25), 0));
    give(&mut w, "cobblestone", 4);
    w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!(w.map.get(p(24, 25)), Tile::Grass);
    assert_eq!(w.map.get(p(24, 23)), Tile::Stone);
    assert_eq!(w.mobs[0].pos, p(24, 25), "not walled into a block");
}

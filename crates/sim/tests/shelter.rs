//! The shelter test (01 §4.11, #58; spec §27 step 12) on hand-built maps:
//! open ground fails, a sealed pit fails, walls with a door pass.

mod common;

use common::*;
use host_api::{Pos, World};
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
fn a_room_must_be_smaller_than_the_spawn_distance() {
    // Hostiles spawn at Chebyshev distance ≥ spawn_min (6) from the agent,
    // so a room whose floor reaches 6 could have one spawn inside it.
    let near = exact().spawn_min;
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

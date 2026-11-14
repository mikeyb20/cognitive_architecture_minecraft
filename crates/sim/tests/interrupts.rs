//! A shock bout's start cuts the current action short (#63; benchmark B3):
//! the action fails without its result, is not blocked, and gives back what
//! it had used up, so the agent can choose again (the lever) mid-bout.

mod common;

use common::*;
use host_api::{Host, Outcome, Skill, World};
use sim::SimWorld;
use sim::map::Tile;
use sim::mob::{Mob, MobKind};
use sim::world::Shock;

/// A harmless bout starts at tick 1000; the clock stands at `tick`.
fn shocked(tick: u64) -> SimWorld {
    let mut w = flat();
    w.shock = Some(Shock::escapable(1000, 0.0, 1));
    w.tick = tick;
    w
}

fn cut(o: &Outcome, ticks: u32) {
    assert_eq!(
        (o.success, o.blocked, o.progress, o.duration_ticks),
        (false, false, 0.0, ticks),
        "{o:?}"
    );
}

#[test]
fn a_walk_stops_where_the_bout_finds_it() {
    let mut w = shocked(990);
    let o = w.execute(&at(Skill::MoveTo, 34, 24), FULL);
    cut(&o, 10);
    assert_eq!(w.body.pos, p(29, 24), "5 of 10 tiles at 2 ticks each");
}

#[test]
fn a_cut_chop_gains_nothing_and_the_next_one_runs_in_full() {
    let mut w = shocked(990);
    w.map.set(p(26, 24), Tile::Tree { chops_left: 3 });
    let o = w.execute(&at(Skill::Chop, 26, 24), FULL);
    cut(&o, 10);
    assert_eq!(w.body.inv.count("oak_log"), 0);
    assert_eq!(w.map.get(p(26, 24)), Tile::Tree { chops_left: 3 });
    let o = w.execute(&at(Skill::Chop, 26, 24), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 30), "{o:?}");
}

#[test]
fn a_cut_meal_keeps_the_food_in_hand() {
    let mut w = shocked(990);
    give(&mut w, "porkchop", 1);
    w.body.food = 0.5;
    let o = w.execute(&item(Skill::Eat, "porkchop"), FULL);
    cut(&o, 10);
    assert_eq!(w.body.inv.count("porkchop"), 1);
    assert!(w.body.food < 0.5, "no food restored");
}

#[test]
fn a_cut_craft_gives_the_ingredients_back() {
    let mut w = shocked(995);
    give(&mut w, "oak_log", 2);
    let o = w.execute(&item(Skill::Craft, "oak_planks"), FULL);
    cut(&o, 5);
    assert!(o.new_things.is_empty(), "no recipe learnt");
    assert_eq!(w.body.inv.count("oak_log"), 2);
    assert_eq!(w.body.inv.count("oak_planks"), 0);
}

#[test]
fn a_cut_shelter_gives_the_blocks_back_and_is_not_built() {
    let mut w = shocked(990);
    give(&mut w, "cobblestone", 4);
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    cut(&o, 10);
    assert_eq!(w.body.inv.count("cobblestone"), 4);
    assert_eq!(w.map.get(p(24, 24)), Tile::Grass);
    assert!(!w.sheltered());
    let mut w = shocked(995);
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    cut(&o, 5);
    assert_eq!(w.map.get(p(24, 24)), Tile::Grass, "nor dug in");
}

#[test]
fn a_cut_rest_fails() {
    let mut w = shocked(990);
    w.map.set(p(24, 24), Tile::Shelter);
    let o = w.execute(&act(Skill::Rest, None), FULL);
    cut(&o, 10);
}

#[test]
fn a_cut_attack_leaves_the_pig() {
    let mut w = shocked(995);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(25, 24), t));
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    cut(&o, 5);
    assert_eq!(w.mobs.len(), 1);
}

#[test]
fn only_a_bout_starting_interrupts() {
    // A bout already running, a world with no shock, and idle ticks do not.
    let mut w = shocked(999);
    w.shock = Some(Shock::escapable(1000, 0.0, 50));
    w.step_world(1);
    assert!(w.shock.as_ref().unwrap().in_bout(), "the 1000 bout runs");
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 6), "{o:?}");
    let mut w = flat();
    w.tick = 990;
    let o = w.execute(&at(Skill::MoveTo, 34, 24), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 20));
}

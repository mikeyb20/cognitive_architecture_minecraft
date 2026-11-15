//! The spaced attack (`skills::combat`): hit when recharged and in reach,
//! let the knockback push the hostile back, step back when it is next to
//! the agent. Hand-worked cases on the flat test map, plus properties.

mod common;

use common::*;
use host_api::{Host, Intensity, Skill};
use proptest::prelude::*;
use sim::Rules;
use sim::map::Tile;
use sim::mob::{Mob, MobKind};

/// Night, so no zombie is cleared by daylight.
const NIGHT: u64 = 1200;

// Zombie at 3 tiles: hit at 0 pushes it to 4; it walks back to 3 at tick 6;
// the swing recharges at 10 and hits it at 3 again; the same at 20 kills.
// It never gets next to the agent, so no damage.
#[test]
fn a_zombie_met_at_reach_never_lands_a_hit() {
    let mut w = flat();
    w.tick = NIGHT;
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(o.success);
    assert_eq!(o.duration_ticks, 20);
    assert_eq!(o.damage, 0.0);
    assert_eq!(w.body.pos, p(24, 24), "never had to move");
}

// Every swing misses (at 0, 10, 21, 31): the zombie closes and hits at
// tick 7; the agent steps back each time it comes next to it (7, 13, 19,
// 25), and the zombie's next hit, due from 27, lands as it steps in at 31,
// the tick the fourth swing ends the action.
#[test]
fn missed_swings_let_the_zombie_close_and_the_attack_gives_up() {
    let mut w = flat_with(Rules {
        base_success: 0.0,
        ..exact()
    });
    w.tick = NIGHT;
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(!o.success && !o.blocked);
    assert_eq!((o.progress, o.duration_ticks), (0.5, 31));
    assert!(close(o.damage, 0.2), "{}", o.damage);
    assert_eq!(w.mobs.len(), 1);
    assert_eq!(w.body.pos, p(20, 24), "four steps back");
}

// A wall right behind the zombie: the hit cannot push it, so it stays next
// to the agent; the agent steps back instead and the zombie follows.
#[test]
fn a_wall_stops_the_knockback() {
    let mut w = flat();
    w.tick = NIGHT;
    w.map.set(p(26, 24), Tile::Stone);
    zombie(&mut w, 25, 24);
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(o.success);
    assert!(o.damage > 0.0, "it was never pushed off: {}", o.damage);
}

// Boxed in with the zombie (stone on three sides, a resting pig behind):
// no tile to step back to, so the agent waits for its swing and trades hits.
#[test]
fn a_cornered_agent_trades_hits() {
    let mut w = flat();
    w.tick = NIGHT;
    for (x, z) in [(24, 23), (24, 25), (26, 24)] {
        w.map.set(p(x, z), Tile::Stone);
    }
    zombie(&mut w, 25, 24);
    w.mobs.push(Mob::new(MobKind::Pig, p(23, 24), NIGHT));
    w.mobs[1].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(o.success);
    assert_eq!(w.body.pos, p(24, 24));
    assert_eq!(o.duration_ticks, 20);
    assert!(close(o.damage, 0.1), "one hit at tick 1: {}", o.damage);
}

// Out of reach: the agent closes in a step at a time, then fights.
#[test]
fn a_far_zombie_is_closed_on_first() {
    let mut w = flat();
    w.tick = NIGHT;
    zombie(&mut w, 30, 24);
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 30, 24), FULL);
    assert!(o.success);
    assert_eq!(w.body.pos, p(27, 24), "walked in to 3 tiles");
    assert_eq!(o.damage, 0.0);
}

// The zombie despawns mid-fight: the attack ends without success.
#[test]
fn a_vanished_target_ends_the_fight() {
    let mut w = flat();
    w.tick = 100; // day: hostiles outside caves are cleared every tick
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(!o.success && !o.blocked);
    assert_eq!(o.progress, 0.0);
}

// Two hits of 0.5 leave the zombie at exactly 0 health, which kills it.
#[test]
fn zero_health_is_dead() {
    let mut w = flat_with(Rules {
        swing_damage: 0.5,
        ..exact()
    });
    w.tick = NIGHT;
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(o.success);
    assert_eq!((o.progress, o.duration_ticks), (1.0, 10));
}

// A push of 2 tiles: the hit at 0 sends it from 3 to 5 tiles, and the
// agent still knows it as the same zombie. It is at 4 when the swing is
// ready at 10, so the agent steps in (2 ticks) and hits at 12 (pushed to
// 4), and again at 22 as it walks back in to 3.
#[test]
fn a_longer_push_keeps_the_target() {
    let mut w = flat_with(Rules {
        knockback: 2,
        ..exact()
    });
    w.tick = NIGHT;
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(o.success);
    assert_eq!((o.duration_ticks, o.damage), (22, 0.0));
    assert_eq!(w.body.pos, p(25, 24));
}

// A zombie across water that never moves: the agent can neither reach it
// nor hit it, so it waits out the whole budget (2 × 4 swings × 10 ticks).
#[test]
fn an_unreachable_zombie_runs_out_the_budget() {
    let mut w = flat();
    w.tick = NIGHT;
    for z in 0..48 {
        w.map.set(p(27, z), Tile::Water);
    }
    zombie(&mut w, 29, 24);
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 29, 24), FULL);
    assert!(!o.success && !o.blocked);
    assert_eq!((o.progress, o.duration_ticks), (0.5, 80));
    assert_eq!(w.body.pos, p(24, 24));
}

// Too little effort to walk (spec §16): the agent stands and swings; at
// exactly the walking floor it steps back as usual.
#[test]
fn below_the_walking_floor_the_agent_stands_its_ground() {
    let still = Intensity {
        effort: 0.04,
        ..FULL
    };
    let mut w = flat();
    w.tick = NIGHT;
    zombie(&mut w, 25, 24);
    let o = w.execute(&at(Skill::Attack, 25, 24), still);
    assert!(o.success);
    assert_eq!(w.body.pos, p(24, 24));
    let floor = Intensity {
        effort: w.rules.min_effort,
        ..FULL
    };
    let mut w = flat();
    w.tick = NIGHT;
    zombie(&mut w, 25, 24);
    w.execute(&at(Skill::Attack, 25, 24), floor);
    assert_eq!(w.body.pos, p(23, 24));
}

// Dying mid-fight ends the attack at once.
#[test]
fn dying_ends_the_fight() {
    let mut w = flat_with(Rules {
        base_success: 0.0,
        ..exact()
    });
    w.tick = NIGHT;
    w.body.health = 0.05;
    zombie(&mut w, 27, 24);
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(!o.success && !w.alive());
    assert_eq!((o.progress, o.duration_ticks), (0.0, 7));
}

proptest! {
    // A lone zombie met within reach on open ground, with every swing
    // landing, dies without ever hitting the agent: this is the skill.
    #[test]
    fn a_lone_zombie_in_reach_is_killed_cleanly(dx in -2i32..=2, dz in -2i32..=2) {
        prop_assume!(dx != 0 || dz != 0);
        prop_assume!(dx.abs().max(dz.abs()) == 2);
        let mut w = flat();
        w.tick = NIGHT;
        zombie(&mut w, 24 + dx, 24 + dz);
        let o = w.execute(&at(Skill::Attack, 24 + dx, 24 + dz), FULL);
        prop_assert!(o.success);
        prop_assert_eq!(o.damage, 0.0);
    }

    // Whatever the odds, an attack stops within its tick budget (plus the
    // step it may be in) and never takes more than a hit per hit interval.
    #[test]
    fn a_fight_is_bounded(x in 20i32..=28, z in 20i32..=28, s in 0.0f32..=1.0) {
        prop_assume!((x, z) != (24, 24));
        let mut w = flat_with(Rules { base_success: s, ..exact() });
        w.tick = NIGHT;
        zombie(&mut w, x, z);
        let r = w.rules.clone();
        let o = w.execute(&at(Skill::Attack, x, z), FULL);
        let budget = 2 * r.max_swings * r.swing_ticks + r.walk_ticks;
        prop_assert!(o.duration_ticks <= budget, "{}", o.duration_ticks);
        let hits = o.damage / r.mob_damage;
        prop_assert!(hits <= (o.duration_ticks as f32 / r.mob_hit_ticks as f32).ceil() + 1e-4);
    }
}

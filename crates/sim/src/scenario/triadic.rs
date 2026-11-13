//! The triadic helplessness scenario (benchmark B3; Seligman & Maier 1967,
//! Maier & Seligman 2016): no mobs and no hunger, only shock bouts. A
//! pretreatment phase gives the arm's bouts (escapable, yoked or none),
//! then a test phase gives every arm the same escapable bouts.

use crate::rules::Rules;
use crate::world::{Shock, SimWorld};

use super::Setup;
use super::trips::setup;

/// Pretreatment and test length, in ticks (B3).
pub const TRIADIC_PRE: u64 = 600;
pub const TRIADIC_TEST: u64 = 600;
/// A bout starts every this many ticks (B3).
pub const SHOCK_EVERY: u64 = 30;
/// A bout not escaped deals damage for this many ticks (B3: flee within 10
/// ticks ends it).
pub const SHOCK_MAX_LEN: u64 = 10;
/// Damage per tick of a bout: a full bout deals 0.05 (B3), which regen
/// heals between bouts (proposed).
pub const SHOCK_DAMAGE: f32 = 0.005;

/// The B3 world: the seed's map with no mobs, no spawns and no hunger,
/// escapable bouts throughout, and the test phase from `TRIADIC_PRE`. The
/// bench swaps in a yoked or empty pretreatment with [`yoked`].
pub fn triadic(rules: Rules, seed: u64) -> Setup {
    let calm = Rules {
        night_spawn: 0.0,
        cave_spawn: 0.0,
        food_decay: 0.0,
        ..rules
    };
    let mut world = SimWorld::new(calm, seed);
    world.mobs.clear();
    world.shock = Some(
        Shock::escapable(SHOCK_EVERY, SHOCK_DAMAGE, SHOCK_MAX_LEN).with_test_from(TRIADIC_PRE),
    );
    setup("triadic", world, TRIADIC_PRE + TRIADIC_TEST)
}

/// The B3 shock for a yoked arm: replay `schedule` (an escapable run's
/// [`Shock::bouts`]) in pretreatment; an empty schedule is the no-shock
/// arm. The test phase is the same as [`triadic`]'s.
pub fn yoked(schedule: Vec<(u64, u64)>) -> Shock {
    Shock::yoked(SHOCK_EVERY, SHOCK_DAMAGE, SHOCK_MAX_LEN, schedule).with_test_from(TRIADIC_PRE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::{Host, Intensity, Skill, World};

    #[test]
    fn b3_values_by_hand() {
        assert_eq!((TRIADIC_PRE, TRIADIC_TEST), (600, 600));
        assert_eq!((SHOCK_EVERY, SHOCK_MAX_LEN), (30, 10));
        assert!((SHOCK_DAMAGE * SHOCK_MAX_LEN as f32 - 0.05).abs() < 1e-6);
        let s = triadic(Rules::default(), 4);
        assert_eq!((s.name, s.max_ticks), ("triadic", 1200));
        let r = &s.world.rules;
        assert_eq!((r.night_spawn, r.cave_spawn, r.food_decay), (0.0, 0.0, 0.0));
        assert!(s.world.mobs.is_empty());
        let k = s.world.shock.as_ref().unwrap();
        assert!(k.escapable && k.schedule.is_none());
        assert_eq!(k.test_from, Some(600));
        let y = yoked(vec![(30, 4)]);
        assert_eq!(y.schedule, Some(vec![(30, 4)]));
        assert_eq!((y.test_from, y.escapable), (Some(600), false));
    }

    #[test]
    fn a_whole_run_of_bouts_keeps_the_agent_alive_and_fed() {
        let mut s = triadic(Rules::default(), 4);
        let w = &mut s.world;
        w.step_world(1199);
        assert!(w.alive());
        assert_eq!(w.body.food, 1.0);
        assert!(w.mobs.is_empty(), "nothing spawns");
        let b = w.shock.as_ref().unwrap().bouts();
        assert_eq!(b.len(), 39, "every 30 ticks from tick 30");
        assert!(b.iter().all(|(t, n)| t % 30 == 0 && *n == 10), "{b:?}");
    }

    #[test]
    fn flee_is_the_lever_during_a_bout() {
        let mut s = triadic(Rules::default(), 4);
        let w = &mut s.world;
        let flee = |w: &mut SimWorld| {
            let o = w.perceive();
            w.candidate_actions(&o)
                .into_iter()
                .find(|c| c.skill == Skill::Flee)
        };
        assert!(flee(w).is_none(), "no bout, no lever");
        w.step_world(30);
        let c = flee(w).expect("offered in a bout");
        let out = w.execute(
            &c,
            Intensity {
                effort: 1.0,
                focus: 1.0,
                external_drive: false,
            },
        );
        assert!(out.success);
        let k = w.shock.as_ref().unwrap();
        assert!(!k.in_bout());
        assert_eq!(k.bouts(), [(30, 1)], "one tick of damage, then fled");
        // Walled in, the lever is pulled on the spot.
        let me = w.body.pos;
        for p in crate::geom::ring(me, 1) {
            w.map.set(p, crate::map::Tile::Stone);
        }
        w.step_world(30);
        let c = flee(w).expect("still offered");
        assert_eq!(c.target, Some(host_api::Target::Pos(me)));
        assert!(c.est_cost < w.cost(1, 0.1), "and costs no steps");
    }
}

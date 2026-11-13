//! Shock bouts for the triadic helplessness benchmark (B3; Seligman & Maier
//! 1967, Maier & Seligman 2016): bouts of damage that the agent can end by
//! fleeing (escapable), or that replay another run's bouts exactly whatever
//! it does (yoked). From `test_from` on, bouts are escapable for every arm.

use super::{Death, SimWorld};

/// Shock bout settings and what happened (benchmark B3).
#[derive(Debug, Clone, PartialEq)]
pub struct Shock {
    /// A bout starts on every tick that is a multiple of this (escapable
    /// and test phases).
    pub every: u64,
    /// Damage dealt on each tick of a bout.
    pub dmg_per_tick: f32,
    /// A bout not escaped ends after this many ticks of damage.
    pub max_len: u64,
    /// Fleeing ends the bout.
    pub escapable: bool,
    /// Yoked: replay these (start tick, ticks of damage) bouts instead;
    /// fleeing does nothing.
    pub schedule: Option<Vec<(u64, u64)>>,
    /// From this tick on, every arm gets escapable bouts on `every` (the
    /// test phase).
    pub test_from: Option<u64>,
    /// The bouts that have ended: (start tick, ticks of damage dealt).
    realised: Vec<(u64, u64)>,
    /// The open bout: (start tick, ticks of damage dealt so far).
    open: Option<(u64, u64)>,
}

impl Shock {
    /// Escapable bouts on `every` (B3 pretreatment, escapable arm).
    pub fn escapable(every: u64, dmg_per_tick: f32, max_len: u64) -> Self {
        Self {
            every,
            dmg_per_tick,
            max_len,
            escapable: true,
            schedule: None,
            test_from: None,
            realised: Vec::new(),
            open: None,
        }
    }

    /// Bouts that replay `schedule` exactly; fleeing does nothing (B3
    /// pretreatment, yoked arm). `every` and `max_len` apply in the test.
    pub fn yoked(every: u64, dmg_per_tick: f32, max_len: u64, schedule: Vec<(u64, u64)>) -> Self {
        Self {
            escapable: false,
            schedule: Some(schedule),
            ..Self::escapable(every, dmg_per_tick, max_len)
        }
    }

    /// The same bouts, with the test phase from `tick` on (B3).
    pub fn with_test_from(self, tick: u64) -> Self {
        Self {
            test_from: Some(tick),
            ..self
        }
    }

    /// Whether a bout is running now.
    pub fn in_bout(&self) -> bool {
        self.open.is_some()
    }

    /// The bouts as they happened, in order, a running one last: (start
    /// tick, ticks of damage dealt). A yoked run replays this (B3).
    pub fn bouts(&self) -> Vec<(u64, u64)> {
        self.realised.iter().copied().chain(self.open).collect()
    }

    fn testing(&self, tick: u64) -> bool {
        self.test_from.is_some_and(|t| tick >= t)
    }

    /// The replayed length of a bout starting at `tick`, if the yoked
    /// schedule has one and the test phase has not begun.
    fn replayed(&self, tick: u64) -> Option<u64> {
        if self.testing(tick) {
            return None;
        }
        let s = self.schedule.as_ref()?;
        s.iter().find(|(t, _)| *t == tick).map(|(_, len)| *len)
    }

    /// Whether fleeing ends a bout at `tick`.
    fn lever_works(&self, tick: u64) -> bool {
        self.testing(tick) || (self.escapable && self.schedule.is_none())
    }

    fn close(&mut self) {
        if let Some(b) = self.open.take() {
            self.realised.push(b);
        }
    }
}

impl SimWorld {
    /// One tick of shock: start a bout when due, deal its damage, end it
    /// at its length (B3).
    pub(super) fn shock_tick(&mut self) {
        let t = self.tick;
        let Some(s) = self.shock.as_mut() else {
            return;
        };
        if s.open.is_none() {
            let yoked = s.schedule.is_some() && !s.testing(t);
            let due = if yoked {
                s.replayed(t).is_some()
            } else {
                t % s.every == 0
            };
            if due {
                s.open = Some((t, 0));
            }
        }
        let Some((start, dealt)) = s.open else {
            return;
        };
        let dmg = s.dmg_per_tick;
        let len = s.replayed(start).unwrap_or(s.max_len);
        s.open = Some((start, dealt + 1));
        if dealt + 1 >= len {
            s.close();
        }
        self.hurt(dmg, Death::Shock);
    }

    /// The agent fled: an escapable bout ends now (B3 lever).
    pub(crate) fn shock_lever(&mut self) {
        let t = self.tick;
        if let Some(s) = self.shock.as_mut() {
            if s.lever_works(t) {
                s.close();
            }
        }
    }

    /// Whether a shock bout is running (fleeing is offered as the lever).
    pub(crate) fn in_shock_bout(&self) -> bool {
        self.shock.as_ref().is_some_and(Shock::in_bout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;
    use proptest::prelude::*;

    /// A calm world (no hunger, no regen, no mobs) with `shock`.
    fn world(shock: Shock) -> SimWorld {
        let rules = Rules {
            food_decay: 0.0,
            regen: 0.0,
            night_spawn: 0.0,
            cave_spawn: 0.0,
            ..Rules::default()
        };
        let mut w = SimWorld::new(rules, 1);
        w.mobs.clear();
        w.shock = Some(shock);
        w
    }

    fn ticks(w: &mut SimWorld, n: u64) {
        for _ in 0..n {
            w.tick_once();
        }
    }

    fn bouts(w: &SimWorld) -> Vec<(u64, u64)> {
        w.shock.as_ref().unwrap().bouts()
    }

    /// Run `n` ticks, pulling the lever on the given ticks.
    fn run(mut w: SimWorld, n: u64, pulls: &[u64]) -> SimWorld {
        for _ in 0..n {
            w.tick_once();
            if pulls.contains(&w.tick) {
                w.shock_lever();
            }
        }
        w
    }

    #[test]
    fn an_escapable_bout_by_hand() {
        let mut w = world(Shock::escapable(5, 0.1, 3));
        assert!(!w.in_shock_bout());
        ticks(&mut w, 4);
        assert_eq!((w.body.health, bouts(&w)), (1.0, vec![]), "none before 5");
        ticks(&mut w, 1);
        assert!(w.in_shock_bout());
        assert!((w.body.health - 0.9).abs() < 1e-6);
        assert_eq!(bouts(&w), [(5, 1)], "the running bout counts");
        w.shock_lever();
        assert!(!w.in_shock_bout(), "fled");
        ticks(&mut w, 4);
        assert!((w.body.health - 0.9).abs() < 1e-6, "no damage till 10");
        ticks(&mut w, 3);
        assert!((w.body.health - 0.6).abs() < 1e-6, "a full bout is 3 ticks");
        assert!(!w.in_shock_bout(), "and then ends");
        assert_eq!(bouts(&w), [(5, 1), (10, 3)]);
        w.shock_lever();
        assert_eq!(bouts(&w), [(5, 1), (10, 3)], "no bout, nothing to end");
    }

    #[test]
    fn a_yoked_bout_ignores_the_lever_until_the_test() {
        let s = Shock::yoked(5, 0.1, 3, vec![(2, 2), (7, 1)]).with_test_from(10);
        let mut w = world(s);
        ticks(&mut w, 2);
        w.shock_lever();
        assert!(w.in_shock_bout(), "the lever does nothing");
        ticks(&mut w, 8);
        assert_eq!(bouts(&w), [(2, 2), (7, 1), (10, 1)], "test bouts on 5s");
        w.shock_lever();
        assert!(!w.in_shock_bout(), "in the test the lever works");
        assert!((w.body.health - 0.6).abs() < 1e-6);
    }

    #[test]
    fn inescapable_bouts_on_a_cadence_and_a_shock_death() {
        let s = Shock {
            escapable: false,
            ..Shock::escapable(2, 0.5, 4)
        };
        let mut w = world(s);
        ticks(&mut w, 2);
        w.shock_lever();
        ticks(&mut w, 1);
        assert_eq!(w.body.death, Some(Death::Shock));
        assert_eq!(bouts(&w), [(2, 2)]);
    }

    #[test]
    fn no_shock_does_nothing() {
        let mut w = world(Shock::escapable(1, 0.5, 1));
        w.shock = None;
        ticks(&mut w, 10);
        w.shock_lever();
        assert!(!w.in_shock_bout());
        assert_eq!(w.body.health, 1.0);
    }

    proptest! {
        #[test]
        fn a_yoked_run_takes_the_escapable_runs_damage(
            every in 2u64..20,
            max_len in 1u64..10,
            pulls in prop::collection::vec(1u64..200, 0..30),
        ) {
            let esc = run(world(Shock::escapable(every, 0.001, max_len)), 200, &pulls);
            let sched = bouts(&esc);
            prop_assert!(sched.iter().all(|(t, n)| t % every == 0 && (1..=max_len).contains(n)));
            prop_assert!(sched.windows(2).all(|p| p[0].0 + p[0].1 <= p[1].0));
            let yoked = Shock::yoked(every, 0.001, max_len, sched.clone());
            let other: Vec<u64> = (1..200).filter(|t| !pulls.contains(t)).collect();
            let y = run(world(yoked), 200, &other);
            prop_assert_eq!(bouts(&y), sched);
            prop_assert!((y.body.health - esc.body.health).abs() < 1e-5);
            prop_assert!((0.0..=1.0).contains(&y.body.health));
        }
    }
}

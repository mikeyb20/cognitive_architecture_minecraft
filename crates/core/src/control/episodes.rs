//! Tenacity episodes (01 §4.4; spec §10): which (mode, task) runs count as
//! "persisted through negative outcomes", and whether persisting paid off.

use super::tenacity::Tenacity;
use crate::modes::Mode;
use host_api::TaskType;
use serde::{Deserialize, Serialize};

/// Ticks a mode must persist after a setback, and ticks uncredited before an
/// episode closes (spec §10: N_p = 10).
pub const N_P: u64 = 10;
/// A mode "keeps its weight" while w_i stays within this of its pre-setback
/// value (spec §10).
pub const W_KEEP: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Episode {
    task: TaskType,
    last_credited: u64,
    /// Tick and w_i of the first setback.
    setback: Option<(u64, f32)>,
    /// Start of the current run of persisting ticks after the setback.
    persist_from: Option<u64>,
    persisted: bool,
    paid_off: bool,
}

/// One tick as the tracker sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EpisodeTick {
    pub tick: u64,
    pub task: TaskType,
    pub credited: Mode,
    pub w: [f32; 4],
    pub success: [f32; 4],
    pub d_goal: f32,
    pub goal_done: bool,
}

/// T[mode, task] plus the open episode per mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenacityTable {
    t: [[Tenacity; 7]; 4],
    open: [Option<Episode>; 4],
}

impl TenacityTable {
    /// Priors (a, b, γ) per mode (spec §19: the archetype sets Performing's).
    pub fn new(priors: [(f32, f32, f32); 4]) -> Self {
        Self {
            t: priors.map(|(a, b, g)| [Tenacity::new(a, b, g); 7]),
            open: [None; 4],
        }
    }

    /// T[mode, task] ∈ [0, 1].
    pub fn value(&self, m: Mode, task: TaskType) -> f32 {
        self.t[m.index()][task.index()].value()
    }

    /// T for every mode on one task (input to the Oracle update, spec §8).
    pub fn for_task(&self, task: TaskType) -> [f32; 4] {
        Mode::ALL.map(|m| self.value(m, task))
    }

    /// Advance every mode's episode by one tick. Returns the modes whose
    /// episode closed and counted (persisted through a setback), with
    /// whether it paid off.
    pub fn step(&mut self, x: EpisodeTick) -> Vec<(Mode, bool)> {
        let mut counted = Vec::new();
        for m in Mode::ALL {
            let i = m.index();
            let credited = x.credited == m;
            if let Some(ep) = self.open[i] {
                let uncredited_too_long = !credited && x.tick >= ep.last_credited + N_P;
                if ep.task != x.task || x.goal_done || uncredited_too_long {
                    self.open[i] = None;
                    if ep.setback.is_some() && ep.persisted {
                        self.t[i][ep.task.index()].record(f32::from(u8::from(ep.paid_off)));
                        counted.push((m, ep.paid_off));
                    }
                }
            }
            if self.open[i].is_none() && credited {
                self.open[i] = Some(Episode {
                    task: x.task,
                    last_credited: x.tick,
                    setback: None,
                    persist_from: None,
                    persisted: false,
                    paid_off: false,
                });
            }
            if let Some(ep) = self.open[i].as_mut() {
                Self::advance(ep, credited, x, i);
            }
        }
        counted
    }

    fn advance(ep: &mut Episode, credited: bool, x: EpisodeTick, i: usize) {
        if credited {
            ep.last_credited = x.tick;
        }
        match ep.setback {
            None if x.success[i] < 0.0 => {
                ep.setback = Some((x.tick, x.w[i]));
                ep.persist_from = Some(x.tick);
            }
            None => {}
            Some((_, w_pre)) => {
                let holding = credited || (x.w[i] - w_pre).abs() <= W_KEEP;
                if !holding {
                    ep.persist_from = None;
                } else if ep.persist_from.is_none() {
                    ep.persist_from = Some(x.tick);
                }
                if ep.persist_from.is_some_and(|t0| x.tick >= t0 + N_P) {
                    ep.persisted = true;
                }
                if ep.persisted && (x.success[i] > 0.0 || x.d_goal > 0.0) {
                    ep.paid_off = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINE: TaskType = TaskType::Mining;

    fn table() -> TenacityTable {
        TenacityTable::new([(1.0, 1.0, 1.0); 4])
    }

    fn tick(t: u64, credited: Mode, p_success: f32) -> EpisodeTick {
        EpisodeTick {
            tick: t,
            task: MINE,
            credited,
            w: [0.25; 4],
            success: [0.0, 0.0, p_success, 0.0],
            d_goal: 0.0,
            goal_done: false,
        }
    }

    /// Credit P from tick 0, setback at tick 1, persist through tick 11.
    fn persist(tb: &mut TenacityTable) {
        tb.step(tick(0, Mode::Performing, 0.0));
        tb.step(tick(1, Mode::Performing, -1.0));
        for t in 2..=11 {
            assert!(tb.step(tick(t, Mode::Performing, 0.0)).is_empty());
        }
    }

    #[test]
    fn persisting_and_paying_off_raises_t() {
        let mut tb = table();
        assert_eq!(tb.value(Mode::Performing, MINE), 0.5);
        persist(&mut tb);
        tb.step(tick(12, Mode::Performing, 1.0));
        let mut t2 = tick(13, Mode::Performing, 0.0);
        t2.task = TaskType::Travel;
        assert_eq!(tb.step(t2), vec![(Mode::Performing, true)]);
        assert!((tb.value(Mode::Performing, MINE) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(tb.value(Mode::Performing, TaskType::Travel), 0.5);
        assert_eq!(tb.for_task(MINE)[2], tb.value(Mode::Performing, MINE));
    }

    #[test]
    fn persisting_without_payoff_lowers_t() {
        let mut tb = table();
        persist(&mut tb);
        let mut done = tick(12, Mode::Performing, 0.0);
        done.goal_done = true;
        assert_eq!(tb.step(done), vec![(Mode::Performing, false)]);
        assert!((tb.value(Mode::Performing, MINE) - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn goal_progress_counts_as_payoff() {
        let mut tb = table();
        persist(&mut tb);
        let mut prog = tick(12, Mode::Performing, 0.0);
        prog.d_goal = 0.1;
        tb.step(prog);
        let mut done = tick(13, Mode::Performing, 0.0);
        done.goal_done = true;
        assert_eq!(tb.step(done), vec![(Mode::Performing, true)]);
    }

    #[test]
    fn giving_up_early_does_not_count() {
        let mut tb = table();
        tb.step(tick(0, Mode::Performing, 0.0));
        tb.step(tick(1, Mode::Performing, -1.0));
        for t in 2..=10 {
            tb.step(tick(t, Mode::Performing, 0.0));
        }
        // Tick 11 would complete N_p; the task changes instead.
        let mut t = tick(11, Mode::Performing, 0.0);
        t.task = TaskType::Travel;
        assert!(tb.step(t).is_empty());
        assert_eq!(tb.value(Mode::Performing, MINE), 0.5);
    }

    #[test]
    fn no_setback_does_not_count() {
        let mut tb = table();
        for t in 0..20 {
            tb.step(tick(t, Mode::Performing, 1.0));
        }
        let mut t = tick(20, Mode::Performing, 0.0);
        t.goal_done = true;
        assert!(tb.step(t).is_empty());
    }

    #[test]
    fn losing_credit_and_weight_breaks_persistence() {
        let mut tb = table();
        tb.step(tick(0, Mode::Performing, 0.0));
        tb.step(tick(1, Mode::Performing, -1.0));
        let mut drop = tick(5, Mode::Learning, 0.0);
        drop.w[2] = 0.1;
        tb.step(drop);
        // Run restarts at tick 6, so tick 15 is one short of N_p.
        for t in 6..=15 {
            tb.step(tick(t, Mode::Performing, 0.0));
        }
        let mut done = tick(15, Mode::Performing, 0.0);
        done.goal_done = true;
        assert!(tb.step(done).is_empty(), "only 9 ticks held");
    }

    #[test]
    fn keeping_weight_without_credit_counts_as_persisting() {
        let mut tb = table();
        tb.step(tick(0, Mode::Performing, 0.0));
        tb.step(tick(1, Mode::Performing, -1.0));
        for t in 2..=9 {
            tb.step(tick(t, Mode::Learning, 0.0));
        }
        tb.step(tick(10, Mode::Performing, 0.0));
        tb.step(tick(11, Mode::Performing, 0.0));
        let mut done = tick(12, Mode::Performing, 0.0);
        done.goal_done = true;
        assert_eq!(tb.step(done), vec![(Mode::Performing, false)]);
    }

    #[test]
    fn uncredited_for_n_p_ticks_closes_the_episode() {
        let mut tb = table();
        persist(&mut tb);
        let mut away = tick(21, Mode::Learning, 0.0);
        away.w[2] = 0.9;
        assert!(tb.step(tick(20, Mode::Learning, 0.0)).is_empty(), "9 ticks");
        assert_eq!(tb.step(away), vec![(Mode::Performing, false)]);
    }
}

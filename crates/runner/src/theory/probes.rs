//! What each theory test reads from a run (spec §29). A probe sees every
//! log row, then the result, and returns one number per column. NaN means
//! "no data in this run" and is left out of the means.

use cognitive_core::agent::TickLog;
use cognitive_core::modes::Mode;
use sim::scenario::DAY;

use crate::brain::BrainResult;
use crate::sweep::tech_held;

/// Reads one run for one theory test.
pub trait Probe {
    fn push(&mut self, row: &TickLog);
    fn finish(&self, r: &BrainResult) -> Vec<f64>;
}

/// What kind of thing a decision did, from its logged label (spec §25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Travel,
    Gather,
    Craft,
    Survive,
    Think,
    Idle,
    Other,
}

/// The kind of a logged `chosen` label ("Chop:3,4", "Plan", "idle").
pub fn kind(chosen: &str) -> Kind {
    let head = chosen.split(':').next().unwrap_or("");
    match head {
        "Explore" | "MoveTo" | "FollowPlayer" => Kind::Travel,
        "Mine" | "Chop" | "Farm" | "Attack" => Kind::Gather,
        "Craft" | "Place" => Kind::Craft,
        "Eat" | "Flee" | "Shelter" | "Rest" => Kind::Survive,
        "Reflect" | "Imagine" | "Rehearse" | "Consolidate" | "Plan" => Kind::Think,
        "idle" => Kind::Idle,
        _ => Kind::Other,
    }
}

/// Running mean that reports NaN when empty.
#[derive(Debug, Default, Clone, Copy)]
pub struct Mean {
    sum: f64,
    n: u32,
}

impl Mean {
    pub fn add(&mut self, x: f32) {
        self.sum += f64::from(x);
        self.n += 1;
    }
    pub fn get(&self) -> f64 {
        if self.n == 0 {
            f64::NAN
        } else {
            self.sum / f64::from(self.n)
        }
    }
}

/// T-IND: each mode's mean success on decisions that acted (00 §9.4.1-2).
#[derive(Debug, Default)]
pub struct ModeSuccess {
    s: [Mean; 4],
}

impl Probe for ModeSuccess {
    fn push(&mut self, row: &TickLog) {
        if kind(&row.chosen) != Kind::Idle {
            for (m, x) in self.s.iter_mut().zip(row.success) {
                m.add(x);
            }
        }
    }
    fn finish(&self, _: &BrainResult) -> Vec<f64> {
        self.s.iter().map(Mean::get).collect()
    }
}

/// Decisions in a row with the same label that count as stuck (proposed).
pub const STUCK_STREAK: u32 = 50;

/// No-kick ablation (00 §9.4.1-3): stuck incidents, mean time to escape,
/// longest streak, and stuck kicks.
#[derive(Debug, Default)]
pub struct Streaks {
    last: String,
    run: u32,
    stuck: Mean,
    longest: u32,
    kicks: u32,
}

/// Count a finished streak of `run` decisions.
fn close(run: u32, stuck: &mut Mean, longest: &mut u32) {
    if run >= STUCK_STREAK {
        stuck.add(run as f32);
    }
    *longest = (*longest).max(run);
}

impl Probe for Streaks {
    fn push(&mut self, row: &TickLog) {
        self.kicks += u32::from(row.stuck_kick);
        if row.chosen == self.last {
            self.run += 1;
        } else {
            close(self.run, &mut self.stuck, &mut self.longest);
            self.last.clone_from(&row.chosen);
            self.run = 1;
        }
    }
    fn finish(&self, _: &BrainResult) -> Vec<f64> {
        let (mut stuck, mut longest) = (self.stuck, self.longest);
        close(self.run, &mut stuck, &mut longest);
        vec![
            f64::from(stuck.n),
            stuck.get(),
            f64::from(longest),
            f64::from(self.kicks),
        ]
    }
}

/// T-MODE behaviour profile (00 §9.4.1-4): share of decisions per kind,
/// and mean effort on actions.
#[derive(Debug, Default)]
pub struct Behaviour {
    n: u32,
    counts: [u32; 5],
    effort: Mean,
}

/// The kinds a behaviour profile counts, in column order.
pub const PROFILE: [Kind; 5] = [
    Kind::Travel,
    Kind::Gather,
    Kind::Craft,
    Kind::Think,
    Kind::Idle,
];

impl Probe for Behaviour {
    fn push(&mut self, row: &TickLog) {
        self.n += 1;
        let k = kind(&row.chosen);
        if let Some(i) = PROFILE.iter().position(|p| *p == k) {
            self.counts[i] += 1;
        }
        if !matches!(k, Kind::Idle | Kind::Think) {
            self.effort.add(row.effort);
        }
    }
    fn finish(&self, _: &BrainResult) -> Vec<f64> {
        let n = f64::from(self.n.max(1));
        let mut out: Vec<f64> = self.counts.iter().map(|c| f64::from(*c) / n).collect();
        out.push(self.effort.get());
        out
    }
}

/// No-subconscious ablation (00 §9.4.1-6): tech held, links formed, items
/// the subconscious processed, novelty found.
#[derive(Debug, Default)]
pub struct Insight {
    formed: u32,
    processed: u32,
}

impl Probe for Insight {
    fn push(&mut self, row: &TickLog) {
        self.formed += row.formed;
        self.processed += row.sub_processed;
    }
    fn finish(&self, r: &BrainResult) -> Vec<f64> {
        vec![
            f64::from(tech_held(&r.inventory)),
            f64::from(self.formed),
            f64::from(self.processed),
            r.new_things as f64,
        ]
    }
}

/// High C improves Performing (00 §4.3): mean C, effort on actions,
/// Performing's success on actions, tech held.
#[derive(Debug, Default)]
pub struct Perform {
    c: Mean,
    effort: Mean,
    s_p: Mean,
}

impl Probe for Perform {
    fn push(&mut self, row: &TickLog) {
        self.c.add(row.c);
        if !matches!(kind(&row.chosen), Kind::Idle | Kind::Think) {
            self.effort.add(row.effort);
            self.s_p.add(row.success[Mode::Performing.index()]);
        }
    }
    fn finish(&self, r: &BrainResult) -> Vec<f64> {
        vec![
            self.c.get(),
            self.effort.get(),
            self.s_p.get(),
            f64::from(tech_held(&r.inventory)),
        ]
    }
}

/// Hostile environments drop C (00 §2.6): mean C and D during the harsh
/// window (ticks DAY/2..DAY) and the day after it (DAY..2·DAY).
#[derive(Debug, Default)]
pub struct Windows {
    during: [Mean; 2],
    after: [Mean; 2],
}

impl Probe for Windows {
    fn push(&mut self, row: &TickLog) {
        let w = if (DAY / 2..DAY).contains(&row.tick) {
            &mut self.during
        } else if (DAY..2 * DAY).contains(&row.tick) {
            &mut self.after
        } else {
            return;
        };
        w[0].add(row.c);
        w[1].add(row.d);
    }
    fn finish(&self, _: &BrainResult) -> Vec<f64> {
        let [cd, dd] = self.during.map(|m| m.get());
        let [ca, da] = self.after.map(|m| m.get());
        vec![cd, ca, dd, da]
    }
}

/// Mortality raises C and exploring (00 §7.9): mean C, U, w_E, and the
/// share of decisions that travel.
#[derive(Debug, Default)]
pub struct Urgency {
    n: u32,
    travel: u32,
    c: Mean,
    u: Mean,
    w_e: Mean,
}

impl Probe for Urgency {
    fn push(&mut self, row: &TickLog) {
        self.n += 1;
        self.travel += u32::from(kind(&row.chosen) == Kind::Travel);
        self.c.add(row.c);
        self.u.add(row.u);
        self.w_e.add(row.w[Mode::Exploring.index()]);
    }
    fn finish(&self, _: &BrainResult) -> Vec<f64> {
        vec![
            self.c.get(),
            self.u.get(),
            self.w_e.get(),
            f64::from(self.travel) / f64::from(self.n.max(1)),
        ]
    }
}

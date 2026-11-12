//! What a run shows (spec §25, §26): per-run metrics read from the log
//! rows as they come, and their summary over seeds. Sweeps, experiments
//! and theory probes all tabulate these.

use cognitive_core::agent::TickLog;
use sim::planner::TECH_GOALS;

use crate::brain::BrainResult;

/// What one run shows, from its log rows and result.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunMetrics {
    pub survived: bool,
    pub ticks: u64,
    pub decisions: u32,
    /// Share of decisions that started something (not idle).
    pub active: f32,
    /// Share of decisions that started something without external drive.
    pub self_started: f32,
    pub mean_c: f32,
    pub mean_d: f32,
    pub mean_v: f32,
    /// Share of decisions where the modes nearly cancel (conflict > 0.9).
    pub freeze: f32,
    pub stuck_kicks: u32,
    pub reflexes: u32,
    pub new_things: usize,
    /// Tech-tree goals held at the end (spec §21), 0..4.
    pub tech: u32,
    /// Items in memory at the end (spec §25 `mem_items`; plan §5.2 flat
    /// memory's bloat).
    pub memory: f32,
    /// Share of decisions that recalled a fact linked to a cue (spec §25
    /// `retrieval_hit`; plan §5.2 flat memory's recall).
    pub recall: f32,
    /// Plan, reflect and imagine runs per 1,000 ticks, chosen or triggered
    /// (each is an LLM call with an LLM tier; plan §5.2 fixed-trigger
    /// thinking).
    pub thinking: f32,
    /// Mean ticks from a new goal to its first plan step; NaN when no goal
    /// got one (plan §5.2 fixed-trigger thinking).
    pub plan_lag: f32,
    /// The first tick the agent stood in a shelter that passes the shelter
    /// test (01 §4.11). On `held_out` this is the transfer test (plan §5.0).
    pub shelter_at: Option<u64>,
    /// Ticks into the run to each `PACE` milestone, if reached (plan §5.0
    /// progression pace).
    pub pace: [Option<u64>; 5],
    /// Mean ticks from a setback to work again (plan §5.0 recovery); NaN
    /// when the run had none it recovered from.
    pub recovery: f32,
    /// Setbacks in the run, and how many it recovered from.
    pub setbacks: u32,
    pub recovered: u32,
}

/// The internal actions that call the LLM tool (spec §6, #40).
const LLM_THOUGHTS: [&str; 3] = ["Plan", "Reflect", "Imagine"];

/// Conflict above which a decision counts as near-freeze (01 §1.5).
pub const FREEZE_CONFLICT: f32 = 0.9;

/// Accumulates `RunMetrics` from rows as they come.
#[derive(Debug, Default)]
pub struct Meter {
    n: u32,
    active: u32,
    self_started: u32,
    c: f64,
    d: f64,
    v: f64,
    freeze: u32,
    stuck: u32,
    recalled: u32,
    memory: usize,
    thoughts: u32,
    goal: Option<String>,
    /// Tick the current goal was set, until its first plan step.
    waiting: Option<u64>,
    lag_sum: u64,
    lags: u32,
}

impl Meter {
    pub fn push(&mut self, r: &TickLog) {
        self.n += 1;
        let acted = r.chosen != "idle";
        self.active += u32::from(acted);
        self.self_started += u32::from(acted && !r.external_drive);
        self.c += f64::from(r.c);
        self.d += f64::from(r.d);
        self.v += f64::from(r.v);
        self.freeze += u32::from(acted && r.conflict > FREEZE_CONFLICT);
        self.stuck += u32::from(r.stuck_kick);
        self.recalled += u32::from(r.retrieval_hit);
        self.memory = r.mem_items;
        self.thoughts += r.triggered + u32::from(LLM_THOUGHTS.contains(&r.chosen.as_str()));
        if r.goal != self.goal {
            self.goal.clone_from(&r.goal);
            self.waiting = r.goal.as_ref().map(|_| r.tick);
        }
        if r.plan_step.is_some() {
            if let Some(t0) = self.waiting.take() {
                self.lag_sum += r.tick - t0;
                self.lags += 1;
            }
        }
    }

    pub fn finish(&self, r: &BrainResult) -> RunMetrics {
        let n = self.n.max(1) as f32;
        let mean = |x: f64| (x / f64::from(self.n.max(1))) as f32;
        RunMetrics {
            survived: r.survived,
            ticks: r.ticks,
            decisions: self.n,
            active: self.active as f32 / n,
            self_started: self.self_started as f32 / n,
            mean_c: mean(self.c),
            mean_d: mean(self.d),
            mean_v: mean(self.v),
            freeze: self.freeze as f32 / n,
            stuck_kicks: self.stuck,
            reflexes: r.reflexes,
            new_things: r.new_things,
            tech: tech_held(&r.inventory),
            memory: self.memory as f32,
            recall: self.recalled as f32 / n,
            thinking: self.thoughts as f32 * 1000.0 / r.ticks.max(1) as f32,
            plan_lag: if self.lags == 0 {
                f32::NAN
            } else {
                self.lag_sum as f32 / self.lags as f32
            },
            shelter_at: r.shelter_at,
            pace: pace(r),
            ..recovery(r)
        }
    }
}

/// Plan §5.0 progression-pace milestones: wood, stone tools, iron, food
/// gathered from the land (the sim has no planting) and a shelter that
/// passes the test. The first four are items first held.
pub const PACE: [&str; 5] = [
    "oak_log",
    "stone_pickaxe",
    "raw_iron",
    "wheat",
    SHELTER_MILESTONE,
];
/// The pace milestone read from the shelter test, not the inventory.
pub const SHELTER_MILESTONE: &str = "shelter";

/// Ticks into the run to each `PACE` milestone (plan §5.0), for any arm.
pub fn pace(r: &BrainResult) -> [Option<u64>; 5] {
    let first = |m: &str| {
        if m == SHELTER_MILESTONE {
            r.shelter_at
        } else {
            r.firsts.iter().find(|(i, _)| i == m).map(|(_, t)| *t)
        }
    };
    PACE.map(|m| first(m).map(|t| t.saturating_sub(r.start_tick)))
}

/// Plan §5.0 recovery for any arm: only the recovery fields are set.
pub fn recovery(r: &BrainResult) -> RunMetrics {
    let n = r.recoveries.len() as u32;
    RunMetrics {
        recovery: finite_mean(r.recoveries.iter().map(|&t| t as f32)),
        setbacks: n + r.unrecovered,
        recovered: n,
        ..RunMetrics::default()
    }
}

/// Tech-tree goals held in an inventory (spec §21), 0..4.
pub fn tech_held(inventory: &[(String, u32)]) -> u32 {
    let held = |g: &str| inventory.iter().any(|(i, k)| i == g && *k > 0);
    TECH_GOALS.iter().filter(|g| held(g)).count() as u32
}

/// Mean of each metric over seeds, and the survival rate.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub runs: u32,
    pub survival: f32,
    pub active: f32,
    pub self_started: f32,
    pub mean_c: f32,
    pub mean_d: f32,
    pub mean_v: f32,
    pub freeze: f32,
    pub stuck_kicks: f32,
    pub reflexes: f32,
    pub new_things: f32,
    pub tech: f32,
    pub memory: f32,
    pub recall: f32,
    pub thinking: f32,
    /// Mean over the runs where some goal got a plan; NaN if none did.
    pub plan_lag: f32,
    /// Share of runs that stood in a passing shelter (01 §4.11); on
    /// `held_out`, the transfer score (plan §5.0).
    pub shelter: f32,
    /// Per `PACE` milestone: the share of runs that reached it and the mean
    /// ticks to it over those runs (NaN if none did).
    pub pace: [Pace; 5],
    /// Mean over runs of each run's mean recovery ticks (runs with none
    /// left out; NaN if no run had one), and the share of all setbacks
    /// recovered from (NaN if there were none).
    pub recovery: f32,
    pub recovered: f32,
}

/// One progression-pace milestone over seeds (plan §5.0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pace {
    pub reached: f32,
    pub ticks: f32,
}

pub fn summarise(runs: &[RunMetrics]) -> Summary {
    let n = runs.len().max(1) as f32;
    let avg = |f: &dyn Fn(&RunMetrics) -> f32| runs.iter().map(f).sum::<f32>() / n;
    Summary {
        runs: runs.len() as u32,
        survival: avg(&|r| f32::from(u8::from(r.survived))),
        active: avg(&|r| r.active),
        self_started: avg(&|r| r.self_started),
        mean_c: avg(&|r| r.mean_c),
        mean_d: avg(&|r| r.mean_d),
        mean_v: avg(&|r| r.mean_v),
        freeze: avg(&|r| r.freeze),
        stuck_kicks: avg(&|r| r.stuck_kicks as f32),
        reflexes: avg(&|r| r.reflexes as f32),
        new_things: avg(&|r| r.new_things as f32),
        tech: avg(&|r| r.tech as f32),
        memory: avg(&|r| r.memory),
        recall: avg(&|r| r.recall),
        thinking: avg(&|r| r.thinking),
        plan_lag: finite_mean(runs.iter().map(|r| r.plan_lag)),
        shelter: avg(&|r| f32::from(u8::from(r.shelter_at.is_some()))),
        pace: std::array::from_fn(|i| Pace {
            reached: avg(&|r| f32::from(u8::from(r.pace[i].is_some()))),
            ticks: finite_mean(
                runs.iter()
                    .map(|r| r.pace[i].map_or(f32::NAN, |t| t as f32)),
            ),
        }),
        recovery: finite_mean(runs.iter().map(|r| r.recovery)),
        recovered: {
            let all: u32 = runs.iter().map(|r| r.setbacks).sum();
            let back: u32 = runs.iter().map(|r| r.recovered).sum();
            back as f32 / all as f32
        },
    }
}

/// Mean of the finite values; NaN when there are none.
fn finite_mean(xs: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = xs
        .filter(|x| x.is_finite())
        .fold((0.0, 0u32), |(s, n), x| (s + x, n + 1));
    if n == 0 { f32::NAN } else { sum / n as f32 }
}

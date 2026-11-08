//! The brain loop (spec §4): one `Brain` per agent, wiring the 01 modules
//! together. The host never sees inside; it gets a [`Decision`] and reports
//! the [`Outcome`] back.
//!
//! Timing: a decision is one cognitive tick. Continuous dynamics (C, V, D,
//! π, fatigue, TM decay, the subconscious budget) are integrated over the
//! world ticks that passed since the last decision, so a 40-tick chop counts
//! as 40 ticks of drift and tiredness.

mod choose;
pub mod config;
pub mod digest;
mod learn;
pub mod log;
mod perceive;
pub mod think;

#[cfg(test)]
mod tests;

pub use config::{BrainConfig, Horizon, LensPrior};
pub use log::TickLog;
pub use think::{NoTools, ThinkTools};

use crate::activation::signals::{LearningProgress, NoveltyWindow};
use crate::activation::{Affect, Curiosity, PowerActivation, Signals};
use crate::body::Fatigue;
use crate::control::development::Development;
use crate::control::episodes::TenacityTable;
use crate::control::kick::StuckDetector;
use crate::control::oracle::{CONTEXT_DIM, Context, Oracle};
use crate::control::permeability::Permeability;
use crate::control::subconscious::Subconscious;
use crate::direction::{PlanStep, ThoughtMomentum};
use crate::memory::{InternalAction, MemoryStore, SkillTable};
use crate::modes::{Features, Lens, Mode};
use host_api::{Candidate, Intensity, Inventory, Observation, Outcome, Skill, TaskType};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The active goal (spec §13). Player commands come first (01 §4.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub item: String,
    pub commanded: bool,
}

/// What the brain chose this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Choice {
    /// Index into the host's candidate list.
    External(usize),
    Internal(InternalAction),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    /// Nothing started this tick: g(C) too low (01 §1.4) and no command.
    Idle,
    /// Run this external candidate with this intensity.
    Act(usize, Intensity),
    /// An internal action ran inside the brain; the world moves on one tick.
    Thought(InternalAction),
}

/// Context indices in x(S) (spec §8).
pub(crate) mod ctx {
    pub const BIAS: usize = 0;
    pub const TASK0: usize = 1;
    pub const COMPETENCE: usize = 8;
    pub const RETRIEVAL_HIT: usize = 9;
    pub const STAKES: usize = 10;
    pub const THREAT: usize = 11;
    pub const ENERGY: usize = 12;
    pub const NOVELTY: usize = 13;
    pub const URGENCY: usize = 14;
    pub const FATIGUE: usize = 15;
}

/// A plan goes stale after this many ticks even if nothing changed, so the
/// planner sees dusk and hunger (proposed).
pub const PLAN_TTL: u64 = 200;

/// Action type id of an idle tick, for the stuck detector.
pub const IDLE_KIND: u32 = 98;

/// Magnitude of the routing priors on Θ (spec §8, proposed).
pub const ROUTING_PRIOR: f32 = 0.5;

/// Θ priors: the archetype's bias column plus 00's routing claims (spec §8).
pub fn oracle_priors(bias: [f32; 4]) -> [Context; 4] {
    let (l, e, p) = (
        Mode::Learning.index(),
        Mode::Exploring.index(),
        Mode::Performing.index(),
    );
    let mut th = [[0.0; CONTEXT_DIM]; 4];
    for (row, b) in th.iter_mut().zip(bias) {
        row[ctx::BIAS] = b;
    }
    for c in [ctx::RETRIEVAL_HIT, ctx::COMPETENCE] {
        th[p][c] += ROUTING_PRIOR;
        th[l][c] -= ROUTING_PRIOR;
    }
    th[p][ctx::STAKES] += ROUTING_PRIOR;
    th[p][ctx::THREAT] += ROUTING_PRIOR;
    th[e][ctx::THREAT] -= ROUTING_PRIOR;
    th
}

/// One executed action, remembered for `reflect` (spec §6).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EpisodeRecord {
    pub mem_id: usize,
    pub skill: String,
    pub target: Option<usize>,
    pub success: bool,
    pub gained: Vec<usize>,
}

/// The decision waiting for its outcome.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub tick: u64,
    pub choice: Choice,
    pub phi: Features,
    /// True (unflipped) scores, for learning (01 §4.3).
    pub m: [f32; 4],
    pub w: [f32; 4],
    pub x: Context,
    pub task: TaskType,
    pub target: Option<String>,
    pub route: Option<(String, f32)>,
    pub skill: Option<Skill>,
    pub skill_label: String,
    pub progress_before: f32,
    pub hazard_vetoed: bool,
    pub tags: u32,
    /// Filled immediately for internal actions.
    pub internal: Option<think::ThoughtResult>,
    pub outcome: Option<Outcome>,
}

pub struct Brain {
    pub config: BrainConfig,
    pub(crate) rng: ChaCha8Rng,
    pub(crate) curiosity: Curiosity,
    pub(crate) affect: Affect,
    pub(crate) act: PowerActivation,
    pub(crate) fatigue: Fatigue,
    pub(crate) tm: ThoughtMomentum,
    pub(crate) lenses: [Lens; 4],
    pub(crate) oracle: Oracle,
    pub(crate) perm: Permeability,
    pub(crate) tenacity: TenacityTable,
    pub(crate) dev: Development,
    pub(crate) stuck: StuckDetector,
    pub(crate) sub: Subconscious,
    pub(crate) mem: MemoryStore,
    pub(crate) skills: SkillTable,
    pub(crate) novelty: NoveltyWindow,
    pub(crate) lp: LearningProgress,
    pub(crate) episodes: VecDeque<EpisodeRecord>,
    pub(crate) goal: Option<Goal>,
    pub(crate) step: Option<PlanStep>,
    pub(crate) plan_inv: Option<Inventory>,
    pub(crate) plan_tick: u64,
    pub(crate) routine_bonus: [f32; 7],
    pub(crate) r_hist: VecDeque<f32>,
    pub(crate) damage: VecDeque<(u64, f32)>,
    pub(crate) c_long: f32,
    pub(crate) last_tick: Option<u64>,
    /// Hidden lifespan and the agent's noisy estimate of it (spec §15).
    pub(crate) lifespan: Option<(f32, f32)>,
    pub(crate) last_voluntary: Option<(u64, Features, f32)>,
    pub(crate) pending: Option<Pending>,
    pub(crate) last_task: TaskType,
    pub(crate) last_effort: f32,
    /// Oracle weights and action type of the last decision (stuck detector).
    pub(crate) last_w: [f32; 4],
    pub(crate) last_kind: u32,
    pub(crate) last_external: bool,
    pub(crate) signals: Signals,
    pub(crate) c_target: f32,
    pub(crate) action_tags: Vec<u32>,
    pub(crate) log: TickLog,
}

/// A standard normal draw (Box–Muller), from the brain's own seeded stream.
pub(crate) fn normal(rng: &mut ChaCha8Rng) -> f32 {
    let u1: f32 = rng.random::<f32>().max(f32::MIN_POSITIVE);
    let u2: f32 = rng.random();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

impl Brain {
    pub fn new(config: BrainConfig, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut lenses = Mode::ALL.map(Lens::with_priors);
        for p in &config.lens_priors {
            lenses[p.mode].theta[p.feature] = p.weight;
        }
        for (i, l) in lenses.iter_mut().enumerate() {
            l.frozen = config.lens_frozen[i];
        }
        let g = config.tenacity_gamma;
        let tenacity = TenacityTable::new(config.tenacity_ab.map(|(a, b)| (a, b, g)));
        let oracle = Oracle {
            theta: oracle_priors(config.oracle_bias),
            floor: config.oracle_floor,
            eta: config.oracle_eta,
            ..Oracle::default()
        };
        let lifespan = config.horizon.map(|h| {
            let hidden = (h.ticks * (1.0 + 0.1 * normal(&mut rng))).max(1.0);
            let estimate = hidden + 0.15 * h.ticks * normal(&mut rng);
            (hidden, estimate)
        });
        Self {
            curiosity: Curiosity::new(config.curiosity.clone()),
            affect: Affect::new(config.affect.clone()),
            act: PowerActivation {
                alpha: config.alpha,
            },
            fatigue: Fatigue::default(),
            tm: ThoughtMomentum::new(config.tm_k, config.tm_tau),
            lenses,
            oracle,
            perm: Permeability::new(config.permeability.clone()),
            tenacity,
            dev: Development::new(config.rho_pre),
            stuck: StuckDetector::new(config.kick.n),
            sub: Subconscious::new(config.subconscious.clone()),
            mem: MemoryStore::new(config.memory.clone()),
            skills: SkillTable::default(),
            novelty: NoveltyWindow::default(),
            lp: LearningProgress::default(),
            episodes: VecDeque::new(),
            goal: None,
            step: None,
            plan_inv: None,
            plan_tick: 0,
            routine_bonus: [0.0; 7],
            r_hist: VecDeque::new(),
            damage: VecDeque::new(),
            c_long: config.curiosity.base,
            last_tick: None,
            lifespan,
            last_voluntary: None,
            pending: None,
            last_task: TaskType::Travel,
            last_effort: 0.0,
            last_w: [0.25; 4],
            last_kind: IDLE_KIND,
            last_external: false,
            signals: Signals::default(),
            c_target: config.curiosity.base,
            action_tags: Vec::new(),
            log: TickLog::default(),
            rng,
            config,
        }
    }

    pub fn c(&self) -> f32 {
        self.curiosity.value()
    }

    pub fn d(&self) -> f32 {
        self.affect.d()
    }

    pub fn v(&self) -> f32 {
        self.affect.v()
    }

    pub fn pi(&self) -> f32 {
        self.perm.value()
    }

    pub fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    pub fn memory(&self) -> &MemoryStore {
        &self.mem
    }

    pub fn lens(&self, m: Mode) -> &Lens {
        &self.lenses[m.index()]
    }

    pub fn oracle(&self) -> &Oracle {
        &self.oracle
    }

    /// The hidden lifespan in ticks, if this run has a horizon (spec §15).
    pub fn lifespan(&self) -> Option<u64> {
        self.lifespan.map(|(h, _)| h as u64)
    }

    /// The log row for the last step (spec §25).
    pub fn last_log(&self) -> &TickLog {
        &self.log
    }

    /// Set a goal directly (scenarios and tests; players use chat).
    pub fn set_goal(&mut self, goal: Option<Goal>) {
        self.goal = goal;
        self.step = None;
        self.plan_inv = None;
    }

    /// Force internal state for the 01 §1.5 reproductions and tests.
    pub fn force_state(&mut self, c: Option<f32>, v_d: Option<(f32, f32)>) {
        if let Some(c) = c {
            self.curiosity = Curiosity::new(self.config.curiosity.clone());
            let now = self.curiosity.value();
            self.curiosity.impulse(c - now, 0.0);
        }
        if let Some((v, d)) = v_d {
            self.affect.force(v, d);
        }
    }

    /// One cognitive tick (spec §4): learn from the last decision, sense,
    /// then choose. Run internal actions inside; return the decision.
    pub fn step(
        &mut self,
        obs: &Observation,
        cands: &[Candidate],
        tools: &mut dyn ThinkTools,
    ) -> Decision {
        let reflex = std::mem::take(&mut self.log.reflex);
        self.log = TickLog::default();
        self.log.reflex = reflex;
        self.learn(obs, tools);
        let s = self.sense(obs);
        let d = self.choose(obs, cands, &s, tools);
        self.fill_state(obs, &s, cands);
        d
    }

    /// Report the outcome of the external action from the last decision.
    pub fn outcome(&mut self, out: Outcome) {
        if let Some(p) = self.pending.as_mut() {
            p.outcome = Some(out);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_obs() -> Observation {
    Observation {
        agent: host_api::AgentId(0),
        tick: 0,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: host_api::Pos::default(),
        inventory: Inventory::default(),
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

//! One wide log row per decision (spec §25). The brain fills what it knows;
//! the runner adds world-only fields and writes JSONL. Fields are only ever
//! added, never repurposed (spec §31.6).

use super::choose::{Scored, preferences};
use super::perceive::Sensed;
use super::{Brain, Choice, Pending};
use crate::control::development::Stage;
use crate::modes::{Mode, f_sign};
use host_api::{Candidate, Observation, SeenKind, Target};
use serde::Serialize;

/// One of the top-5 candidates (spec §25).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CandLog {
    pub action: String,
    pub score: f32,
    pub m: [f32; 4],
    pub phi: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TickLog {
    pub tick: u64,
    pub dt: u64,
    // activation and control
    pub c: f32,
    pub c_target: f32,
    pub g: f32,
    pub d: f32,
    pub v: f32,
    pub pi: f32,
    pub fatigue: f32,
    pub novelty: f32,
    pub learning_progress: f32,
    pub energy: f32,
    pub threat: f32,
    pub habituation: f32,
    pub social: f32,
    pub rho: f32,
    pub rho_eff: f32,
    pub stage: u8,
    pub u: f32,
    // choice
    pub tm: Vec<usize>,
    pub retrieved: usize,
    pub retrieval_hit: bool,
    pub w: [f32; 4],
    pub theta_x: [f32; 4],
    pub top5: Vec<CandLog>,
    pub n_choices: usize,
    pub kicks: u32,
    pub stuck_kick: bool,
    pub chosen: String,
    pub chosen_score: f32,
    /// 1 − |Σ w·f(m)| / Σ |w·f(m)| for the chosen action: 1 means the
    /// modes cancel each other exactly (freeze, 01 §1.5).
    pub conflict: f32,
    /// Largest |w·f(m)| among the modes for the chosen action.
    pub pull: f32,
    pub effort: f32,
    pub focus: f32,
    pub external_drive: bool,
    // outcome of the previous decision
    pub prev_action: String,
    pub success: [f32; 4],
    pub r: f32,
    pub d_goal: f32,
    pub credited: String,
    pub tenacity: [f32; 4],
    pub creative: bool,
    pub new_items: u32,
    // memory
    pub mem_items: usize,
    pub formed: u32,
    pub pruned: u32,
    pub sub_processed: u32,
    pub sigma_tiers: Option<[f32; 4]>,
    pub richness: f32,
    pub llm_calls: u32,
    // goal
    pub goal: Option<String>,
    pub goal_commanded: bool,
    pub plan_step: Option<String>,
    // body and survival context (spec §20.1)
    pub health: f32,
    pub food: f32,
    pub x: i32,
    pub z: i32,
    pub sheltered: bool,
    pub reflex: bool,
    pub nearest_hostile: Option<f32>,
    pub hostiles: u32,
    pub ticks_to_night: u64,
    pub food_items: u32,
}

/// Bit layout for action tags (spec §18): skill, target kind, task, night.
pub(crate) fn action_tags(c: &Candidate, target: Option<&str>, obs: &Observation) -> u32 {
    let mut t = 1u32 << (c.skill as u32);
    let kind = match (target, &c.target) {
        (Some(k), _) if k.starts_with("block:") => 0,
        (Some(k), _) if k.starts_with("entity:") => 1,
        (Some(k), _) if k.starts_with("item:") => 2,
        (Some(k), _) if k.starts_with("region:") => 3,
        (_, Some(Target::Player(_))) => 4,
        _ => 5,
    };
    t |= 1 << (14 + kind);
    t |= 1 << (20 + c.task_type.index() as u32);
    if obs.is_night() {
        t |= 1 << 27;
    }
    if obs.nearest_hostile().is_some() {
        t |= 1 << 28;
    }
    t
}

const FOOD: [&str; 2] = ["porkchop", "bread"];

impl TickLog {
    pub(crate) fn fill_choice(
        &mut self,
        all: &[Scored],
        scores: &[f32],
        w: &[f32; 4],
        kicks: usize,
        stuck: bool,
        pick: Option<usize>,
    ) {
        self.w = *w;
        self.kicks = kicks as u32;
        self.stuck_kick = stuck;
        self.n_choices = all.len();
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
        self.top5 = order
            .iter()
            .take(5)
            .map(|&i| CandLog {
                action: label(all[i].choice),
                score: scores[i],
                m: all[i].m,
                phi: all[i].phi.to_vec(),
            })
            .collect();
        match pick {
            Some(k) => {
                self.chosen = label(all[k].choice);
                self.chosen_score = scores[k];
                let parts: Vec<f32> = (0..4).map(|i| w[i] * f_sign(all[k].kicked[i])).collect();
                let abs: f32 = parts.iter().map(|p| p.abs()).sum();
                self.pull = parts.iter().map(|p| p.abs()).fold(0.0, f32::max);
                self.conflict = if abs > 0.0 {
                    1.0 - parts.iter().sum::<f32>().abs() / abs
                } else {
                    0.0
                };
            }
            None => self.chosen = "idle".into(),
        }
    }

    pub(crate) fn fill_learning(&mut self, p: &Pending, l: &Learned) {
        self.prev_action = p.skill_label.clone();
        self.success = l.success;
        self.r = l.r;
        self.d_goal = l.d_goal;
        self.credited = format!("{:?}", l.credited);
        self.tenacity = l.tenacity;
        self.creative = l.creative;
        self.new_items = l.new_items;
    }
}

/// What learning from one outcome produced, for the log row (spec §25).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Learned {
    pub success: [f32; 4],
    pub r: f32,
    pub d_goal: f32,
    pub credited: Mode,
    pub tenacity: [f32; 4],
    pub creative: bool,
    pub new_items: u32,
}

fn label(c: Choice) -> String {
    match c {
        Choice::External(i) => format!("ext:{i}"),
        Choice::Internal(a) => format!("{a:?}"),
    }
}

impl Brain {
    /// State fields, filled after the decision (spec §25).
    pub(crate) fn fill_state(&mut self, obs: &Observation, s: &Sensed, cands: &[Candidate]) {
        let tm = self.tm.top();
        // Distinct semantic items in TM + retrieved, ÷ k (spec §18).
        let semantic = tm
            .iter()
            .chain(&s.retrieved)
            .filter(|&&i| {
                self.mem.is_alive(i) && self.mem.entry(i).item.tier == crate::memory::Tier::Semantic
            })
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let x = self.pending.as_ref().map(|p| p.x);
        let l = &mut self.log;
        if let Some(i) = l
            .chosen
            .strip_prefix("ext:")
            .and_then(|i| i.parse::<usize>().ok())
        {
            l.chosen = format!("{:?}", cands[i].skill);
            if let Some(t) = &cands[i].target {
                l.chosen = format!("{}:{}", l.chosen, short_target(t));
            }
        }
        for c in &mut l.top5 {
            if let Some(i) = c
                .action
                .strip_prefix("ext:")
                .and_then(|i| i.parse::<usize>().ok())
            {
                c.action = format!("{:?}", cands[i].skill);
            }
        }
        l.tick = obs.tick;
        l.dt = s.dt;
        l.c = self.curiosity.value();
        l.c_target = self.c_target;
        l.g = self.act.g(l.c);
        l.d = self.affect.d();
        l.v = self.affect.v();
        l.pi = self.perm.value();
        l.fatigue = self.fatigue.value();
        let sg = self.signals;
        (l.novelty, l.learning_progress, l.energy) = (sg.novelty, sg.learning_progress, sg.energy);
        (l.threat, l.habituation, l.social) = (sg.threat, sg.habituation, sg.social);
        l.rho = self.dev.rho();
        l.rho_eff = self.dev.rho_eff();
        l.stage = match self.dev.stage() {
            Stage::Initialization => 1,
            Stage::RapidDevelopment => 2,
            Stage::Maturation => 3,
        };
        l.u = s.u;
        l.tm = tm;
        l.retrieved = s.retrieved.len();
        l.retrieval_hit = s.retrieval_hit;
        if let Some(x) = x {
            l.theta_x = preferences(&self.oracle, &x);
        }
        l.mem_items = self.mem.len();
        l.formed = s.formed;
        l.pruned = s.pruned;
        l.sub_processed = s.sub_processed;
        if obs.tick / 100 != obs.tick.saturating_sub(s.dt) / 100 || obs.tick == 0 {
            l.sigma_tiers = Some(self.mem.strength_by_tier(obs.tick));
        }
        l.richness = semantic as f32 / self.config.tm_k.max(1) as f32;
        l.goal = self.goal.as_ref().map(|g| g.item.clone());
        l.goal_commanded = self.goal.as_ref().is_some_and(|g| g.commanded);
        l.plan_step = self
            .step
            .as_ref()
            .map(|p| format!("{:?}:{}", p.skill, p.thing));
        l.health = obs.health;
        l.food = obs.food;
        l.x = obs.position.x;
        l.z = obs.position.z;
        l.sheltered = obs.sheltered;
        l.nearest_hostile = obs.nearest_hostile();
        l.hostiles = obs
            .visible
            .iter()
            .filter(|v| matches!(v.kind, SeenKind::Entity { hostile: true }))
            .count() as u32;
        l.ticks_to_night = ticks_to_night(obs.day_phase);
        l.food_items = FOOD.iter().map(|f| obs.inventory.count(f)).sum();
    }
}

fn short_target(t: &Target) -> String {
    match t {
        Target::Pos(p) => format!("{},{}", p.x, p.z),
        Target::Thing(t) => t.0.clone(),
        Target::Item(i) => i.clone(),
        Target::Player(p) => p.clone(),
    }
}

/// Day length assumed for "ticks until night" (spec §21: 2,400).
pub const DAY_TICKS: f32 = 2400.0;

/// Ticks until the phase reaches 0.5 (0 at night).
pub fn ticks_to_night(phase: f32) -> u64 {
    if phase >= 0.5 {
        0
    } else {
        ((0.5 - phase) * DAY_TICKS).round() as u64
    }
}

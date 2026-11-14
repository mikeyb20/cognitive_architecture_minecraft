//! The sim's skills (spec §2, §21), one handler per group: each offers its
//! candidates and runs the skills it owns. Handlers are asked for
//! candidates in table order, and that order is part of the contract (the
//! brain's choices and the golden traces depend on it). Paid debt D2.

use host_api::{Candidate, Intensity, Pos, Skill, Target, TaskType};

use crate::world::{MAX_CANDIDATES, Reach, SimWorld};

mod crafting;
mod eating;
mod gathering;
mod hunting;
mod sheltering;
mod threats;
mod travelling;

/// What running an action did: (success, progress, blocked).
pub(crate) type Exec = (bool, f32, bool);

/// The action could not start.
pub(crate) const BLOCKED: Exec = (false, 0.0, true);

/// What every handler sees when offering candidates.
pub(crate) struct Ctx {
    pub me: Pos,
    pub sheltered: bool,
    pub reach: Reach,
}

/// The candidate list being built.
#[derive(Default)]
pub(crate) struct Out(Vec<Candidate>);

impl Out {
    /// Add a candidate; cost and risk are clamped to 0..1.
    pub fn add(
        &mut self,
        skill: Skill,
        target: Option<Target>,
        task: TaskType,
        cost: f32,
        risk: f32,
    ) {
        self.0.push(Candidate {
            id: self.0.len() as u32,
            skill,
            target,
            task_type: task,
            est_cost: cost.clamp(0.0, 1.0),
            est_risk: risk.clamp(0.0, 1.0),
            commanded: false,
        });
    }
}

/// One group of skills.
pub(crate) trait Handler: Sync {
    /// Offer this group's candidates.
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out);
    /// Whether this handler runs `skill`.
    fn owns(&self, _skill: Skill) -> bool {
        false
    }
    /// Run a candidate whose skill this handler owns.
    fn execute(&self, _w: &mut SimWorld, _a: &Candidate, _it: Intensity) -> Exec {
        BLOCKED
    }
}

/// The handlers, in candidate order.
const HANDLERS: [&dyn Handler; 7] = [
    &threats::Threats,
    &eating::Eating,
    &sheltering::Sheltering,
    &gathering::Gathering,
    &hunting::Hunting,
    &crafting::Crafting,
    &travelling::Travelling,
];

/// Every candidate on offer now (spec §6).
pub(crate) fn candidates(w: &SimWorld) -> Vec<Candidate> {
    let cx = Ctx {
        me: w.body.pos,
        sheltered: w.sheltered(),
        reach: w.reach(),
    };
    let mut out = Out::default();
    for h in HANDLERS {
        h.candidates(w, &cx, &mut out);
    }
    // The sim can offer at most 19 (see the `fullest` test), within K.
    debug_assert!(out.0.len() <= MAX_CANDIDATES);
    out.0
}

/// Run an action with the handler that owns its skill. Skills the sim does
/// not have (place, follow, chat) are blocked.
pub(crate) fn execute(w: &mut SimWorld, a: &Candidate, it: Intensity) -> Exec {
    HANDLERS
        .iter()
        .find(|h| h.owns(a.skill))
        .map_or(BLOCKED, |h| h.execute(w, a, it))
}

/// The candidate's target position, if it has one.
pub(crate) fn target_pos(a: &Candidate) -> Option<Pos> {
    match &a.target {
        Some(Target::Pos(p)) => Some(*p),
        _ => None,
    }
}

/// The candidate's target item, if it has one.
pub(crate) fn target_item(a: &Candidate) -> Option<&str> {
    match &a.target {
        Some(Target::Item(s)) => Some(s),
        _ => None,
    }
}

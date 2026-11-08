//! Internal actions (spec §6; 00 §2.2) and the tools they call. The LLM is a
//! tool here, never the decider (#40): `ThinkTools` is implemented by the
//! scripted planner in the sim and by an LLM tier later. Reflect, imagine,
//! rehearse and consolidate have code versions in the brain itself (the
//! template reflector and the recombiner, spec §21).

use super::{Brain, Goal};
use crate::direction::PlanStep;
use crate::memory::{InternalAction, PriorityInputs, Source, Tier};
use host_api::{Observation, TaskType};

/// Tools the internal actions call (spec §6, §23).
pub trait ThinkTools {
    /// Next step toward the goal item, or `None` if there is no route.
    fn plan(&mut self, goal: &str, obs: &Observation) -> Option<PlanStep>;
    /// Goal progress in [0, 1] (spec §13).
    fn progress(&mut self, goal: &str, obs: &Observation) -> f32;
    /// An own goal when idle (01 §4.6, via `imagine`).
    fn propose_goal(&mut self, obs: &Observation) -> Option<String>;
}

/// No tools at all: plans and goals never come.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTools;

impl ThinkTools for NoTools {
    fn plan(&mut self, _: &str, _: &Observation) -> Option<PlanStep> {
        None
    }
    fn progress(&mut self, _: &str, _: &Observation) -> f32 {
        0.0
    }
    fn propose_goal(&mut self, _: &Observation) -> Option<String> {
        None
    }
}

/// What an internal action did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThoughtResult {
    pub success: bool,
    pub new_items: u32,
    pub new_linked: u32,
    pub o_decision: bool,
    pub pruned: u32,
}

/// Internal depth: items processed for effort e is `1 + ⌊9e⌋` (spec §16).
pub fn depth(effort: f32) -> usize {
    1 + (9.0 * effort.clamp(0.0, 1.0)).floor() as usize
}

/// Routine boost from one rehearsal, which lowers π for that task (spec §11).
pub const REHEARSAL: f32 = 1.0;

impl Brain {
    pub(crate) fn think(
        &mut self,
        a: InternalAction,
        effort: f32,
        obs: &Observation,
        tools: &mut dyn ThinkTools,
    ) -> ThoughtResult {
        let n = depth(effort);
        match a {
            InternalAction::Reflect => self.reflect(n, obs.tick),
            InternalAction::Imagine => self.imagine(obs, tools),
            InternalAction::Rehearse => {
                let task = self.step.as_ref().map(|s| s.task);
                if let Some(t) = task {
                    self.routine_bonus[t.index()] = REHEARSAL;
                }
                ThoughtResult {
                    success: task.is_some(),
                    ..ThoughtResult::default()
                }
            }
            InternalAction::Consolidate => {
                let budget = n as f32 * self.config.subconscious.b_base;
                let r = self.sub.run(&mut self.mem, budget, obs.tick);
                ThoughtResult {
                    success: true,
                    o_decision: r.pruned + r.reinforced > 0,
                    pruned: r.pruned,
                    ..ThoughtResult::default()
                }
            }
            InternalAction::Plan => {
                let step = self.goal.as_ref().and_then(|g| tools.plan(&g.item, obs));
                self.plan_inv = step.as_ref().map(|_| obs.inventory.clone());
                self.plan_tick = obs.tick;
                let ok = step.is_some();
                if let Some(s) = &step {
                    self.last_task = s.task;
                }
                self.step = step;
                ThoughtResult {
                    success: ok,
                    ..ThoughtResult::default()
                }
            }
        }
    }

    /// Template reflector: each unreflected episode becomes a semantic fact
    /// linked to its target and to what it gained (spec §6 `reflect`).
    fn reflect(&mut self, n: usize, now: u64) -> ThoughtResult {
        let mut r = ThoughtResult::default();
        let todo: Vec<usize> = (0..self.episodes.len())
            .rev()
            .filter(|&i| !self.mem.entry(self.episodes[i].mem_id).reflected)
            .take(n)
            .collect();
        for i in todo {
            let ep = self.episodes[i].clone();
            self.mem.entry_mut(ep.mem_id).reflected = true;
            let target = ep
                .target
                .filter(|&t| self.mem.is_alive(t))
                .map(|t| self.mem.entry(t).item.content.clone())
                .unwrap_or_else(|| "none".into());
            let fact = format!(
                "fact:{}:{}:{}",
                ep.skill,
                target,
                if ep.success { "ok" } else { "fail" }
            );
            let x = PriorityInputs {
                v_enc: self.mem.entry(ep.mem_id).item.v_enc,
                c_enc: self.c(),
                ..PriorityInputs::default()
            };
            let (id, new) = self
                .mem
                .encode(&fact, Tier::Semantic, x, now, Source::Reflection);
            if new {
                r.new_items += 1;
            }
            let mut linked = false;
            for other in ep.target.into_iter().chain(ep.gained.iter().copied()) {
                if self.mem.is_alive(other) {
                    self.mem.link(id, other, crate::memory::store::LINK_STEP);
                    linked = true;
                }
            }
            if new && linked {
                r.new_linked += 1;
            }
            r.success = true;
        }
        r
    }

    /// Code recombiner: with no goal, propose one; otherwise join the first
    /// unlinked pair of semantic items in TM into a new idea (spec §6
    /// `imagine`).
    fn imagine(&mut self, obs: &Observation, tools: &mut dyn ThinkTools) -> ThoughtResult {
        let mut r = ThoughtResult::default();
        if self.goal.is_none() {
            if let Some(item) = tools.propose_goal(obs) {
                let x = PriorityInputs {
                    c_enc: self.c(),
                    ..PriorityInputs::default()
                };
                let (id, new) = self.mem.encode(
                    &format!("goal:{item}"),
                    Tier::Semantic,
                    x,
                    obs.tick,
                    Source::Reflection,
                );
                if let Some(it) = self.mem.get(&format!("item:{item}")) {
                    self.mem.link(id, it, crate::memory::store::LINK_STEP);
                    r.new_linked += u32::from(new);
                }
                r.new_items += u32::from(new);
                self.set_goal(Some(Goal {
                    item,
                    commanded: false,
                }));
                r.success = true;
            }
            return r;
        }
        if let Some((a, b, idea)) = self.unlinked_pair() {
            let x = PriorityInputs {
                c_enc: self.c(),
                connection: 0.4,
                ..PriorityInputs::default()
            };
            let (id, _) = self
                .mem
                .encode(&idea, Tier::Semantic, x, obs.tick, Source::Reflection);
            self.mem.link(id, a, crate::memory::store::LINK_STEP);
            self.mem.link(id, b, crate::memory::store::LINK_STEP);
            r = ThoughtResult {
                success: true,
                new_items: 1,
                new_linked: 1,
                ..ThoughtResult::default()
            };
        }
        r
    }

    /// First pair of live semantic TM items with no link and no stored idea.
    pub(crate) fn unlinked_pair(&self) -> Option<(usize, usize, String)> {
        let tm: Vec<usize> = self
            .tm
            .top()
            .into_iter()
            .filter(|&i| self.mem.is_alive(i) && self.mem.entry(i).item.tier == Tier::Semantic)
            .collect();
        for (i, &a) in tm.iter().enumerate() {
            for &b in &tm[i + 1..] {
                if self.mem.linked(a, b) {
                    continue;
                }
                let (ka, kb) = (
                    &self.mem.entry(a).item.content,
                    &self.mem.entry(b).item.content,
                );
                let idea = format!("idea:{ka}+{kb}");
                if self.mem.get(&idea).is_none_or(|id| !self.mem.is_alive(id)) {
                    return Some((a, b, idea));
                }
            }
        }
        None
    }

    /// The task the agent is working on: the plan step's, else the last one.
    pub(crate) fn context_task(&self) -> TaskType {
        self.step.as_ref().map_or(self.last_task, |s| s.task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_scales_with_effort() {
        assert_eq!(depth(0.0), 1);
        assert_eq!(depth(0.5), 5);
        assert_eq!(depth(1.0), 10);
        assert_eq!(depth(2.0), 10);
    }

    #[test]
    fn no_tools_never_plan() {
        let o = crate::agent::test_obs();
        let mut t = NoTools;
        assert_eq!(t.plan("x", &o), None);
        assert_eq!(t.progress("x", &o), 0.0);
        assert_eq!(t.propose_goal(&o), None);
    }
}

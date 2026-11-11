//! Spec §4 lines 16–18: each mode's success, meta-reward R, and learning:
//! lenses (from the unflipped scores), Oracle, tenacity, memory, ρ and age.

use super::log::Learned;
use super::think::ThinkTools;
use super::{Brain, Choice, EpisodeRecord, Pending};
use crate::control::episodes::EpisodeTick;
use crate::control::oracle::Oracle;
use crate::control::reward::meta_reward;
use crate::control::time::{age, plasticity};
use crate::derived::is_creative;
use crate::memory::store::LINK_STEP;
use crate::memory::{ActionKey, PriorityInputs, Source, Tier};
use crate::modes::{Mode, ModeNetwork, OutcomeView, Thresholds, all_success};
use host_api::{Observation, Outcome, Skill};

/// Window for "useful" memories: in TM within 20 ticks before R > 0 (spec §14).
pub(crate) const USEFUL_WINDOW: u64 = 20;
/// An emergency reflex within this many ticks teaches O (spec §20, #42).
pub(crate) const REFLEX_WINDOW: u64 = 100;
/// Kept episode records for `reflect`.
pub(crate) const EPISODES_KEPT: usize = 50;
/// Creativity threshold θ_cr (spec §18).
pub(crate) const THETA_CREATIVE: f32 = 0.6;

impl Brain {
    /// Learn from the decision made at the previous step.
    pub(crate) fn learn(&mut self, obs: &Observation, tools: &mut dyn ThinkTools) {
        let Some(p) = self.pending.take() else {
            return;
        };
        let view = match p.choice {
            Choice::External(_) => {
                let out = p.outcome.clone().unwrap_or(Outcome {
                    blocked: true,
                    ..Outcome::default()
                });
                self.learn_relief(&p, &out, obs);
                self.learn_lesson(&p, &out);
                self.write_external(&p, &out, obs)
            }
            Choice::Internal(a) => {
                let r = p.internal.unwrap_or_default();
                self.skills
                    .record(ActionKey::from(a), p.task, r.success, obs.tick);
                OutcomeView {
                    success: r.success,
                    progress: f32::from(u8::from(r.success)),
                    new_items: r.new_items,
                    new_linked: r.new_linked,
                    o_decision: r.o_decision,
                    internal: Some(a),
                    ..OutcomeView::default()
                }
            }
        };
        let rho = self.dev.rho_eff();
        let thr = Thresholds {
            l: lerp(self.config.thr_l, rho),
            e: lerp(self.config.thr_e, rho),
            p: lerp(self.config.thr_p, rho),
        };
        let success = all_success(&view, &thr);
        let progress_now = self
            .goal
            .as_ref()
            .map_or(0.0, |g| tools.progress(&g.item, obs));
        let d_goal = progress_now - p.progress_before;
        let goal_done = self.goal.is_some() && progress_now >= 1.0;
        if goal_done {
            self.set_goal(None);
        }
        let r = meta_reward(&p.w, &success, d_goal, self.config.k_goal);
        self.push_r(r);
        self.dev.record_experience();
        self.learn_lenses(&p, &success, obs.tick);
        let credited = Oracle::credited(&p.w, &p.m);
        let ten = self.tenacity.for_task(p.task);
        self.oracle.update(&p.x, &p.w, credited, r, &ten);
        self.tenacity.step(EpisodeTick {
            tick: obs.tick,
            task: p.task,
            credited,
            w: p.w,
            success,
            d_goal,
            goal_done,
        });
        if r > 0.0 {
            self.mark_useful(obs.tick);
        }
        let creative = self.creative(&p);
        if matches!(p.choice, Choice::External(_)) {
            let o = Mode::Optimizing.index();
            self.last_voluntary = Some((p.tick, p.phi, p.m[o]));
        }
        let learned = Learned {
            success,
            r,
            d_goal,
            credited,
            tenacity: ten,
            creative,
            new_items: view.new_items,
        };
        self.log.fill_learning(&p, &learned);
    }

    /// R̄_20 keeps the last 20 decisions' R (spec §12).
    pub(crate) fn push_r(&mut self, r: f32) {
        self.r_hist.push_back(r);
        if self.r_hist.len() > 20 {
            self.r_hist.pop_front();
        }
    }

    fn learn_lenses(&mut self, p: &Pending, success: &[f32; 4], now: u64) {
        let a = age(
            self.config.age_per_tick,
            now,
            self.config.age_per_experience,
            self.dev.experiences(),
        );
        let s_lr = self.dev.lr_multiplier();
        for (i, lens) in self.lenses.iter_mut().enumerate() {
            let base = lens.params.base_lr * self.config.lens_lr[i];
            let lr = plasticity(base, self.config.aging_rate, self.c_long, a) * s_lr;
            lens.learn(&p.phi, p.m[i], success[i], lr);
        }
    }

    /// Memory writes for an external action; returns the outcome view.
    fn write_external(&mut self, p: &Pending, out: &Outcome, obs: &Observation) -> OutcomeView {
        let now = obs.tick;
        let x = PriorityInputs {
            v_enc: self.v(),
            survival: f32::from(u8::from(out.damage > 0.0)),
            c_enc: self.c(),
            urgency: self.urgency(now),
            ..PriorityInputs::default()
        };
        let target = p.target.as_ref().map(|k| {
            self.mem
                .encode(k, Tier::Semantic, x, now, Source::Perception)
                .0
        });
        let mut view = OutcomeView {
            success: out.success,
            progress: out.progress,
            damage: out.damage,
            blocked: out.blocked,
            died: obs.health <= 0.0,
            ..OutcomeView::default()
        };
        let mut gained = Vec::new();
        let mut learned: Vec<String> = out.new_things.iter().map(|t| t.0.clone()).collect();
        for (item, _) in &out.items_gained.items {
            learned.push(format!("item:{item}"));
            if p.skill == Some(Skill::Craft) {
                learned.push(format!("recipe:{item}"));
            }
        }
        for key in learned {
            let (id, new) = self
                .mem
                .encode(&key, Tier::Semantic, x, now, Source::Perception);
            if key.starts_with("item:") {
                gained.push(id);
            }
            if new {
                view.new_items += 1;
                if let Some(t) = target.filter(|&t| t != id) {
                    self.mem.link(id, t, LINK_STEP);
                    view.new_linked += 1;
                }
            }
        }
        let skill_moves = matches!(p.skill, Some(Skill::Explore | Skill::MoveTo | Skill::Flee));
        view.moved = skill_moves && out.success;
        // Episode, procedural skill item, danger, route, habituation.
        let (ep, _) = self.mem.encode(
            &format!("ep:{}:{}", p.tick, p.skill_label),
            Tier::Episodic,
            x,
            now,
            Source::Perception,
        );
        self.mem.entry_mut(ep).unresolved = !out.success || out.damage > 0.0;
        let (sk, _) = self.mem.encode(
            &format!("skill:{}:{:?}", p.skill_label, p.task),
            Tier::Procedural,
            PriorityInputs::default(),
            now,
            Source::Perception,
        );
        self.mem.expose(sk, now, view.new_items > 0);
        if let Some(t) = target {
            self.mem.link(ep, t, LINK_STEP);
            self.mem.expose(t, now, view.new_items > 0);
            let e = self.mem.entry_mut(t);
            e.danger = if out.damage > 0.0 {
                e.danger.max((out.damage * 5.0).min(1.0))
            } else {
                e.danger * 0.9
            };
            if out.success {
                self.resolve(t);
            }
        }
        let mut cheaper = false;
        if let (Some((key, cost)), true) = (&p.route, out.success) {
            cheaper = self
                .mem
                .record_route(key, *cost)
                .is_some_and(|prev| *cost < prev);
        }
        view.o_decision = cheaper || p.hazard_vetoed;
        if let Some(skill) = p.skill {
            let err = self
                .skills
                .record(ActionKey::from(skill), p.task, out.success, now);
            self.lp.record(err);
        }
        self.episodes.push_back(EpisodeRecord {
            mem_id: ep,
            skill: p.skill_label.clone(),
            target,
            success: out.success,
            gained,
        });
        if self.episodes.len() > EPISODES_KEPT {
            self.episodes.pop_front();
        }
        view
    }

    /// A success on a target resolves earlier failed episodes about it.
    fn resolve(&mut self, target: usize) {
        let eps: Vec<usize> = self.mem.links(target).map(|(o, _)| o).collect();
        for e in eps {
            if self.mem.entry(e).item.tier == Tier::Episodic {
                self.mem.entry_mut(e).unresolved = false;
            }
        }
    }

    /// Items in TM within 20 ticks before a tick with R > 0 become useful
    /// (spec §14).
    fn mark_useful(&mut self, now: u64) {
        for id in self.tm.top() {
            let e = self.mem.entry_mut(id);
            if e.last_tm
                .is_some_and(|t| now.saturating_sub(t) <= USEFUL_WINDOW)
            {
                e.useful = 1.0;
                self.mem.mark_useful(id);
            }
        }
    }

    /// `creative(a) = novelty > θ_cr ∧ m_O(a) > 0`, with m_O re-scored after
    /// the outcome (spec §18).
    fn creative(&mut self, p: &Pending) -> bool {
        if p.tags == 0 {
            return false;
        }
        let novelty = 1.0
            - self
                .action_tags
                .iter()
                .map(|&t| jaccard(t, p.tags))
                .fold(0.0, f32::max);
        if !self.action_tags.contains(&p.tags) {
            self.action_tags.push(p.tags);
        }
        let m_o = self.lenses[Mode::Optimizing.index()].score(&p.phi);
        is_creative(novelty, THETA_CREATIVE, m_o)
    }

    /// An emergency reflex fired: O scores the last voluntary action −1 if it
    /// was within 100 ticks (spec §7.1, §20).
    pub fn reflex_fired(&mut self, tick: u64) {
        if let Some((t, phi, m_o)) = self.last_voluntary {
            if tick.saturating_sub(t) <= REFLEX_WINDOW {
                let o = Mode::Optimizing.index();
                let lr = self.lenses[o].params.base_lr * self.config.lens_lr[o];
                self.lenses[o].learn(&phi, m_o, -1.0, lr * self.dev.lr_multiplier());
                self.last_voluntary = None;
            }
        }
        self.log.reflex = true;
    }
}

fn lerp((a, b): (f32, f32), t: f32) -> f32 {
    a + (b - a) * t
}

/// Jaccard similarity of two tag sets held as bitmasks.
pub(crate) fn jaccard(a: u32, b: u32) -> f32 {
    let u = (a | b).count_ones();
    if u == 0 {
        return 1.0;
    }
    (a & b).count_ones() as f32 / u as f32
}

/// An id per action type for the stuck detector's entropy.
pub(crate) fn action_type(c: Choice, skill: Option<Skill>) -> u32 {
    match (c, skill) {
        (Choice::Internal(a), _) => 100 + a as u32,
        (_, Some(s)) => s as u32,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InternalAction;

    #[test]
    fn jaccard_of_bitmasks() {
        assert_eq!(jaccard(0, 0), 1.0);
        assert_eq!(jaccard(0b1100, 0b1010), 1.0 / 3.0);
        assert_eq!(jaccard(0b1, 0b10), 0.0);
        assert_eq!(jaccard(0b11, 0b11), 1.0);
    }

    #[test]
    fn action_types_and_lerp() {
        let mine = Some(Skill::Mine);
        assert_eq!(action_type(Choice::External(3), mine), Skill::Mine as u32);
        assert_eq!(action_type(Choice::External(3), None), 99);
        assert_eq!(
            action_type(Choice::Internal(InternalAction::Plan), None),
            100 + InternalAction::Plan as u32
        );
        assert_eq!(lerp((0.1, 0.5), 0.5), 0.3);
    }
}

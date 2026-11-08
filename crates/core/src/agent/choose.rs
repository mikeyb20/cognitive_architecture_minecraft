//! Spec §4 lines 9–15: candidates plus internal actions, lens scores, the
//! Oracle with kick, urgency and choking, aggregation, choice and intensity.

use super::needs::Felt;
use super::perceive::Sensed;
use super::think::ThinkTools;
use super::{Brain, Choice, Decision, Pending, ctx};
use crate::activation::external_drive_effort;
use crate::control::kick::kick_extremes;
use crate::control::oracle::{CONTEXT_DIM, Context, Oracle, WeightAdjust};
use crate::control::permeability::PermeabilityInputs;
use crate::direction::features::route_key;
use crate::direction::{FeatureInputs, InternalExtras, target_key};
use crate::memory::{ActionKey, InternalAction};
use crate::modes::{Features, Mode, ModeNetwork, feature as f, score};
use host_api::{Candidate, Intensity, Observation, Skill};
use rand::Rng;

/// Danger at which a remembered hazard counts for O's veto credit (spec §7.1).
pub(crate) const HAZARD: f32 = 0.5;

#[derive(Debug, Clone)]
pub(crate) struct Scored {
    pub choice: Choice,
    pub phi: Features,
    pub m: [f32; 4],
    pub kicked: [f32; 4],
    /// Effort-floor share in [0, 1]: 1 for a command or danger, a felt
    /// need's push otherwise (01 §1.6), 0 for none.
    pub push: f32,
}

impl Brain {
    /// π inputs (spec §11).
    pub(crate) fn perm_inputs(&self, obs: &Observation) -> PermeabilityInputs {
        let task = self.context_task();
        let step_danger = self
            .step
            .as_ref()
            .and_then(|s| self.mem.get(&s.thing))
            .filter(|&i| self.mem.is_alive(i))
            .map_or(0.0, |i| self.mem.entry(i).danger);
        let stakes = if self.step.is_some() {
            self.signals.threat.max(step_danger)
        } else {
            0.0
        };
        let routine = self.step.as_ref().map_or(0.0, |s| {
            let key = format!("skill:{:?}:{:?}", s.skill, s.task);
            let h = self
                .mem
                .get(&key)
                .filter(|&i| self.mem.is_alive(i))
                .map_or(0.0, |i| self.mem.habituation(i, obs.tick));
            h * self.skills.p_hat(ActionKey::from(s.skill), s.task)
        });
        PermeabilityInputs {
            threat: self.signals.threat,
            stakes,
            fatigue: self.fatigue.value(),
            tenacity_p: self.tenacity.value(Mode::Performing, task),
            routine: (routine + self.routine_bonus[task.index()]).min(1.0),
        }
    }

    /// x(S) (spec §8).
    fn context(&self, s: &Sensed, stakes: f32) -> Context {
        let mut x = [0.0; CONTEXT_DIM];
        let task = self.context_task();
        x[ctx::BIAS] = 1.0;
        x[ctx::TASK0 + task.index()] = 1.0;
        x[ctx::COMPETENCE] = self.skills.task_p_hat(task);
        x[ctx::RETRIEVAL_HIT] = f32::from(u8::from(s.retrieval_hit));
        x[ctx::STAKES] = stakes;
        x[ctx::THREAT] = self.signals.threat;
        x[ctx::ENERGY] = self.signals.energy;
        x[ctx::NOVELTY] = self.signals.novelty;
        x[ctx::URGENCY] = s.u;
        x[ctx::FATIGUE] = self.fatigue.value();
        x
    }

    fn extras(&self, obs: &Observation, stakes: f32) -> InternalExtras {
        let recent: Vec<bool> = self
            .episodes
            .iter()
            .rev()
            .take(20)
            .map(|e| self.mem.entry(e.mem_id).reflected)
            .collect();
        let unreflected = if recent.is_empty() {
            0.0
        } else {
            recent.iter().filter(|r| !**r).count() as f32 / recent.len() as f32
        };
        let imagine_fresh = if self.goal.is_none() || self.unlinked_pair().is_some() {
            1.0
        } else {
            0.0
        };
        InternalExtras {
            unreflected,
            imagine_fresh,
            stakes,
            weak_share: self.sub.weak_share(),
            plan_valid: self.step.is_some()
                && self.plan_inv.as_ref() == Some(&obs.inventory)
                && obs.tick < self.plan_tick + super::PLAN_TTL,
        }
    }

    fn score_all(
        &self,
        obs: &Observation,
        cands: &[Candidate],
        stakes: f32,
        felt: &Felt,
    ) -> Vec<Scored> {
        let tm = self.tm.top();
        let fi = FeatureInputs {
            obs,
            mem: &self.mem,
            skills: &self.skills,
            tm: &tm,
            step: self.step.as_ref(),
            has_goal: self.goal.is_some(),
            task: self.context_task(),
            now: obs.tick,
        };
        let ext = self.extras(obs, stakes);
        let cmd_goal = self.goal.as_ref().is_some_and(|g| g.commanded);
        let mut out: Vec<Scored> = Vec::with_capacity(cands.len() + 5);
        let threatened = self.config.threat_drive && self.signals.threat > 0.0;
        for (i, c) in cands.iter().enumerate() {
            let forced = c.commanded || (threatened && c.skill == Skill::Flee);
            let push = if forced {
                1.0
            } else {
                self.need_push(obs, c, felt)
            };
            out.push(self.scored(Choice::External(i), fi.external(c), push));
        }
        let chosen = InternalAction::ALL
            .into_iter()
            .filter(|&a| !self.config.fixed_thinking.triggered(a));
        for a in chosen {
            let push = f32::from(u8::from(a == InternalAction::Plan && cmd_goal));
            out.push(self.scored(Choice::Internal(a), fi.internal(a, &ext), push));
        }
        out
    }

    fn scored(&self, choice: Choice, phi: Features, push: f32) -> Scored {
        let m = std::array::from_fn(|i| self.lenses[i].score(&phi));
        Scored {
            choice,
            phi,
            m,
            kicked: m,
            push,
        }
    }

    /// Lines 9–15. Returns the decision and fills `pending`.
    pub(crate) fn choose(
        &mut self,
        obs: &Observation,
        cands: &[Candidate],
        s: &Sensed,
        tools: &mut dyn ThinkTools,
    ) -> Decision {
        let pin = self.perm_inputs(obs);
        let x = self.context(s, pin.stakes);
        let felt = self.feel(obs, tools);
        self.log.hunger = felt.hunger;
        let mut all = self.score_all(obs, cands, pin.stakes, &felt);
        // 11 Oracle: kick first (00 §3.5.4), then weights.
        let p_kick = self.dev.kick_rate(self.config.kick.p_base);
        let mut kicks = 0;
        if self.config.kick_enabled {
            for sc in &mut all {
                kicks += kick_extremes(
                    &mut sc.kicked,
                    self.config.kick.theta,
                    p_kick,
                    &mut self.rng,
                );
            }
        }
        let p_leads_raw = {
            let w0 = self.oracle.weights(&x, WeightAdjust::default());
            leads(&w0, Mode::Performing)
        };
        let adj = WeightAdjust {
            urgency_bias: self.config.beta_u * s.u,
            choke_shift: if p_leads_raw {
                self.perm.choke_shift()
            } else {
                0.0
            },
        };
        let w = self.oracle.weights(&x, adj);
        let dominant = dominant(&w);
        if s.stuck {
            // Stuck trigger: flip the dominant mode's scores for one tick.
            for sc in &mut all {
                sc.kicked[dominant] = -sc.kicked[dominant];
            }
        }
        // 12–13 score and choose.
        let scores: Vec<f32> = all.iter().map(|sc| score(&w, &sc.kicked)).collect();
        let g = self.g();
        // A command, danger or felt need sets an effort floor (#41; 01 §1.6).
        let (eps, d) = (self.config.eps_ext, self.d());
        let effort_of = |sc: &Scored| external_drive_effort(g, eps * sc.push, d);
        let u: f32 = self.rng.random();
        let starts = |e: f32| (3.0 * e + u).floor() >= 1.0;
        // With no push the floor is g itself, so only pushed steps pass here.
        let allowed: Vec<usize> = if starts(g) {
            (0..all.len()).collect()
        } else {
            (0..all.len())
                .filter(|&i| starts(effort_of(&all[i])))
                .collect()
        };
        let tau = self.lenses[dominant].temperature() * self.config.tau_scale;
        let pick = self.sample(&allowed, &scores, tau);
        self.log
            .fill_choice(&all, &scores, &w, kicks, s.stuck, pick);
        self.last_w = w;
        self.last_kind = pick.map_or(super::IDLE_KIND, |k| {
            let skill = match all[k].choice {
                Choice::External(i) => Some(cands[i].skill),
                Choice::Internal(_) => None,
            };
            super::learn::action_type(all[k].choice, skill)
        });
        let Some(k) = pick else {
            // An idle tick earns nothing: R̄_20 counts it as 0 (spec §12).
            self.push_r(0.0);
            self.last_effort = 0.0;
            self.last_external = false;
            self.pending = None;
            return Decision::Idle;
        };
        // 14 intensity.
        let sc = &all[k];
        let effort = effort_of(sc);
        let p_leads = leads(&w, Mode::Performing);
        let it = Intensity {
            effort,
            focus: self.perm.focus(p_leads),
            external_drive: effort > g,
        };
        let picked = Picked {
            w,
            x,
            it,
            hazard_vetoed: hazard_vetoed(&all, k),
        };
        self.prepare(obs, cands, &all[k], &picked, tools)
    }

    /// Softmax over the allowed choices at temperature τ (01 §3.3).
    pub(crate) fn sample(&mut self, allowed: &[usize], scores: &[f32], tau: f32) -> Option<usize> {
        if allowed.is_empty() {
            return None;
        }
        let tau = tau.max(1e-3);
        let max = allowed
            .iter()
            .map(|&i| scores[i])
            .fold(f32::NEG_INFINITY, f32::max);
        let e: Vec<f32> = allowed
            .iter()
            .map(|&i| ((scores[i] - max) / tau).exp())
            .collect();
        let total: f32 = e.iter().sum();
        let mut r = self.rng.random::<f32>() * total;
        for (j, &i) in allowed.iter().enumerate() {
            r -= e[j];
            if r <= 0.0 {
                return Some(i);
            }
        }
        allowed.last().copied()
    }

    fn prepare(
        &mut self,
        obs: &Observation,
        cands: &[Candidate],
        sc: &Scored,
        picked: &Picked,
        tools: &mut dyn ThinkTools,
    ) -> Decision {
        let Picked {
            w,
            x,
            it,
            hazard_vetoed,
        } = *picked;
        let progress_before = self
            .goal
            .as_ref()
            .map_or(0.0, |g| tools.progress(&g.item, obs));
        let (target, route, skill, task, tags) = match sc.choice {
            Choice::External(i) => {
                let c = &cands[i];
                let t = target_key(obs, c);
                let route = t.as_ref().map(|k| (route_key(c.skill, k), c.est_cost));
                let tags = super::log::action_tags(c, t.as_deref(), obs);
                (t, route, Some(c.skill), c.task_type, tags)
            }
            Choice::Internal(_) => (None, None, None, self.context_task(), 0),
        };
        let label = match sc.choice {
            Choice::External(i) => format!("{:?}", cands[i].skill),
            Choice::Internal(a) => format!("{a:?}"),
        };
        if let Some(id) = target.as_ref().and_then(|k| self.mem.get(k)) {
            if self.mem.is_alive(id) {
                self.tm.add(id, super::perceive::FOCUS_ACTIVATION);
            }
        }
        self.last_task = task;
        self.last_effort = it.effort;
        self.last_external = matches!(sc.choice, Choice::External(_));
        let mut p = Pending {
            tick: obs.tick,
            choice: sc.choice,
            phi: sc.phi,
            m: sc.m,
            w,
            x,
            task,
            target,
            route,
            skill,
            skill_label: label,
            progress_before,
            hazard_vetoed,
            tags,
            internal: None,
            outcome: None,
            food_before: obs.food,
        };
        self.log.effort = it.effort;
        self.log.focus = it.focus;
        self.log.external_drive = it.external_drive;
        self.log.need_push = sc.push;
        let decision = match sc.choice {
            Choice::External(i) => Decision::Act(i, it),
            Choice::Internal(a) => {
                p.internal = Some(self.think(a, it.effort, obs, tools));
                Decision::Thought(a)
            }
        };
        self.pending = Some(p);
        decision
    }
}

/// What `choose` settled for the picked choice, handed to `prepare`.
#[derive(Debug, Clone, Copy)]
struct Picked {
    w: [f32; 4],
    x: Context,
    it: Intensity,
    hazard_vetoed: bool,
}

/// Re-export for the log: the Oracle's raw preferences Θ·x.
pub(crate) fn preferences(o: &Oracle, x: &Context) -> [f32; 4] {
    std::array::from_fn(|i| o.theta[i].iter().zip(x).map(|(a, b)| a * b).sum())
}

/// O avoided a remembered hazard: another option with remembered danger
/// was scored negative by O and not taken (spec §7.1).
pub(crate) fn hazard_vetoed(all: &[Scored], chosen: usize) -> bool {
    let o = Mode::Optimizing.index();
    all.iter().enumerate().any(|(i, sc)| {
        i != chosen
            && sc.phi[f::RISK] >= HAZARD
            && sc.m[o] < 0.0
            && all[chosen].phi[f::RISK] < sc.phi[f::RISK]
    })
}

/// Index of the largest weight; the first wins a tie (spec §8, stuck kick).
pub(crate) fn dominant(w: &[f32; 4]) -> usize {
    (0..4).fold(0, |b, i| if w[i] > w[b] { i } else { b })
}

/// Mode `m` leads: its weight is at least every other weight (#43).
pub(crate) fn leads(w: &[f32; 4], m: Mode) -> bool {
    w.iter().all(|&wi| w[m.index()] >= wi)
}

//! Spec §4 lines 1–8: perceive, body, signals, V, D, C, π and memory upkeep,
//! integrated over the world ticks since the last decision.

use super::{Brain, Goal};
use crate::activation::signals::{social, threat};
use crate::activation::{Activation, ValenceInputs};
use crate::body::energy;
use crate::control::time::urgency;
use crate::memory::{PriorityInputs, Source, Tier};
use host_api::ontology::region_of;
use host_api::{Observation, SeenKind};

/// Longest stretch integrated in one decision (one day, spec §21).
pub(crate) const MAX_INTEGRATE: u64 = 2400;
/// TM activation for retrieved and flagged items, and for the chosen target.
pub(crate) const RECALL_ACTIVATION: f32 = 0.5;
pub(crate) const FOCUS_ACTIVATION: f32 = 1.0;
/// Co-activation link step for TM pairs (proposed).
pub(crate) const TM_LINK: f32 = 0.01;
/// Per-tick fade of a rehearsal's routine boost (proposed).
pub(crate) const ROUTINE_FADE: f32 = 0.995;
/// The V input "damage" is the health lost over this many ticks (proposed;
/// spec §12 does not say over what span, and one tick's loss is tiny).
pub(crate) const DAMAGE_WINDOW: u64 = 20;
/// C̄_long time constant in ticks (spec §15: about 10⁴).
pub(crate) const C_LONG_TICKS: f32 = 1e4;

/// What sensing found, for the decision and the log.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sensed {
    pub dt: u64,
    pub retrieved: Vec<usize>,
    pub retrieval_hit: bool,
    pub stuck: bool,
    pub nudge: f32,
    pub sub_processed: u32,
    pub pruned: u32,
    pub formed: u32,
    pub u: f32,
}

/// Food and hostile things count as survival-relevant for priority (spec §17).
fn survival_thing(key: &str) -> bool {
    key.starts_with("entity:zombie")
        || matches!(
            key,
            "entity:pig" | "block:wheat" | "item:porkchop" | "item:bread" | "block:lava"
        )
}

impl Brain {
    pub(crate) fn sense(&mut self, obs: &Observation) -> Sensed {
        let now = obs.tick;
        let dt = self
            .last_tick
            .map_or(1, |t| now.saturating_sub(t).max(1))
            .min(MAX_INTEGRATE);
        self.last_tick = Some(now);
        let mut s = Sensed {
            dt,
            ..Sensed::default()
        };
        self.read_chat(obs);
        // 2 body
        for _ in 0..dt {
            self.fatigue
                .step(self.last_effort, self.last_external, obs.resting);
        }
        // 3 signals
        s.formed = self.perceive_things(obs);
        s.u = self.urgency(now);
        self.signals.threat = threat(obs);
        self.signals.social = social(obs);
        self.signals.energy = energy(obs.health, obs.food, self.fatigue.value());
        self.signals.learning_progress = self.lp.value();
        self.signals.habituation = self.current_target_habituation(now);
        // subconscious, with the budget for the ticks that passed
        let budget = self
            .dev
            .subconscious_budget(self.config.subconscious.b_base, obs.resting);
        let rep = self.sub.run(&mut self.mem, budget * dt as f32, now);
        s.sub_processed = rep.processed;
        s.pruned = rep.pruned;
        s.nudge = rep.nudge;
        for id in rep.flagged {
            if self.rng.random_bool_f32(self.perm.value()) {
                s.retrieved.push(id);
            }
        }
        // 4–5 V and D
        let tm_v = self.tm_valence();
        self.damage.push_back((now, obs.damage_taken));
        while self
            .damage
            .front()
            .is_some_and(|(t, _)| now.saturating_sub(*t) >= DAMAGE_WINDOW)
        {
            self.damage.pop_front();
        }
        let x = ValenceInputs {
            r_mean_20: self.r_mean(),
            threat: self.signals.threat,
            damage: self.damage.iter().map(|(_, d)| d).sum::<f32>().min(1.0),
            tm_valence: tm_v,
        };
        for _ in 0..dt {
            self.affect.step(x);
        }
        // 6 C: drift, nudge × π, stuck boost κ; every rise × (1 − D)
        let shift = self.config.beta_c * s.u;
        self.c_target = self
            .curiosity
            .target(self.config.k.weighted(&self.signals), shift);
        for _ in 0..dt {
            self.curiosity.drift(self.c_target, self.affect.d());
        }
        self.curiosity
            .impulse(s.nudge * self.perm.value(), self.affect.d());
        for _ in 0..dt {
            self.stuck.record(self.last_w, self.c(), self.last_kind);
        }
        s.stuck = self.config.kick_enabled && self.stuck.is_stuck();
        if s.stuck {
            self.curiosity
                .impulse(self.config.kick.kappa, self.affect.d());
            self.stuck.reset();
        }
        let a = 1.0 - (-(dt as f32) / C_LONG_TICKS).exp();
        self.c_long += a * (self.c() - self.c_long);
        // 7 π
        let pin = self.perm_inputs(obs);
        for _ in 0..dt {
            self.perm.step(pin);
        }
        for r in &mut self.routine_bonus {
            *r *= ROUTINE_FADE.powi(dt as i32);
        }
        // 8 memory: TM decay, retrieval by cue
        self.recall(obs, &mut s);
        s
    }

    /// Player commands set the active goal (01 §4.6; spec §13).
    fn read_chat(&mut self, obs: &Observation) {
        for m in &obs.chat {
            let x = PriorityInputs {
                repetition: 1.0,
                c_enc: self.c(),
                ..PriorityInputs::default()
            };
            self.mem.encode(
                &format!("chat:{}", m.text),
                Tier::Episodic,
                x,
                obs.tick,
                Source::PlayerChat,
            );
            if let Some(item) = m.text.strip_prefix("get ") {
                self.set_goal(Some(Goal {
                    item: item.trim().to_string(),
                    commanded: true,
                }));
            }
        }
    }

    /// Encode first-seen things and update the novelty window (spec §5).
    fn perceive_things(&mut self, obs: &Observation) -> u32 {
        let mut keys: Vec<&str> = Vec::with_capacity(8);
        for s in &obs.visible {
            if !keys.contains(&s.thing.0.as_str()) {
                keys.push(&s.thing.0);
            }
        }
        let region = region_of(obs.position).0;
        let mut new = 0;
        let total = keys.len() as u32 + 1;
        let hostile_near = obs
            .visible
            .iter()
            .any(|s| matches!(s.kind, SeenKind::Entity { hostile: true }));
        for key in keys.into_iter().map(str::to_string).chain([region]) {
            let x = PriorityInputs {
                v_enc: self.v(),
                survival: f32::from(u8::from(survival_thing(&key) || hostile_near)),
                c_enc: self.c(),
                urgency: self.urgency(obs.tick),
                ..PriorityInputs::default()
            };
            let (id, fresh) =
                self.mem
                    .encode(&key, Tier::Semantic, x, obs.tick, Source::Perception);
            self.mem.expose(id, obs.tick, fresh);
            new += u32::from(fresh);
        }
        self.signals.novelty = self.novelty.record(new, total);
        new
    }

    /// U from the noisy horizon estimate (spec §15).
    pub(crate) fn urgency(&self, now: u64) -> f32 {
        match (self.lifespan, self.config.horizon) {
            (Some((_, est)), Some(h)) => urgency(est - now as f32, h.ticks),
            _ => 0.0,
        }
    }

    fn current_target_habituation(&self, now: u64) -> f32 {
        let key = self
            .step
            .as_ref()
            .map(|s| s.thing.clone())
            .or_else(|| self.pending.as_ref().and_then(|p| p.target.clone()));
        key.and_then(|k| self.mem.get(&k))
            .filter(|&id| self.mem.is_alive(id))
            .map_or(0.0, |id| self.mem.habituation(id, now))
    }

    fn tm_valence(&self) -> f32 {
        let ids: Vec<usize> = self
            .tm
            .top()
            .into_iter()
            .filter(|&i| self.mem.is_alive(i))
            .collect();
        if ids.is_empty() {
            return 0.0;
        }
        ids.iter()
            .map(|&i| self.mem.entry(i).item.v_enc)
            .sum::<f32>()
            / ids.len() as f32
    }

    pub(crate) fn r_mean(&self) -> f32 {
        if self.r_hist.is_empty() {
            0.0
        } else {
            self.r_hist.iter().sum::<f32>() / self.r_hist.len() as f32
        }
    }

    /// g(C) now.
    pub(crate) fn g(&self) -> f32 {
        self.act.g(self.c())
    }

    /// Cues are the plan step's thing, the last target and the TM items;
    /// facts linked to a cue are relevant prior experience (00 §4.1).
    fn recall(&mut self, obs: &Observation, s: &mut Sensed) {
        let now = obs.tick;
        let mut cues: Vec<usize> = self.tm.top();
        let keys = [
            self.step.as_ref().map(|p| p.thing.clone()),
            self.pending.as_ref().and_then(|p| p.target.clone()),
        ];
        for k in keys.into_iter().flatten() {
            if let Some(id) = self.mem.get(&k).filter(|&i| self.mem.is_alive(i)) {
                cues.push(id);
            }
        }
        for &c in &cues {
            for (o, _) in self.mem.links(c).collect::<Vec<_>>() {
                if self.mem.entry(o).item.content.starts_with("fact:") && !s.retrieved.contains(&o)
                {
                    s.retrieved.push(o);
                    s.retrieval_hit = true;
                }
            }
        }
        for &id in &s.retrieved {
            self.mem.access(id, now);
        }
        let act: Vec<(usize, f32)> = s
            .retrieved
            .iter()
            .map(|&i| (i, RECALL_ACTIVATION))
            .collect();
        self.tm.tick_n(s.dt, &act);
        let top = self.tm.top();
        for (i, &a) in top.iter().enumerate() {
            self.mem.entry_mut(a).last_tm = Some(now);
            for &b in &top[i + 1..] {
                if self.mem.is_alive(a) && self.mem.is_alive(b) {
                    self.mem.link(a, b, TM_LINK);
                }
            }
        }
    }
}

/// Bernoulli draw with an f32 probability (clamped to [0, 1]).
trait RandomBool {
    fn random_bool_f32(&mut self, p: f32) -> bool;
}

impl<R: rand::Rng> RandomBool for R {
    fn random_bool_f32(&mut self, p: f32) -> bool {
        self.random::<f32>() < p.clamp(0.0, 1.0)
    }
}

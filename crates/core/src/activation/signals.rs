//! The six C input signals and their weights (01 §1.1; spec §5).
//!
//! Every signal is in [0, 1] and uses the same formula in both worlds. The
//! extractors here are pure functions of perception, memory and body; the
//! brain loop feeds them.

use host_api::{Observation, SeenKind};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// One tick's signals, all in [0, 1] (spec §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    pub novelty: f32,
    pub learning_progress: f32,
    pub energy: f32,
    pub threat: f32,
    pub habituation: f32,
    pub social: f32,
}

/// Personality weights k_j (01 §1.1; spec §19 default column). The sign gives
/// the direction of effect.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SignalWeights {
    pub novelty: f32,
    pub learning_progress: f32,
    /// Applied to `(1 − energy)`, so a negative k lowers C when energy is low.
    pub energy: f32,
    pub threat: f32,
    pub habituation: f32,
    pub social: f32,
}

impl Default for SignalWeights {
    fn default() -> Self {
        Self {
            novelty: 0.40,
            learning_progress: 0.30,
            energy: -0.20,
            threat: -0.40,
            habituation: -0.30,
            social: 0.20,
        }
    }
}

impl SignalWeights {
    /// `Σ_j k_j·signal_j`, with energy entering as `(1 − energy)` (spec §5).
    pub fn weighted(&self, s: &Signals) -> f32 {
        self.novelty * s.novelty
            + self.learning_progress * s.learning_progress
            + self.energy * (1.0 - s.energy)
            + self.threat * s.threat
            + self.habituation * s.habituation
            + self.social * s.social
    }
}

/// Novelty window W in ticks (spec §5, proposed).
pub const NOVELTY_WINDOW: usize = 20;

/// Novelty: share of things perceived in the last W ticks that had no memory
/// item when perceived (spec §5).
#[derive(Debug, Clone, Default)]
pub struct NoveltyWindow {
    ticks: VecDeque<(u32, u32)>,
}

impl NoveltyWindow {
    /// Record one tick: how many distinct things were perceived and how many
    /// of them were new. Returns the novelty signal.
    pub fn record(&mut self, new: u32, total: u32) -> f32 {
        self.ticks.push_back((new, total));
        if self.ticks.len() > NOVELTY_WINDOW {
            self.ticks.pop_front();
        }
        self.value()
    }

    pub fn value(&self) -> f32 {
        let (n, t) = self
            .ticks
            .iter()
            .fold((0, 0), |(a, b), (n, t)| (a + n, b + t));
        if t == 0 { 0.0 } else { n as f32 / t as f32 }
    }
}

/// Learning progress over two windows of p̂ prediction error:
/// `clamp((err_old − err_new)/err_old, 0, 1)` (spec §5; Oudeyer & Kaplan 2007).
#[derive(Debug, Clone, Default)]
pub struct LearningProgress {
    errors: VecDeque<f32>,
}

/// Size of each of the two error windows (proposed).
pub const LP_WINDOW: usize = 20;

impl LearningProgress {
    /// Record one prediction error `|outcome − p̂|`.
    pub fn record(&mut self, err: f32) {
        self.errors.push_back(err.abs().min(1.0));
        if self.errors.len() > 2 * LP_WINDOW {
            self.errors.pop_front();
        }
    }

    pub fn value(&self) -> f32 {
        if self.errors.len() < 2 * LP_WINDOW {
            return 0.0;
        }
        let old: f32 = self.errors.iter().take(LP_WINDOW).sum::<f32>() / LP_WINDOW as f32;
        let new: f32 = self.errors.iter().skip(LP_WINDOW).sum::<f32>() / LP_WINDOW as f32;
        if old <= 0.0 {
            return 0.0;
        }
        ((old - new) / old).clamp(0.0, 1.0)
    }
}

/// Distance scale for hostile threat, in tiles (spec §5: `e^(−d/8)`).
pub const THREAT_SCALE: f32 = 8.0;

/// Lava counts as "near" when the agent could step into it: adjacent,
/// diagonals included. Spec §5 leaves lava_near undefined; its size is
/// proposed at half a close hostile so standing next to lava does not pin
/// C at 0 (flagged in the step 4 PR).
pub const LAVA_NEAR: f32 = 1.5;
pub const LAVA_THREAT: f32 = 0.5;

/// `clamp(Σ_hostile e^(−d/8) + lava_near + fall_risk, 0, 1)` (spec §5).
/// The sim has no falls, so fall risk is 0 until the Minecraft adapter
/// reports it.
pub fn threat(obs: &Observation) -> f32 {
    let mut t = 0.0;
    for s in &obs.visible {
        match s.kind {
            SeenKind::Entity { hostile: true } if !obs.sheltered => {
                t += (-s.distance / THREAT_SCALE).exp();
            }
            SeenKind::Block if s.thing.0 == "block:lava" && s.distance <= LAVA_NEAR => {
                t += LAVA_THREAT;
            }
            _ => {}
        }
    }
    f32::clamp(t, 0.0, 1.0)
}

/// `min(1, players_within_16/2)`, or 1 on the tick after a player chats (spec §5).
pub fn social(obs: &Observation) -> f32 {
    if !obs.chat.is_empty() {
        return 1.0;
    }
    let near = obs
        .visible
        .iter()
        .filter(|s| s.kind == SeenKind::Player && s.distance <= 16.0)
        .count();
    (near as f32 / 2.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::{AgentId, ChatMsg, Inventory, Pos, Seen, ThingId};

    fn obs(visible: Vec<Seen>) -> Observation {
        Observation {
            agent: AgentId(0),
            tick: 0,
            day_phase: 0.0,
            health: 1.0,
            food: 1.0,
            position: Pos::default(),
            inventory: Inventory::default(),
            visible,
            chat: vec![],
            damage_taken: 0.0,
            resting: false,
            sheltered: false,
        }
    }

    fn seen(thing: &str, kind: SeenKind, d: f32) -> Seen {
        Seen {
            thing: ThingId(thing.into()),
            kind,
            pos: Pos::default(),
            distance: d,
        }
    }

    #[test]
    fn weights_default_and_weighted_sum() {
        let k = SignalWeights::default();
        let s = Signals {
            novelty: 1.0,
            learning_progress: 1.0,
            energy: 0.5,
            threat: 0.5,
            habituation: 1.0,
            social: 1.0,
        };
        // 0.4 + 0.3 − 0.2·0.5 − 0.4·0.5 − 0.3 + 0.2 = 0.3
        assert!((k.weighted(&s) - 0.3).abs() < 1e-6);
        let rested = Signals {
            energy: 1.0,
            ..Signals::default()
        };
        assert_eq!(k.weighted(&rested), 0.0, "full energy adds nothing");
    }

    #[test]
    fn novelty_is_the_share_of_new_things_over_the_window() {
        let mut w = NoveltyWindow::default();
        assert_eq!(w.value(), 0.0);
        assert_eq!(w.record(2, 4), 0.5);
        assert_eq!(w.record(0, 4), 0.25);
        for _ in 0..NOVELTY_WINDOW {
            w.record(0, 2);
        }
        assert_eq!(w.value(), 0.0, "old ticks leave the window");
        assert_eq!(w.record(1, 0), 1.0 / 38.0, "one (0, 2) tick dropped");
    }

    #[test]
    fn learning_progress_needs_two_full_windows_and_rewards_falling_error() {
        let mut lp = LearningProgress::default();
        for _ in 0..LP_WINDOW {
            lp.record(0.8);
        }
        assert_eq!(lp.value(), 0.0);
        for _ in 0..LP_WINDOW - 1 {
            lp.record(0.2);
        }
        assert_eq!(lp.value(), 0.0, "one short of two windows");
        lp.record(0.2);
        assert!((lp.value() - 0.75).abs() < 1e-5);
        for _ in 0..2 * LP_WINDOW {
            lp.record(0.0);
        }
        assert_eq!(lp.value(), 0.0, "no old error: no progress to measure");
        let mut worse = LearningProgress::default();
        for e in [0.1, 0.9] {
            for _ in 0..LP_WINDOW {
                worse.record(e);
            }
        }
        assert_eq!(worse.value(), 0.0, "rising error clamps to 0");
        worse.record(-3.0);
        assert!(worse.value() <= 1.0);
    }

    #[test]
    fn threat_sums_hostiles_and_lava() {
        let z = SeenKind::Entity { hostile: true };
        let one = threat(&obs(vec![seen("entity:zombie", z, 8.0)]));
        assert!((one - (-1.0f32).exp()).abs() < 1e-6);
        let pig = SeenKind::Entity { hostile: false };
        assert_eq!(threat(&obs(vec![seen("entity:pig", pig, 1.0)])), 0.0);
        assert_eq!(
            threat(&obs(vec![seen("block:lava", SeenKind::Block, 1.5)])),
            LAVA_THREAT
        );
        assert_eq!(
            threat(&obs(vec![seen("block:lava", SeenKind::Block, 1.6)])),
            0.0
        );
        assert_eq!(
            threat(&obs(vec![seen("block:stone", SeenKind::Block, 0.0)])),
            0.0
        );
        let two = threat(&obs(vec![
            seen("entity:zombie", z, 0.0),
            seen("entity:zombie", z, 0.0),
        ]));
        assert_eq!(two, 1.0, "clamped");
        let mut safe = obs(vec![seen("entity:zombie", z, 1.0)]);
        safe.sheltered = true;
        assert_eq!(threat(&safe), 0.0, "hostiles can't reach a sheltered agent");
    }

    #[test]
    fn social_counts_players_and_chat() {
        let p = |d| seen("player:steve", SeenKind::Player, d);
        assert_eq!(social(&obs(vec![])), 0.0);
        assert_eq!(social(&obs(vec![p(16.0)])), 0.5);
        assert_eq!(social(&obs(vec![p(16.1)])), 0.0);
        assert_eq!(social(&obs(vec![p(1.0), p(2.0), p(3.0)])), 1.0);
        let mut o = obs(vec![]);
        o.chat.push(ChatMsg {
            from: "steve".into(),
            text: "hi".into(),
        });
        assert_eq!(social(&o), 1.0);
    }
}

//! Each mode's own success function (01 §3.1, §4.6; spec §7.1). Thresholds
//! rise with ρ, so "success evolves" (00 §7.2.1).

use super::Mode;
use crate::memory::InternalAction;

/// What an executed action did, as the success functions see it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OutcomeView {
    pub success: bool,
    /// Step progress 0..1.
    pub progress: f32,
    pub damage: f32,
    pub blocked: bool,
    pub died: bool,
    /// The agent moved (a movement skill that succeeded).
    pub moved: bool,
    /// New semantic memory items formed by this action.
    pub new_items: u32,
    /// How many of them link to an item that already existed.
    pub new_linked: u32,
    /// The action ended in a keep, remove or reinforce decision (spec §7.1 O).
    pub o_decision: bool,
    pub internal: Option<InternalAction>,
}

impl OutcomeView {
    /// Novelty of the outcome, `n/(n + 1)` for n new items (proposed).
    pub fn novelty(&self) -> f32 {
        let n = self.new_items as f32;
        n / (n + 1.0)
    }
}

/// Success thresholds thr_L, thr_E, thr_P at the current ρ_eff (spec §14).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub l: f32,
    pub e: f32,
    pub p: f32,
}

/// Credit for retaining an unlinked new item ("an isolated fact is
/// low-value"), for moving without punishment, and for a completed
/// rehearsal (spec §7.1).
pub const PARTIAL: f32 = 0.3;
/// L's score when nothing was retained.
pub const NOTHING_RETAINED: f32 = -0.5;

/// `success_i ∈ [−1, 1]` (spec §7.1).
pub fn success(mode: Mode, o: &OutcomeView, thr: &Thresholds) -> f32 {
    match mode {
        Mode::Learning => {
            if o.new_linked > 0 && o.novelty() >= thr.l {
                1.0
            } else if o.new_items > 0 {
                PARTIAL
            } else {
                NOTHING_RETAINED
            }
        }
        Mode::Exploring => {
            if o.died || o.damage > 0.0 || o.blocked {
                -1.0
            } else if o.new_items > 0 && o.novelty() >= thr.e {
                1.0
            } else if o.moved {
                PARTIAL
            } else {
                0.0
            }
        }
        Mode::Performing => {
            if !o.success {
                -1.0
            } else if o.internal == Some(InternalAction::Rehearse) {
                PARTIAL
            } else if o.progress >= thr.p {
                o.progress.clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        Mode::Optimizing => {
            if o.o_decision {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// All four, in mode order.
pub fn all_success(o: &OutcomeView, thr: &Thresholds) -> [f32; 4] {
    Mode::ALL.map(|m| success(m, o, thr))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THR: Thresholds = Thresholds {
        l: 0.4,
        e: 0.5,
        p: 0.3,
    };

    fn ok() -> OutcomeView {
        OutcomeView {
            success: true,
            progress: 1.0,
            ..OutcomeView::default()
        }
    }

    #[test]
    fn credit_values() {
        assert_eq!((PARTIAL, NOTHING_RETAINED), (0.3, -0.5));
        // With thr_E = 0, an action that found nothing still gets no full
        // credit: Exploring needs at least one new item.
        let zero = Thresholds { e: 0.0, ..THR };
        assert_eq!(success(Mode::Exploring, &ok(), &zero), 0.0);
    }

    #[test]
    fn novelty_of_an_outcome() {
        let mut o = ok();
        assert_eq!(o.novelty(), 0.0);
        o.new_items = 1;
        assert_eq!(o.novelty(), 0.5);
        o.new_items = 3;
        assert_eq!(o.novelty(), 0.75);
    }

    #[test]
    fn learning_wants_linked_new_items() {
        let s = |o: &OutcomeView| success(Mode::Learning, o, &THR);
        let mut o = ok();
        assert_eq!(s(&o), NOTHING_RETAINED);
        o.new_items = 1;
        assert_eq!(s(&o), PARTIAL, "new but unlinked");
        o.new_linked = 1;
        assert_eq!(s(&o), 1.0);
        let strict = Thresholds { l: 0.6, ..THR };
        assert_eq!(success(Mode::Learning, &o, &strict), PARTIAL, "below thr_L");
    }

    #[test]
    fn exploring_wants_new_ground_without_harm() {
        let s = |o: &OutcomeView| success(Mode::Exploring, o, &THR);
        let mut o = ok();
        assert_eq!(s(&o), 0.0);
        o.moved = true;
        assert_eq!(s(&o), PARTIAL);
        o.new_items = 1;
        assert_eq!(s(&o), 1.0);
        let strict = Thresholds { e: 0.51, ..THR };
        assert_eq!(success(Mode::Exploring, &o, &strict), PARTIAL);
        for harm in [
            OutcomeView { damage: 0.1, ..o },
            OutcomeView { blocked: true, ..o },
            OutcomeView { died: true, ..o },
        ] {
            assert_eq!(s(&harm), -1.0);
        }
    }

    #[test]
    fn performing_wants_progress() {
        let s = |o: &OutcomeView| success(Mode::Performing, o, &THR);
        assert_eq!(s(&ok()), 1.0);
        assert_eq!(
            s(&OutcomeView {
                progress: 0.3,
                ..ok()
            }),
            0.3
        );
        assert_eq!(
            s(&OutcomeView {
                progress: 0.29,
                ..ok()
            }),
            0.0
        );
        assert_eq!(
            s(&OutcomeView {
                success: false,
                ..ok()
            }),
            -1.0
        );
        let rehearsed = OutcomeView {
            internal: Some(InternalAction::Rehearse),
            ..ok()
        };
        assert_eq!(s(&rehearsed), PARTIAL);
        assert_eq!(
            s(&OutcomeView {
                success: false,
                ..rehearsed
            }),
            -1.0
        );
    }

    #[test]
    fn optimizing_wants_decisions() {
        assert_eq!(success(Mode::Optimizing, &ok(), &THR), 0.0);
        let o = OutcomeView {
            o_decision: true,
            ..ok()
        };
        assert_eq!(success(Mode::Optimizing, &o, &THR), 1.0);
        assert_eq!(all_success(&o, &THR), [NOTHING_RETAINED, 0.0, 1.0, 1.0]);
    }
}

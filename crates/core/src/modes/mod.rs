//! 01 §3 Modes: the four modes, learnable lenses (#39) and sign-preserving
//! aggregation (spec §7).

mod aggregate;
mod lens;
pub mod success;

pub use aggregate::{f_sign, score};
pub use lens::{Features, Lens, LensParams, ModeNetwork, N_FEATURES, f as feature};
pub use success::{OutcomeView, Thresholds, all_success};

use serde::{Deserialize, Serialize};

/// The four modes (01 §3.1). Order is fixed: L, E, P, O.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    Learning,
    Exploring,
    Performing,
    Optimizing,
}

impl Mode {
    pub const ALL: [Mode; 4] = [
        Mode::Learning,
        Mode::Exploring,
        Mode::Performing,
        Mode::Optimizing,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

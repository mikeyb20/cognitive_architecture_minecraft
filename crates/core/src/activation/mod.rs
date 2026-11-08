//! 01 §1 Activation: curiosity C, suppression D, valence V and the activation
//! function g(C) (spec §5, §12, §16).

mod curiosity;
mod g;
pub mod signals;
mod valence;

pub use curiosity::{Curiosity, CuriosityParams};
pub use g::{
    Activation, GShape, LogActivation, PowerActivation, build_activation, external_drive_effort,
};
pub use signals::{SignalWeights, Signals};
pub use valence::{Affect, AffectParams, ValenceInputs};

//! Baseline arms (spec §24): A is a behaviour tree, B is our own minimal LLM
//! loop (#30), B+N adds a count-based novelty bonus. All share skills,
//! perception and candidates with arm C; only the decision layer differs.

pub mod arm_a;
pub mod arm_b;

pub use arm_a::ArmA;
pub use arm_b::{ArmB, ArmBConfig};
pub use host_api;

use host_api::{Candidate, Observation};

/// A decision layer: picks one of the host's candidates by index.
pub trait Policy {
    fn choose(&mut self, obs: &Observation, candidates: &[Candidate]) -> Option<usize>;

    /// LLM use so far, for arms that call one (spec §25).
    fn llm_stats(&self) -> Option<&llm::LlmStats> {
        None
    }
}

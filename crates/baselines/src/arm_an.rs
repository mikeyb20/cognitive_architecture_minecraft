//! Arm A+N: the behaviour tree with B+N's count bonus (spec §24; 07 M2),
//! the code-only steelman of "C is just a novelty bonus". It needs no LLM,
//! so the comparison can run in the sim at hundreds of seeds.
//!
//! It keeps arm A's danger, command, dusk and hunger branches (the same
//! survival floor). Otherwise it scores every candidate as
//! `goal + β/√(n+1) − (cost + risk)/2`, where `goal` is 1 for the tree's
//! own work pick and 0 for the rest, and n counts earlier picks of the
//! candidate's target (Bellemare et al. 2016). At β = 0 it is arm A; as β
//! grows, novelty outweighs the goal.

use host_api::{Candidate, Observation};

use crate::Policy;
use crate::arm_a::ArmA;
use crate::arm_b::Counts;
use crate::arm_b::novelty::value;

/// Arm A+N.
#[derive(Debug, Clone, Default)]
pub struct ArmAN {
    pub tree: ArmA,
    /// The bonus weight β.
    pub beta: f32,
    counts: Counts,
}

impl ArmAN {
    pub fn new(beta: f32) -> Self {
        Self {
            beta,
            ..Self::default()
        }
    }

    /// Visit counts so far.
    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// The free choice: the tree's work pick gets +1, every candidate its
    /// novelty value; best first, the host's order on ties.
    fn free(&self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        let goal = self.tree.work(obs, c);
        let score =
            |i: usize| f32::from(u8::from(goal == Some(i))) + value(&c[i], &self.counts, self.beta);
        (0..c.len()).reduce(|best, i| if score(i) > score(best) { i } else { best })
    }
}

impl Policy for ArmAN {
    fn choose(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        let pick = self.tree.urgent(obs, c).or_else(|| self.free(obs, c))?;
        self.counts.visit(&c[pick]);
        Some(pick)
    }
}

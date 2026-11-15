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
//!
//! [`CountBy::Region`] counts states instead: n is how many decisions the
//! agent has made in the 8×8 region a candidate leads to (its own region
//! for a candidate with no place), the novelty unit C's novelty signal uses
//! (spec §5, §21). That is the closer analogue of a state-count bonus.

use host_api::ontology::region_of;
use host_api::{Candidate, Observation, Target};

use crate::Policy;
use crate::arm_a::ArmA;
use crate::arm_b::Counts;
use crate::arm_b::novelty::value;

/// What n counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CountBy {
    /// Earlier picks of the candidate's target (as B+N).
    #[default]
    Target,
    /// Earlier decisions made in the region the candidate leads to.
    Region,
}

/// Arm A+N.
#[derive(Debug, Clone, Default)]
pub struct ArmAN {
    pub tree: ArmA,
    /// The bonus weight β.
    pub beta: f32,
    pub count_by: CountBy,
    counts: Counts,
}

/// The region a candidate leads to: its place's, else where the agent is.
fn region_key(obs: &Observation, x: &Candidate) -> String {
    let at = match x.target {
        Some(Target::Pos(p)) => p,
        _ => obs.position,
    };
    region_of(at).0
}

impl ArmAN {
    pub fn new(beta: f32) -> Self {
        Self {
            beta,
            ..Self::default()
        }
    }

    /// A+N counting regions ([`CountBy::Region`]).
    pub fn by_region(beta: f32) -> Self {
        Self {
            beta,
            count_by: CountBy::Region,
            ..Self::default()
        }
    }

    /// A candidate's novelty value: bonus minus the mean of cost and risk.
    fn value(&self, obs: &Observation, x: &Candidate) -> f32 {
        match self.count_by {
            CountBy::Target => value(x, &self.counts, self.beta),
            CountBy::Region => {
                let n = self.counts.count_key(&region_key(obs, x));
                self.beta / ((n + 1) as f32).sqrt() - (x.est_cost + x.est_risk) / 2.0
            }
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
        let score = |i: usize| f32::from(u8::from(goal == Some(i))) + self.value(obs, &c[i]);
        (0..c.len()).reduce(|best, i| if score(i) > score(best) { i } else { best })
    }
}

impl Policy for ArmAN {
    fn choose(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        let pick = self.tree.urgent(obs, c).or_else(|| self.free(obs, c))?;
        match self.count_by {
            CountBy::Target => self.counts.visit(&c[pick]),
            CountBy::Region => self.counts.visit_key(&region_of(obs.position).0),
        }
        Some(pick)
    }
}

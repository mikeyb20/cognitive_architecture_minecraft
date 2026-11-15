//! B+N's count bonus (spec §24): `β/√(n+1)`, where n is how often the
//! candidate's target has been chosen before (Bellemare et al. 2016 style).

use std::collections::BTreeMap;

use host_api::{Candidate, Target};

/// Visit counts per target.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    n: BTreeMap<String, u32>,
}

/// What counts as "the same target": the place, thing, item or player;
/// skills with no target count per skill.
pub(crate) fn target_key(c: &Candidate) -> String {
    match &c.target {
        None => format!("skill:{}", c.skill.name()),
        Some(Target::Pos(p)) => format!("pos:{},{},{}", p.x, p.y, p.z),
        Some(Target::Thing(t)) => t.0.clone(),
        Some(Target::Item(i)) => format!("item:{i}"),
        Some(Target::Player(p)) => format!("player:{p}"),
    }
}

impl Counts {
    /// Times this candidate's target has been chosen.
    pub fn count(&self, c: &Candidate) -> u32 {
        self.count_key(&target_key(c))
    }

    /// Times `key` has been counted.
    pub fn count_key(&self, key: &str) -> u32 {
        self.n.get(key).copied().unwrap_or(0)
    }

    /// Count `key` once more.
    pub fn visit_key(&mut self, key: &str) {
        *self.n.entry(key.to_string()).or_insert(0) += 1;
    }

    /// The bonus `β/√(n+1)`.
    pub fn bonus(&self, c: &Candidate, beta: f32) -> f32 {
        beta / ((self.count(c) + 1) as f32).sqrt()
    }

    /// Count one more choice of this candidate's target.
    pub fn visit(&mut self, c: &Candidate) {
        self.visit_key(&target_key(c));
    }
}

/// A candidate's novelty value: its bonus minus the mean of its cost and
/// risk (spec §24). B+N ranks by it; A+N adds it to its goal.
pub(crate) fn value(x: &Candidate, counts: &Counts, beta: f32) -> f32 {
    counts.bonus(x, beta) - (x.est_cost + x.est_risk) / 2.0
}

/// Candidate indices, best first, by [`value`] (ties keep the host's
/// order).
pub(crate) fn ranked(c: &[Candidate], counts: &Counts, beta: f32) -> Vec<usize> {
    let score = |x: &Candidate| value(x, counts, beta);
    let mut order: Vec<usize> = (0..c.len()).collect();
    order.sort_by(|&a, &b| score(&c[b]).total_cmp(&score(&c[a])));
    order
}

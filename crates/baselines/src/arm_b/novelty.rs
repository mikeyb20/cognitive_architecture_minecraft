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
    value_at(counts.count(x), x, beta)
}

/// [`value`] with the count `n` given: `β/√(n+1) − (cost + risk)/2`.
pub(crate) fn value_at(n: u32, x: &Candidate, beta: f32) -> f32 {
    beta / ((n + 1) as f32).sqrt() - (x.est_cost + x.est_risk) / 2.0
}

/// Candidate indices, best first, by [`value`] (ties keep the host's
/// order).
pub(crate) fn ranked(c: &[Candidate], counts: &Counts, beta: f32) -> Vec<usize> {
    let score = |x: &Candidate| value(x, counts, beta);
    let mut order: Vec<usize> = (0..c.len()).collect();
    order.sort_by(|&a, &b| score(&c[b]).total_cmp(&score(&c[a])));
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::{Skill, TaskType};

    #[test]
    fn value_at_is_the_bonus_minus_mean_cost_and_risk() {
        let x = Candidate {
            id: 0,
            skill: Skill::Explore,
            target: None,
            task_type: TaskType::Travel,
            est_cost: 0.3,
            est_risk: 0.5,
            commanded: false,
        };
        // 2/√4 − (0.3 + 0.5)/2 = 1 − 0.4.
        assert!((value_at(3, &x, 2.0) - 0.6).abs() < 1e-6);
        // n = 0: the full β.
        assert!((value_at(0, &x, 2.0) - 1.6).abs() < 1e-6);
        let mut counts = Counts::default();
        counts.visit(&x);
        counts.visit(&x);
        counts.visit(&x);
        assert_eq!(value(&x, &counts, 2.0), value_at(3, &x, 2.0));
    }
}

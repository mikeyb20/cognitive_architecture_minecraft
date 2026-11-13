//! 01 §2 Direction: thought momentum (01 §2.2), the shared state S and the
//! per-candidate features φ (spec §3).

pub mod features;

pub use features::{FeatureInputs, InternalExtras, PlanStep, target_key};

/// Running activation per memory item: `a_j ← a_j·e^(−Δt/τ) + activation`;
/// TM is the top-k by a_j (01 §2.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThoughtMomentum {
    pub k: usize,
    pub tau: f32,
    activation: Vec<(usize, f32)>,
}

impl ThoughtMomentum {
    pub fn new(k: usize, tau: f32) -> Self {
        Self {
            k,
            tau,
            activation: Vec::new(),
        }
    }

    /// Advance one tick, adding this tick's activation for each item id.
    pub fn tick(&mut self, activated: &[(usize, f32)]) {
        self.tick_n(1, activated);
    }

    /// `n` ticks of fade, then one round of activation (for decisions that
    /// span several world ticks).
    pub fn tick_n(&mut self, n: u64, activated: &[(usize, f32)]) {
        let fade = (-(n.max(1) as f32) / self.tau).exp();
        self.tick_n_inner(fade, activated);
    }

    fn tick_n_inner(&mut self, fade: f32, activated: &[(usize, f32)]) {
        for (_, a) in self.activation.iter_mut() {
            *a *= fade;
        }
        for &(id, add) in activated {
            match self.activation.iter_mut().find(|(i, _)| *i == id) {
                Some((_, a)) => *a += add,
                None => self.activation.push((id, add)),
            }
        }
        self.activation.retain(|(_, a)| *a > 1e-6);
    }

    /// Add activation to one item without a fade step.
    pub fn add(&mut self, id: usize, add: f32) {
        self.tick_n_inner(1.0, &[(id, add)]);
    }

    /// Current running activation a_j of one item (0 if absent).
    pub fn activation(&self, id: usize) -> f32 {
        self.activation
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(0.0, |(_, a)| *a)
    }

    /// Item ids in TM, strongest first.
    pub fn top(&self) -> Vec<usize> {
        let mut v = self.activation.clone();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(self.k).map(|(id, _)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_items_displace_faded_ones() {
        let mut tm = ThoughtMomentum::new(2, 10.0);
        tm.tick(&[(1, 1.0), (2, 1.0), (3, 0.5)]);
        assert_eq!(tm.top(), vec![1, 2]);
        for _ in 0..30 {
            tm.tick(&[]);
        }
        tm.tick(&[(3, 0.5)]);
        assert_eq!(tm.top()[0], 3, "fresh activation beats faded items");
        assert!(tm.top().len() <= 2);
    }

    #[test]
    fn tick_n_fades_n_ticks_and_add_does_not_fade() {
        let mut a = ThoughtMomentum::new(3, 10.0);
        let mut b = a.clone();
        a.tick(&[(1, 1.0)]);
        b.tick(&[(1, 1.0)]);
        a.tick_n(5, &[]);
        for _ in 0..5 {
            b.tick(&[]);
        }
        let expect = (-0.5f32).exp();
        assert!(
            (a.activation(1) - expect).abs() < 1e-6,
            "{}",
            a.activation(1)
        );
        assert!((a.activation(1) - b.activation(1)).abs() < 1e-6);
        a.add(1, 0.25);
        a.add(2, 0.5);
        assert!((a.activation(1) - (expect + 0.25)).abs() < 1e-6);
        assert_eq!(a.activation(2), 0.5);
        assert_eq!(a.activation(9), 0.0);
        a.tick_n(0, &[]);
        assert!(
            (a.activation(2) - 0.5 * (-0.1f32).exp()).abs() < 1e-6,
            "n = 0 fades once"
        );
    }
}

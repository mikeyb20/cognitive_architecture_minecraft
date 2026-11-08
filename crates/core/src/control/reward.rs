//! Meta-reward R, 01 §4.6 and spec §13.

/// `R = Σ_i w_i·success_i + k_g·Δgoal_progress`, clamped to [−1, 1].
/// Each success_i ∈ [−1, 1] comes from that mode's own success function.
pub fn meta_reward(w: &[f32; 4], success: &[f32; 4], d_goal: f32, k_g: f32) -> f32 {
    let s: f32 = w
        .iter()
        .zip(success)
        .map(|(wi, si)| wi * si.clamp(-1.0, 1.0))
        .sum();
    (s + k_g * d_goal).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn in_range(
            raw in proptest::array::uniform4(0.01f32..1.0),
            s in proptest::array::uniform4(-1.0f32..=1.0),
            dg in -1.0f32..=1.0,
        ) {
            let t: f32 = raw.iter().sum();
            let w = raw.map(|r| r / t);
            let r = meta_reward(&w, &s, dg, 0.5);
            prop_assert!((-1.0..=1.0).contains(&r));
        }
    }

    #[test]
    fn weighted_by_mode_weight() {
        let w = [0.0, 0.0, 1.0, 0.0];
        assert_eq!(meta_reward(&w, &[-1.0, -1.0, 0.8, -1.0], 0.0, 0.5), 0.8);
    }
}

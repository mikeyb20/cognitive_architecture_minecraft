//! Sign-preserving aggregation (01 §3.2, §3.3).

/// `f(m) = m·|m|`: keeps the sign, so a veto still pushes away (01 §3.2).
pub fn f_sign(m: f32) -> f32 {
    m * m.abs()
}

/// `score(a) = Σ_i w_i·f(m_i(a))` (01 §3.3). `m` may contain kicked scores;
/// that is the only place kicked scores are used (spec §4 line 12).
pub fn score(w: &[f32; 4], m: &[f32; 4]) -> f32 {
    w.iter().zip(m).map(|(wi, mi)| wi * f_sign(*mi)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn veto_example_from_01() {
        // 01 §3.2: Performing −0.7 outweighs Exploring +0.4 at equal weight.
        let w = [0.25; 4];
        assert!(score(&w, &[0.0, 0.4, -0.7, 0.0]) < 0.0);
    }

    proptest! {
        #[test]
        fn f_keeps_sign(m in -1.0f32..=1.0) {
            if m != 0.0 {
                prop_assert_eq!(f_sign(m).signum(), m.signum());
            }
            prop_assert!(f_sign(m).abs() <= 1.0);
        }

        #[test]
        fn score_bounded(
            m in proptest::array::uniform4(-1.0f32..=1.0),
            raw in proptest::array::uniform4(0.01f32..1.0),
        ) {
            let s: f32 = raw.iter().sum();
            let w = raw.map(|r| r / s);
            prop_assert!(score(&w, &m).abs() <= 1.0 + 1e-6);
        }
    }
}

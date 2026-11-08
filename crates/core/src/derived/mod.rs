//! 01 §6 Derived measures (spec §18).

/// `creative(a) = [novelty(a | memory) > θ] AND [m_O(a) > 0]` (01 §6.1,
/// formula proposed; θ is OQ20). Kept after Optimizing reviews it means the
/// Optimizing lens did not veto it.
pub fn is_creative(novelty: f32, theta: f32, m_optimizing: f32) -> bool {
    novelty > theta && m_optimizing > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn novel_and_kept() {
        assert!(is_creative(0.8, 0.5, 0.2));
        assert!(!is_creative(0.8, 0.5, -0.2), "vetoed by Optimizing");
        assert!(!is_creative(0.3, 0.5, 0.9), "not new");
    }
}

//! The brain must be deterministic given a seed (spec §0), so runs can be
//! replayed and compared.

use cognitive_core::control::kick::kick_extremes;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

#[test]
fn same_seed_same_kicks() {
    let run = |seed| {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut m = vec![0.95f32; 1000];
        kick_extremes(&mut m, 0.9, 0.3, &mut rng);
        m
    };
    assert_eq!(run(42), run(42));
    assert_ne!(run(42), run(43));
}

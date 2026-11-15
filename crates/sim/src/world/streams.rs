//! Separate seeded random streams (07 M4, spec §21 audit gap 10): one per
//! source of chance in the world, so two arms run on the same seed see the
//! same map, and an action roll one arm makes does not shift the mob spawns
//! the other arm meets. Arms can then be compared seed by seed (common
//! random numbers).
//!
//! Every stream is ChaCha8 seeded with the run seed and its own stream
//! number, so a seed still replays exactly.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// The sources of chance in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stream {
    /// Map generation and the starting animals (drawn once, at reset).
    Map,
    /// Whether a hostile spawns this tick, where, and which kind.
    Spawn,
    /// Wandering mobs' steps.
    Wander,
    /// The agent's skill rolls (spec §21 success chance).
    Act,
    /// Skeleton arrows hitting.
    Arrow,
    /// How much an animal drops.
    Loot,
}

impl Stream {
    const ALL: [Stream; 6] = [
        Stream::Map,
        Stream::Spawn,
        Stream::Wander,
        Stream::Act,
        Stream::Arrow,
        Stream::Loot,
    ];

    /// The ChaCha stream number. The map keeps stream 0, the one a plain
    /// seeded generator uses, so every seed still makes the map (and the
    /// starting animals) it made with one stream.
    fn number(self) -> u64 {
        Self::ALL
            .iter()
            .position(|s| *s == self)
            .map_or(0, |i| i as u64)
    }
}

/// One generator per [`Stream`].
#[derive(Debug, Clone)]
pub(crate) struct Streams {
    rngs: [ChaCha8Rng; 6],
}

impl Streams {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            rngs: Stream::ALL.map(|s| {
                let mut r = ChaCha8Rng::seed_from_u64(seed);
                r.set_stream(s.number());
                r
            }),
        }
    }

    pub(crate) fn rng(&mut self, s: Stream) -> &mut ChaCha8Rng {
        &mut self.rngs[s.number() as usize]
    }

    /// True with probability `p`, drawn from stream `s`.
    pub(crate) fn chance(&mut self, s: Stream, p: f32) -> bool {
        self.rng(s).random::<f32>() < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(st: &mut Streams, s: Stream) -> Vec<u32> {
        (0..4).map(|_| st.rng(s).random()).collect()
    }

    #[test]
    fn streams_replay_by_seed_and_differ_from_each_other() {
        let (mut a, mut b) = (Streams::new(7), Streams::new(7));
        for s in Stream::ALL {
            assert_eq!(draws(&mut a, s), draws(&mut b, s), "{s:?} replays");
        }
        let mut c = Streams::new(7);
        let firsts: Vec<Vec<u32>> = Stream::ALL.iter().map(|&s| draws(&mut c, s)).collect();
        for i in 0..firsts.len() {
            for j in i + 1..firsts.len() {
                assert_ne!(firsts[i], firsts[j], "streams {i} and {j}");
            }
        }
        assert_ne!(draws(&mut Streams::new(8), Stream::Act), firsts[3]);
    }

    #[test]
    fn drawing_from_one_stream_leaves_the_others_where_they_were() {
        let (mut a, mut b) = (Streams::new(3), Streams::new(3));
        for _ in 0..100 {
            a.chance(Stream::Act, 0.5);
        }
        assert_eq!(draws(&mut a, Stream::Spawn), draws(&mut b, Stream::Spawn));
        assert_eq!(Stream::Map.number(), 0);
        assert_eq!(Stream::Loot.number(), 5);
    }

    #[test]
    fn the_map_stream_is_the_plain_seeded_generator() {
        let mut plain = ChaCha8Rng::seed_from_u64(9);
        let want: Vec<u32> = (0..4).map(|_| plain.random()).collect();
        assert_eq!(draws(&mut Streams::new(9), Stream::Map), want);
    }

    #[test]
    fn chance_is_never_true_at_zero_and_always_at_one() {
        let mut st = Streams::new(1);
        for _ in 0..200 {
            assert!(!st.chance(Stream::Arrow, 0.0));
            assert!(st.chance(Stream::Arrow, 1.0));
        }
        let hits = (0..2000).filter(|_| st.chance(Stream::Act, 0.3)).count();
        assert!((500..700).contains(&hits), "{hits}");
    }
}

//! Mobs: hostile zombies (night and caves) and pigs (food).

use host_api::Pos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobKind {
    Zombie,
    Pig,
}

impl MobKind {
    pub fn name(self) -> &'static str {
        match self {
            MobKind::Zombie => "zombie",
            MobKind::Pig => "pig",
        }
    }

    pub fn hostile(self) -> bool {
        self == MobKind::Zombie
    }

    pub fn max_health(self) -> f32 {
        match self {
            MobKind::Zombie => 1.0,
            MobKind::Pig => 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mob {
    pub kind: MobKind,
    pub pos: Pos,
    pub health: f32,
    /// Next tick this mob may move.
    pub next_move: u64,
    /// Next tick this mob may hit (hostiles only).
    pub next_hit: u64,
}

impl Mob {
    pub fn new(kind: MobKind, pos: Pos, tick: u64) -> Self {
        Self {
            kind,
            pos,
            health: kind.max_health(),
            next_move: tick,
            next_hit: tick,
        }
    }
}

/// Chebyshev distance: king-move steps between two tiles.
pub fn cheb(a: Pos, b: Pos) -> i32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

/// Straight-line distance between two tiles.
pub fn dist(a: Pos, b: Pos) -> f32 {
    let dx = (a.x - b.x) as f32;
    let dz = (a.z - b.z) as f32;
    (dx * dx + dz * dz).sqrt()
}

/// Greedy chase steps toward `to`: the longer axis first, then the other.
pub fn chase_steps(from: Pos, to: Pos) -> [(i32, i32); 2] {
    let dx = (to.x - from.x).signum();
    let dz = (to.z - from.z).signum();
    if (to.x - from.x).abs() >= (to.z - from.z).abs() {
        [(dx, 0), (0, dz)]
    } else {
        [(0, dz), (dx, 0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, z: i32) -> Pos {
        Pos { x, y: 0, z }
    }

    #[test]
    fn kinds() {
        assert!(MobKind::Zombie.hostile() && !MobKind::Pig.hostile());
        assert_eq!(MobKind::Zombie.name(), "zombie");
        assert_eq!(MobKind::Pig.name(), "pig");
        assert_eq!(MobKind::Zombie.max_health(), 1.0);
        assert_eq!(MobKind::Pig.max_health(), 0.5);
        let m = Mob::new(MobKind::Pig, p(1, 2), 7);
        assert_eq!((m.health, m.next_move, m.next_hit), (0.5, 7, 7));
    }

    #[test]
    fn distances() {
        assert_eq!(cheb(p(0, 0), p(3, -5)), 5);
        assert_eq!(cheb(p(2, 2), p(2, 2)), 0);
        assert_eq!(dist(p(0, 0), p(3, 4)), 5.0);
    }

    #[test]
    fn chase_prefers_longer_axis() {
        assert_eq!(chase_steps(p(0, 0), p(5, 2)), [(1, 0), (0, 1)]);
        assert_eq!(chase_steps(p(0, 0), p(-2, -5)), [(0, -1), (-1, 0)]);
        assert_eq!(chase_steps(p(0, 0), p(3, -3)), [(1, 0), (0, -1)]);
    }
}

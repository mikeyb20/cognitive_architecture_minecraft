//! Skeletons (Michael, 15 Nov 2026: "run from a skeleton"; live runs
//! 14 Nov: skeletons at range were the top killer). A skeleton shoots the
//! unsheltered agent from up to `shoot_range` tiles when nothing between
//! them blocks sight, holds its ground while it can shoot, backs off when
//! the agent comes within `keep_away`, and otherwise closes in like a
//! zombie. So running a few tiles does not help against it; breaking its
//! line of sight does (minecraft.fandom.com/wiki/Skeleton).

use host_api::Pos;

use super::{SimWorld, Stream};
use crate::geom::{between, cheb, dist, neighbours4};
use crate::mob::Mob;
use crate::world::Death;

impl SimWorld {
    /// Whether a skeleton at `from` can shoot a target at `to`: within
    /// `shoot_range` (straight-line) and no tile between them blocks sight.
    pub(crate) fn sees(&self, from: Pos, to: Pos) -> bool {
        dist(from, to) <= self.rules.shoot_range as f32
            && between(from, to)
                .iter()
                .all(|p| !self.map.get(*p).blocks_sight())
    }

    /// Whether mob `m` threatens the agent: a hostile within
    /// `threat_range`, or a skeleton that can shoot it from further out.
    pub(crate) fn threatens(&self, m: &Mob) -> bool {
        let me = self.body.pos;
        m.kind.hostile()
            && (cheb(m.pos, me) <= self.rules.threat_range
                || (m.kind.ranged() && self.sees(m.pos, me)))
    }

    /// Whether any skeleton could shoot the agent if it stood at `p`.
    pub(crate) fn exposed_at(&self, p: Pos) -> bool {
        self.mobs
            .iter()
            .any(|m| m.kind.ranged() && self.sees(m.pos, p))
    }

    /// Mob `i` (a skeleton) shoots if its shot is due and it sees the
    /// unsheltered agent; the arrow hits with `arrow_hit`.
    pub(super) fn shoot(&mut self, i: usize, sheltered: bool) {
        let m = &self.mobs[i];
        if sheltered || self.tick < m.next_hit || !self.sees(m.pos, self.body.pos) {
            return;
        }
        self.mobs[i].next_hit = self.tick + self.rules.arrow_ticks;
        if self.rng.chance(Stream::Arrow, self.rules.arrow_hit) {
            self.hurt(self.rules.mob_damage, Death::Arrow);
        }
    }

    /// A chasing skeleton's move, if it does not simply close in: back off
    /// (the steps that most raise its distance) when the agent is nearer
    /// than `keep_away`, hold (no steps) while it can shoot. `None` for a
    /// zombie, or a skeleton that must close in to see the agent.
    pub(super) fn skeleton_steps(&self, i: usize) -> Option<Vec<(i32, i32)>> {
        let m = &self.mobs[i];
        let me = self.body.pos;
        if !m.kind.ranged() {
            return None;
        }
        if cheb(m.pos, me) < self.rules.keep_away {
            let here = dist(m.pos, me);
            let mut away: Vec<(f32, (i32, i32))> = neighbours4(m.pos)
                .into_iter()
                .map(|p| (dist(p, me), (p.x - m.pos.x, p.z - m.pos.z)))
                .filter(|(d, _)| *d > here)
                .collect();
            away.sort_by(|a, b| b.0.total_cmp(&a.0));
            return Some(away.into_iter().map(|(_, s)| s).collect());
        }
        self.sees(m.pos, me).then(Vec::new)
    }
}

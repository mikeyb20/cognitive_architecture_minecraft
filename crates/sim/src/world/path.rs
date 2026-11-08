//! Paths and reach (spec §21): breadth-first search over walkable tiles,
//! 4-way moves, within `PATH_RADIUS` of the agent.

use std::collections::VecDeque;

use host_api::{Pos, World};

use super::{PATH_RADIUS, SimWorld};
use crate::geom::{cheb, neighbours4, ring, square};

/// Path lengths from one origin (see [`SimWorld::reach`]).
#[derive(Debug, Clone)]
pub struct Reach {
    origin: Pos,
    side: i32,
    steps: Vec<u16>,
}

impl Reach {
    fn idx(&self, p: Pos) -> Option<usize> {
        let x = p.x - self.origin.x + PATH_RADIUS;
        let z = p.z - self.origin.z + PATH_RADIUS;
        if x < 0 || z < 0 || x >= self.side || z >= self.side {
            return None;
        }
        Some((z * self.side + x) as usize)
    }

    /// Steps to stand on `p`, if reachable.
    pub fn steps(&self, p: Pos) -> Option<u16> {
        self.idx(p)
            .map(|i| self.steps[i])
            .filter(|d| *d != u16::MAX)
    }

    /// Steps to stand on or next to `p` (king move), if reachable.
    pub fn steps_adjacent(&self, p: Pos) -> Option<u16> {
        square(p, 1).filter_map(|q| self.steps(q)).min()
    }
}

impl SimWorld {
    /// Path lengths from the agent to every walkable tile within
    /// `PATH_RADIUS` (4-way moves).
    pub fn reach(&self) -> Reach {
        let o = self.body.pos;
        let side = 2 * PATH_RADIUS + 1;
        let mut r = Reach {
            origin: o,
            side,
            steps: vec![u16::MAX; (side * side) as usize],
        };
        let start = r.idx(o).expect("origin is inside its own window");
        r.steps[start] = 0;
        let mut q = VecDeque::from([o]);
        while let Some(p) = q.pop_front() {
            let d = r.steps[r.idx(p).expect("queued tiles are inside")];
            for s in neighbours4(p) {
                let Some(i) = r.idx(s) else { continue };
                if r.steps[i] == u16::MAX && self.map.in_bounds(s) && self.map.get(s).walkable() {
                    r.steps[i] = d + 1;
                    q.push_back(s);
                }
            }
        }
        r
    }

    /// A reachable tile within 3 of `p` (nearest ring first), not the
    /// agent's own tile.
    fn reachable_near(&self, reach: &Reach, p: Pos) -> Option<Pos> {
        (0..=3)
            .flat_map(|r| ring(p, r))
            .find(|q| *q != self.body.pos && reach.steps(*q).is_some())
    }

    /// Shortest walkable path (4-way) from the agent to `goal`, or to a tile
    /// next to it (king move) when `adjacent`. Excludes the start tile.
    pub fn path_to(&self, goal: Pos, adjacent: bool) -> Option<Vec<Pos>> {
        let start = self.body.pos;
        let done = |p: Pos| {
            if adjacent {
                cheb(p, goal) <= 1
            } else {
                p == goal
            }
        };
        if done(start) {
            return Some(Vec::new());
        }
        let n = self.map.size;
        let mut parent = vec![-1i32; (n * n) as usize];
        let idx = |p: Pos| (p.z * n + p.x) as usize;
        let mut q = VecDeque::from([start]);
        parent[idx(start)] = idx(start) as i32;
        while let Some(p) = q.pop_front() {
            for s in neighbours4(p) {
                if !self.map.in_bounds(s)
                    || !self.map.get(s).walkable()
                    || parent[idx(s)] >= 0
                    || cheb(s, start) > PATH_RADIUS
                {
                    continue;
                }
                parent[idx(s)] = idx(p) as i32;
                if done(s) {
                    let mut path = vec![s];
                    let mut cur = idx(p);
                    while cur != idx(start) {
                        path.push(Pos {
                            x: cur as i32 % n,
                            y: 0,
                            z: cur as i32 / n,
                        });
                        cur = parent[cur] as usize;
                    }
                    path.reverse();
                    return Some(path);
                }
                q.push_back(s);
            }
        }
        None
    }

    fn tile_ticks(&self, effort: f32) -> u32 {
        if effort >= self.rules.sprint_effort {
            self.rules.sprint_ticks
        } else {
            self.rules.walk_ticks
        }
    }

    /// Walk to `goal` (or next to it). Returns whether it got there alive.
    pub(crate) fn walk_to(&mut self, goal: Pos, adjacent: bool, effort: f32) -> bool {
        let Some(path) = self.path_to(goal, adjacent) else {
            return false;
        };
        let per = self.tile_ticks(effort);
        for p in path {
            self.enter(p);
            self.step_world(per);
            if !self.alive() {
                return false;
            }
        }
        true
    }

    /// The point `max_move` tiles from the agent in direction (dx, dz),
    /// clamped to the map and snapped to a walkable tile.
    pub(crate) fn point_toward(&self, reach: &Reach, dx: i32, dz: i32) -> Option<Pos> {
        let m = self.rules.max_move;
        let last = self.map.size - 1;
        let p = Pos {
            x: (self.body.pos.x + dx * m).clamp(0, last),
            y: 0,
            z: (self.body.pos.z + dz * m).clamp(0, last),
        };
        self.reachable_near(reach, p)
    }
}

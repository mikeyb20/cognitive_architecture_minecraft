//! The shelter test (01 §4.11, #58; spec §27 step 12): a goal's test, the
//! condition a shelter must meet however it was built. "No hostile has a
//! path to me" and "I have a way out", computed with the same 4-way moves
//! the zombies chase by (#62).

use std::collections::{BTreeSet, VecDeque};

use host_api::Pos;

use super::SimWorld;
use crate::geom::{cheb, neighbours4};
use crate::map::Tile;

/// The shelter test's two parts (01 §4.11). The host reports pass or fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShelterTest {
    /// No hostile has a path to the agent: the tiles a mob could walk to
    /// the agent from all lie closer than `shelter_radius`, so nothing spawns
    /// among them, and none holds a hostile.
    pub enclosed: bool,
    /// The agent can walk out (through a door) as far as `shelter_radius`, into
    /// the open world.
    pub exit: bool,
}

impl ShelterTest {
    /// Both parts hold (01 §4.11).
    pub fn passes(self) -> bool {
        self.enclosed && self.exit
    }
}

impl SimWorld {
    /// Run the shelter test where the agent stands (01 §4.11, #58).
    pub fn shelter_test(&self) -> ShelterTest {
        let me = self.body.pos;
        let near = self.rules.shelter_radius;
        let inside = self.flood(me, near, Tile::mob_walkable);
        let enclosed = inside.iter().all(|&(x, z)| cheb(pos(x, z), me) < near)
            && !self
                .mobs
                .iter()
                .any(|m| m.kind.hostile() && inside.contains(&(m.pos.x, m.pos.z)));
        let exit = self
            .flood(me, near, Tile::walkable)
            .into_iter()
            .any(|(x, z)| cheb(pos(x, z), me) == near);
        ShelterTest { enclosed, exit }
    }

    /// Tiles reachable from `start` (always included) over tiles that pass
    /// `pass`, 4-way, within Chebyshev `radius` of it.
    fn flood(&self, start: Pos, radius: i32, pass: fn(Tile) -> bool) -> BTreeSet<(i32, i32)> {
        let mut seen = BTreeSet::from([(start.x, start.z)]);
        let mut q = VecDeque::from([start]);
        while let Some(p) = q.pop_front() {
            for s in neighbours4(p) {
                if cheb(s, start) > radius || !pass(self.map.get(s)) {
                    continue;
                }
                if seen.insert((s.x, s.z)) {
                    q.push_back(s);
                }
            }
        }
        seen
    }
}

fn pos(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

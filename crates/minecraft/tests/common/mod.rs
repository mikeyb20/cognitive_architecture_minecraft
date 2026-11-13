// Shared builders for the minecraft crate's tests.
#![allow(dead_code, reason = "each test file uses a different subset")]

use host_api::{AgentId, Inventory, Observation, Pos, Seen, SeenKind, ThingId};

pub fn at(x: i32, y: i32, z: i32) -> Pos {
    Pos { x, y, z }
}

pub fn seen(thing: &str, kind: SeenKind, me: Pos, p: Pos) -> Seen {
    let d = |u: i32, v: i32| (u - v) as f32;
    Seen {
        thing: ThingId(thing.into()),
        kind,
        pos: p,
        distance: (d(me.x, p.x).powi(2) + d(me.y, p.y).powi(2) + d(me.z, p.z).powi(2)).sqrt(),
    }
}

pub fn block(name: &str, me: Pos, p: Pos) -> Seen {
    seen(&format!("block:{name}"), SeenKind::Block, me, p)
}

pub fn obs(me: Pos, inv: &[(&str, u32)], visible: Vec<Seen>) -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.25,
        health: 1.0,
        food: 1.0,
        position: me,
        inventory: Inventory {
            items: inv.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        },
        visible,
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

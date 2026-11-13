//! What the agent can do now in Minecraft (spec §2, §6), built from the
//! observation and the place memory. The groups and their order mirror the
//! sim's handlers (threats, eating, shelter, gathering, hunting, crafting,
//! travel, then players, which the sim does not have), so every arm sees
//! the same kinds of choice in both worlds. At most K = 20; players are
//! last, so they are the ones cut.

use host_api::craft::{pick_tier, same_block, worth_crafting};
use host_api::{
    Candidate, MAX_CANDIDATES, Observation, Pos, Seen, SeenKind, Skill, Target, TaskType, ThingId,
};

use crate::Places;
use crate::dist;

/// Distances for Minecraft's scale (proposed until the pilot; the sim's
/// counterparts are tile counts in `sim::rules`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McParams {
    /// A hostile this close (blocks) is a threat: offer flee and attack.
    pub threat_range: f32,
    /// How far one flee runs.
    pub flee_distance: i32,
    /// How far one explore step goes; also scales cost.
    pub explore_distance: i32,
    /// Blocks needed in the inventory to wall in quickly.
    pub shelter_blocks: u32,
}

impl Default for McParams {
    fn default() -> Self {
        Self {
            threat_range: 8.0,
            flee_distance: 16,
            explore_distance: 24,
            shelter_blocks: 8,
        }
    }
}

/// Raw materials of the tech tree: skill, memory key, pickaxe tier needed.
const RESOURCES: [(Skill, TaskType, &str, u8); 4] = [
    (Skill::Chop, TaskType::Mining, "block:oak_log", 0),
    (Skill::Mine, TaskType::Mining, "block:stone", 1),
    (Skill::Mine, TaskType::Mining, "block:iron_ore", 2),
    (Skill::Farm, TaskType::Farming, "block:wheat", 0),
];

/// Food in the order it is worth eating.
const FOODS: [&str; 10] = [
    "cooked_beef",
    "cooked_porkchop",
    "bread",
    "cooked_chicken",
    "baked_potato",
    "apple",
    "carrot",
    "beef",
    "porkchop",
    "chicken",
];

/// Animals worth hunting for food.
const GAME: [&str; 4] = ["entity:pig", "entity:cow", "entity:chicken", "entity:sheep"];

/// Blocks that can wall the agent in.
const BUILDING: [&str; 3] = ["cobblestone", "dirt", "oak_planks"];

/// Costs and risks of a search with no known spot.
const SEARCH_COST: f32 = 0.6;
const SEARCH_RISK: f32 = 0.3;

struct Ctx<'a> {
    obs: &'a Observation,
    p: &'a McParams,
    out: Vec<Candidate>,
}

impl Ctx<'_> {
    fn add(&mut self, skill: Skill, target: Option<Target>, task: TaskType, cost: f32, risk: f32) {
        self.out.push(Candidate {
            id: self.out.len() as u32,
            skill,
            target,
            task_type: task,
            est_cost: cost.clamp(0.0, 1.0),
            est_risk: risk.clamp(0.0, 1.0),
            commanded: false,
        });
    }

    /// Cost of walking `d` blocks plus a base (as the sim's `cost`).
    fn cost(&self, d: f32, base: f32) -> f32 {
        base + d / (4 * self.p.explore_distance) as f32
    }

    /// Risk at `p` (as the sim's `risk_at`): night in the open, hostiles
    /// near it, lava next to it.
    fn risk_at(&self, p: Pos) -> f32 {
        let o = self.obs;
        let mut r = 0.0;
        if o.is_night() && !o.sheltered {
            r += 0.3;
        }
        let near = o
            .visible
            .iter()
            .filter(|s| is_hostile(s) && dist(s.pos, p) <= self.p.threat_range)
            .count();
        r += 0.15 * near as f32;
        if o.visible
            .iter()
            .any(|s| s.thing.0 == "block:lava" && dist(s.pos, p) <= 1.5)
        {
            r += 0.3;
        }
        r
    }

    fn nearest(&self, want: impl Fn(&Seen) -> bool) -> Option<&Seen> {
        self.obs
            .visible
            .iter()
            .filter(|s| want(s))
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    /// A point `d` blocks from the agent in direction (dx, dz).
    fn toward(&self, dx: f32, dz: f32, d: i32) -> Pos {
        let me = self.obs.position;
        let len = (dx * dx + dz * dz).sqrt().max(f32::EPSILON);
        Pos {
            x: me.x + (dx / len * d as f32).round() as i32,
            y: me.y,
            z: me.z + (dz / len * d as f32).round() as i32,
        }
    }
}

fn is_hostile(s: &Seen) -> bool {
    s.kind == (SeenKind::Entity { hostile: true })
}

fn threats(cx: &mut Ctx) {
    let Some(h) = cx
        .nearest(|s| is_hostile(s) && s.distance <= cx.p.threat_range)
        .cloned()
    else {
        return;
    };
    let me = cx.obs.position;
    let (dx, dz) = ((me.x - h.pos.x) as f32, (me.z - h.pos.z) as f32);
    let (dx, dz) = if dx == 0.0 && dz == 0.0 {
        (1.0, 0.0)
    } else {
        (dx, dz)
    };
    let away = cx.toward(dx, dz, cx.p.flee_distance);
    let flee = cx.cost(cx.p.flee_distance as f32, 0.1);
    cx.add(
        Skill::Flee,
        Some(Target::Pos(away)),
        TaskType::Travel,
        flee,
        0.2,
    );
    let fight = cx.cost(h.distance, 0.2);
    cx.add(
        Skill::Attack,
        Some(Target::Pos(h.pos)),
        TaskType::Combat,
        fight,
        0.6,
    );
}

fn eating(cx: &mut Ctx) {
    if cx.obs.food >= 1.0 {
        return;
    }
    if let Some(f) = FOODS.into_iter().find(|f| cx.obs.inventory.count(f) > 0) {
        cx.add(
            Skill::Eat,
            Some(Target::Item(f.into())),
            TaskType::Farming,
            0.05,
            0.0,
        );
    }
}

fn shelter(cx: &mut Ctx) {
    let o = cx.obs;
    if o.sheltered {
        cx.add(Skill::Rest, None, TaskType::Building, 0.1, 0.0);
        return;
    }
    let ready = BUILDING
        .iter()
        .any(|b| o.inventory.count(b) >= cx.p.shelter_blocks);
    let cost = if ready { 0.1 } else { 0.25 };
    let risk = cx.risk_at(o.position);
    cx.add(Skill::Shelter, None, TaskType::Building, cost, risk);
    let bed = cx
        .nearest(|s| s.kind == SeenKind::Block && s.thing.0.ends_with("_bed"))
        .map(|s| (s.pos, s.distance));
    if let Some((p, d)) = bed {
        let (cost, risk) = (cx.cost(d, 0.1), cx.risk_at(p));
        cx.add(
            Skill::Rest,
            Some(Target::Pos(p)),
            TaskType::Building,
            cost,
            risk,
        );
    }
}

/// The nearest known spot of each raw material the pickaxe allows: in
/// view, else remembered, else a search for it.
fn gathering(cx: &mut Ctx, places: &Places) {
    let tier = pick_tier(&cx.obs.inventory);
    let me = cx.obs.position;
    for (skill, task, thing, need) in RESOURCES {
        if tier < need {
            continue;
        }
        let seen = cx
            .nearest(|s| s.kind == SeenKind::Block && same_block(&s.thing.0, thing))
            .map(|s| s.pos);
        match seen.or_else(|| places.nearest(thing, me)) {
            Some(p) => {
                let (cost, risk) = (cx.cost(dist(me, p), 0.1), cx.risk_at(p));
                cx.add(skill, Some(Target::Pos(p)), task, cost, risk);
            }
            None => {
                let search = Some(Target::Thing(ThingId(thing.into())));
                cx.add(skill, search, task, SEARCH_COST, SEARCH_RISK);
            }
        }
    }
}

fn hunting(cx: &mut Ctx) {
    let game = cx
        .nearest(|s| GAME.contains(&s.thing.0.as_str()))
        .map(|s| (s.pos, s.distance));
    if let Some((p, d)) = game {
        let (cost, risk) = (cx.cost(d, 0.15), cx.risk_at(p));
        cx.add(
            Skill::Attack,
            Some(Target::Pos(p)),
            TaskType::Farming,
            cost,
            risk,
        );
    }
}

fn crafting(cx: &mut Ctx) {
    let inv = cx.obs.inventory.clone();
    for r in worth_crafting(&inv) {
        let item = Some(Target::Item(r.output.into()));
        cx.add(Skill::Craft, item, TaskType::Crafting, 0.05, 0.0);
    }
}

fn players(cx: &mut Ctx) {
    let seen: Vec<(String, f32)> = cx
        .obs
        .visible
        .iter()
        .filter(|s| s.kind == SeenKind::Player)
        .filter_map(|s| Some((s.thing.0.strip_prefix("player:")?.to_string(), s.distance)))
        .collect();
    for (name, d) in seen {
        let cost = cx.cost(d, 0.05);
        cx.add(
            Skill::FollowPlayer,
            Some(Target::Player(name)),
            TaskType::Helping,
            cost,
            0.0,
        );
    }
}

fn travel(cx: &mut Ctx) {
    let d = cx.p.explore_distance;
    for (dx, dz) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        let p = cx.toward(dx, dz, d);
        let (cost, risk) = (cx.cost(d as f32, 0.0), cx.risk_at(p));
        cx.add(
            Skill::Explore,
            Some(Target::Pos(p)),
            TaskType::Travel,
            cost,
            risk,
        );
    }
}

/// Every external candidate on offer now (spec §6), at most K = 20. Call
/// [`Places::see`] with the same observation first.
pub fn candidates(obs: &Observation, places: &Places, p: &McParams) -> Vec<Candidate> {
    let mut cx = Ctx {
        obs,
        p,
        out: Vec::new(),
    };
    threats(&mut cx);
    eating(&mut cx);
    shelter(&mut cx);
    gathering(&mut cx, places);
    hunting(&mut cx);
    crafting(&mut cx);
    travel(&mut cx);
    players(&mut cx);
    cx.out.truncate(MAX_CANDIDATES);
    cx.out
}

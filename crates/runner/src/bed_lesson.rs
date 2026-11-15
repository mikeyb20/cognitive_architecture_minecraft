//! The bed lesson (Michael, 15 Nov 2026: "sleeping in a bed at night should
//! also be a priority ... inside some sort of shelter"). Minecraft only: the
//! sim has no sheep or beds, so its runs are unchanged.
//!
//! Once a day from the afternoon (`ontology::BED_CALL`) until dusk, while
//! the agent has no bed, the player says "get bed". The teacher (01 §4.11,
//! #59) shows a recipe and explains a test, as for shelter:
//! - Recipe: hunt a sheep for each wool still wanted, then Shelter. The
//!   bot's Shelter makes the bed and puts it in the home.
//! - Test: a bed is in view.
//!
//! Live runs shaped it: put in the dusk shelter recipe, the hunt came
//! before the shelter every night; with one "explore" step for a sheep not
//! in view, the explore used the step up and no sheep was ever hunted. The recipe fades with ρ; the test does not.

use cognitive_core::control::teacher::Recipe;
use cognitive_core::direction::PlanStep;
use host_api::ontology::{BED_CALL, BED_GOAL, DUSK};
use host_api::{ChatMsg, Observation, SeenKind, Skill, TaskType};
use sim::player::PLAYER;

/// Wool of one colour a bed takes (Minecraft 1.21's recipe, with three planks).
pub const BED_WOOL: u32 = 3;
const SHEEP: &str = "entity:sheep";

/// The lesson's test: a bed (`*_bed` block) is in view.
pub fn test(obs: &Observation) -> bool {
    obs.visible
        .iter()
        .any(|s| s.kind == SeenKind::Block && s.thing.0.ends_with("_bed"))
}

/// Wool still to get for a bed: none with a bed held, else what the
/// best-stocked colour lacks of `BED_WOOL`.
fn wool_wanted(obs: &Observation) -> u32 {
    let inv = &obs.inventory;
    let bed = inv
        .items
        .iter()
        .any(|(name, _)| name.ends_with("_bed") && inv.count(name) > 0);
    let most = inv
        .items
        .iter()
        .filter(|(name, _)| name.ends_with("_wool"))
        .map(|(name, _)| inv.count(name))
        .max()
        .unwrap_or(0);
    if bed {
        0
    } else {
        BED_WOOL.saturating_sub(most)
    }
}

/// The recipe (module doc): one sheep hunted for each wool still wanted
/// (a sheep drops one), then Shelter. A recipe is shown once per goal and
/// a done step stays done, so every hunt is its own step; with no sheep in
/// view no hunt is offered and the brain looks around on its own.
pub fn recipe(obs: &Observation) -> Recipe {
    let step = |skill, thing: &str, task| PlanStep {
        skill,
        thing: thing.to_string(),
        task,
    };
    let hunt = step(Skill::Attack, SHEEP, TaskType::Farming);
    let mut steps = vec![hunt; wool_wanted(obs) as usize];
    steps.push(step(Skill::Shelter, "", TaskType::Building));
    Recipe {
        goal: BED_GOAL.to_string(),
        steps,
    }
}

/// The player's daily bed call (module doc).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BedCall {
    called_day: Option<u64>,
}

impl BedCall {
    /// Say "get bed" into `obs` if it is time and no bed is in view;
    /// returns whether it did. A held bed still gets the call: Shelter
    /// puts it in the home.
    pub fn call(&mut self, obs: &mut Observation, day_ticks: u64) -> bool {
        let day = obs.tick / day_ticks;
        let time = (BED_CALL..DUSK).contains(&obs.day_phase);
        if !time || test(obs) || self.called_day == Some(day) {
            return false;
        }
        self.called_day = Some(day);
        obs.chat.push(ChatMsg {
            from: PLAYER.into(),
            text: format!("get {BED_GOAL}"),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taught::tests::obs;
    use host_api::{Pos, Seen, ThingId};

    const DAY: u64 = 24_000;

    /// A view holding `items`, with `seen` things in it (blocks if `block:`).
    fn view(items: &[(&str, u32)], seen: &[&str]) -> Observation {
        let mut o = obs(&[], items);
        o.visible = seen
            .iter()
            .map(|t| Seen {
                thing: ThingId(t.to_string()),
                kind: if t.starts_with("block:") {
                    SeenKind::Block
                } else {
                    SeenKind::Entity { hostile: false }
                },
                pos: Pos { x: 3, y: 64, z: 0 },
                distance: 3.0,
            })
            .collect();
        o
    }

    fn skills(r: &Recipe) -> Vec<(Skill, &str, TaskType)> {
        r.steps
            .iter()
            .map(|s| (s.skill, s.thing.as_str(), s.task))
            .collect()
    }

    const SHELTER: (Skill, &str, TaskType) = (Skill::Shelter, "", TaskType::Building);

    #[test]
    fn the_recipe_hunts_a_sheep_per_wool_wanted_then_shelters() {
        let hunt = (Skill::Attack, "entity:sheep", TaskType::Farming);
        let r = recipe(&view(&[], &["entity:pig"]));
        assert_eq!(r.goal, "bed");
        assert_eq!(skills(&r), [hunt, hunt, hunt, SHELTER]);
        let n = |items: &[(&str, u32)]| recipe(&view(items, &[])).steps.len() - 1;
        assert_eq!(
            n(&[("white_wool", 2), ("red_wool", 1)]),
            1,
            "the best colour"
        );
        assert_eq!(n(&[("lime_wool", 3)]), 0);
        assert_eq!(n(&[("lime_wool", 7)]), 0);
        assert_eq!(
            n(&[("white_wool", 1), ("white_wool", 1)]),
            1,
            "one colour, two stacks"
        );
        assert_eq!(n(&[("red_bed", 1)]), 0);
        assert_eq!(n(&[("red_bed", 0), ("white_wool", 2)]), 1);
        assert_eq!(
            n(&[("bedrock", 5), ("woolly", 5)]),
            3,
            "names that only look alike"
        );
    }

    #[test]
    fn hunts_are_exactly_the_wool_a_bed_still_needs() {
        // Property over a grid of holdings.
        for bed in 0..3 {
            for white in 0..5 {
                for red in 0..5 {
                    let items = [("white_bed", bed), ("white_wool", white), ("red_wool", red)];
                    let want = if bed > 0 {
                        0
                    } else {
                        BED_WOOL.saturating_sub(white.max(red))
                    };
                    let r = recipe(&view(&items, &[SHEEP]));
                    assert_eq!(r.steps.len() as u32, want + 1, "{items:?}");
                }
            }
        }
    }

    #[test]
    fn the_test_is_a_bed_in_view() {
        assert!(test(&view(&[], &["block:white_bed"])));
        assert!(
            !test(&view(&[("white_bed", 1)], &["block:oak_log"])),
            "held is not placed"
        );
        assert!(
            !test(&view(&[], &["entity:white_bed"])),
            "a block, not a thing"
        );
    }

    #[test]
    fn the_player_asks_for_a_bed_once_each_afternoon_until_one_is_in_view() {
        let at = |tick: u64, phase: f32, seen: &[&str]| {
            let mut o = view(&[], seen);
            o.tick = tick;
            o.day_phase = phase;
            o.chat.clear();
            o
        };
        let mut c = BedCall::default();
        assert!(!c.call(&mut at(10, 0.2, &[]), DAY), "morning");
        let mut o = at(20, BED_CALL, &[]);
        assert!(c.call(&mut o, DAY));
        assert_eq!(o.chat.len(), 1);
        assert_eq!(
            (o.chat[0].from.as_str(), o.chat[0].text.as_str()),
            ("steve", "get bed")
        );
        assert!(!c.call(&mut at(30, 0.3, &[]), DAY), "once a day");
        assert!(
            !c.call(&mut at(DAY + 10, DUSK, &[]), DAY),
            "dusk is the shelter call's"
        );
        assert!(
            !c.call(&mut at(DAY + 20, 0.3, &["block:red_bed"]), DAY),
            "a bed in view"
        );
        assert!(
            c.call(&mut at(DAY + 30, DUSK - 0.01, &[]), DAY),
            "the next day"
        );
    }
}

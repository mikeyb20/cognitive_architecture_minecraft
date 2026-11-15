//! Shared ontology (spec §2). Both worlds use Minecraft's names, so memories can
//! move between worlds (expansion item X17).

use serde::{Deserialize, Serialize};

/// External skills a host can execute (spec §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Skill {
    /// Head for ground not yet seen.
    Explore,
    /// Walk to a target.
    MoveTo,
    /// Break stone or ore with a pickaxe.
    Mine,
    /// Cut a tree for logs.
    Chop,
    /// Make an item from the tech tree.
    Craft,
    /// Put a block down.
    Place,
    /// Fight a mob.
    Attack,
    /// Run from a hostile.
    Flee,
    /// Eat food from the inventory.
    Eat,
    /// Harvest or plant wheat.
    Farm,
    /// Build or dig into an enclosure.
    Shelter,
    /// Stay put and rest.
    Rest,
    /// Keep near a player.
    FollowPlayer,
    /// Say something in chat.
    Chat,
}

impl Skill {
    /// Every skill, in declaration order.
    pub const ALL: [Skill; 14] = [
        Skill::Explore,
        Skill::MoveTo,
        Skill::Mine,
        Skill::Chop,
        Skill::Craft,
        Skill::Place,
        Skill::Attack,
        Skill::Flee,
        Skill::Eat,
        Skill::Farm,
        Skill::Shelter,
        Skill::Rest,
        Skill::FollowPlayer,
        Skill::Chat,
    ];

    /// The wire name (spec §2), as serde writes it; used in LLM prompts.
    pub fn name(self) -> &'static str {
        match self {
            Skill::Explore => "explore",
            Skill::MoveTo => "move_to",
            Skill::Mine => "mine",
            Skill::Chop => "chop",
            Skill::Craft => "craft",
            Skill::Place => "place",
            Skill::Attack => "attack",
            Skill::Flee => "flee",
            Skill::Eat => "eat",
            Skill::Farm => "farm",
            Skill::Shelter => "shelter",
            Skill::Rest => "rest",
            Skill::FollowPlayer => "follow_player",
            Skill::Chat => "chat",
        }
    }
}

/// The 7 tenacity task types (#36). Finer splits only if logs show a type
/// behaves differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TaskType {
    /// Breaking stone and ore.
    Mining,
    /// Shelter, resting and placing blocks.
    Building,
    /// Fighting.
    Combat,
    /// Making items.
    Crafting,
    /// Food: harvesting, hunting, eating.
    Farming,
    /// Moving: exploring, walking, fleeing.
    Travel,
    /// Doing what a player asked.
    Helping,
}

impl TaskType {
    /// Every task type, in declaration order.
    pub const ALL: [TaskType; 7] = [
        TaskType::Mining,
        TaskType::Building,
        TaskType::Combat,
        TaskType::Crafting,
        TaskType::Farming,
        TaskType::Travel,
        TaskType::Helping,
    ];

    /// Index for one-hot encodings (Oracle context, spec §8).
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Side of a novelty region in tiles (spec §21: 8×8). Both worlds name
/// regions the same way, so region memories carry over (X17).
pub const REGION: i32 = 8;

/// Day phase (0..1) from which the routine survival tree shelters (v0.2
/// survival tree; spec §20, arm A). The sim planner and the dusk call use
/// it too.
pub const DUSK: f32 = 0.42;

/// The mob lessons the teacher knows (01 §4.11; Michael, 15 Nov 2026:
/// "zombie: fight; skeleton: hide"). Each has a recipe and a test: fight
/// passes once no zombie is within threat range, hide once no skeleton can
/// shoot the agent.
pub const FIGHT_ZOMBIE_GOAL: &str = "fight_zombie";
/// See [`FIGHT_ZOMBIE_GOAL`].
pub const HIDE_SKELETON_GOAL: &str = "hide_from_skeleton";
/// The zombie lesson for a weak agent (Michael, 15 Nov 2026: "fighting
/// should only occur with enough energy otherwise run hide and get health
/// and food bars up"): run, and pass once no zombie is within threat range.
pub const ESCAPE_ZOMBIE_GOAL: &str = "escape_zombie";

/// The goal the teacher knows a recipe and a test for (01 §4.11, #58): a
/// shelter that passes the host's shelter test.
pub const SHELTER_GOAL: &str = "shelter";

/// Hostiles that shoot from range: the skeleton and its 1.21 variants
/// (stray, bogged). Both worlds count one that can shoot as a threat from
/// further out than a melee mob (sim `world::ranged`).
pub const RANGED_HOSTILES: [&str; 3] = ["entity:skeleton", "entity:stray", "entity:bogged"];

/// The novelty unit for the 8×8 region containing `pos` (spec §5, §21).
pub fn region_of(pos: crate::Pos) -> crate::ThingId {
    crate::ThingId(format!(
        "region:{},{}",
        pos.x.div_euclid(REGION),
        pos.z.div_euclid(REGION)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_are_8_by_8_and_floor_negative_coordinates() {
        let r = |x, z| region_of(crate::Pos { x, y: 0, z }).0;
        assert_eq!(r(0, 0), "region:0,0");
        assert_eq!(r(7, 15), "region:0,1");
        assert_eq!(r(8, 16), "region:1,2");
        assert_eq!(r(-1, -8), "region:-1,-1");
        assert_eq!(r(-9, 0), "region:-2,0");
    }

    #[test]
    fn task_type_indices_are_dense() {
        for (i, t) in TaskType::ALL.iter().enumerate() {
            assert_eq!(t.index(), i);
        }
    }
}

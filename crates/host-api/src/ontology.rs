//! Shared ontology (spec §2). Both worlds use Minecraft's names, so memories can
//! move between worlds (expansion item X17).

use serde::{Deserialize, Serialize};

/// External skills a host can execute (spec §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    Explore,
    MoveTo,
    Mine,
    Chop,
    Craft,
    Place,
    Attack,
    Flee,
    Eat,
    Farm,
    Shelter,
    Rest,
    FollowPlayer,
    Chat,
}

/// The 7 tenacity task types (#36). Finer splits only if logs show a type
/// behaves differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Mining,
    Building,
    Combat,
    Crafting,
    Farming,
    Travel,
    Helping,
}

impl TaskType {
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

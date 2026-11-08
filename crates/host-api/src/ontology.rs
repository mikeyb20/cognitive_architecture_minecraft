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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_type_indices_are_dense() {
        for (i, t) in TaskType::ALL.iter().enumerate() {
            assert_eq!(t.index(), i);
        }
    }
}

//! Game rules in one place (spec §21). Every number here is a proposal for
//! the sim pilot; `tests/rules.rs` pins them so a change is deliberate.

/// All tunable sim rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Map side in tiles (spec §21: 96).
    pub map_size: i32,
    /// Ticks per day (spec §21: 2,400). Night is phase 0.5–1.0.
    pub day_ticks: u64,
    /// Food lost per tick. Spec §21 proposed 1/600; the sim uses 1/2400 (one
    /// full bar per day) because 1/600 needs ~13 meals a day. Flagged in PR #3.
    pub food_decay: f32,
    /// Health lost per tick while food is 0.
    pub starve_damage: f32,
    /// Health regained per tick while food > `regen_food_min`.
    pub regen: f32,
    pub regen_food_min: f32,
    /// How far the agent sees, in tiles.
    pub view_radius: i32,
    /// Ticks per tile when walking and when sprinting (see `sprint_effort`).
    pub walk_ticks: u32,
    pub sprint_ticks: u32,
    /// Most tiles one move action covers.
    pub max_move: i32,
    /// Hostile spawn chance per tick at night (outside) and in caves.
    pub night_spawn: f32,
    pub cave_spawn: f32,
    /// Spawn ring around the agent, in tiles.
    pub spawn_min: i32,
    pub spawn_max: i32,
    /// Most hostiles alive at once.
    pub max_hostiles: usize,
    /// Hostiles move one tile every this many ticks, toward the agent within
    /// `chase_radius`.
    pub mob_move_ticks: u64,
    pub chase_radius: i32,
    /// Damage per hostile hit and ticks between hits.
    pub mob_damage: f32,
    pub mob_hit_ticks: u64,
    /// Food restored by eating.
    pub meat_food: f32,
    pub bread_food: f32,
    /// Ticks to build a shelter with and without 4 blocks in hand.
    pub shelter_ticks_blocks: u32,
    pub shelter_ticks_dig: u32,
    /// Base skill success before focus and threat.
    pub base_success: f32,
    /// Chops before a tree is used up.
    pub tree_chops: u8,
    /// Animals per 100 plains tiles at world creation.
    pub animal_density: f32,
    /// Below this effort the agent does not move or act: g(0) = 0 means no
    /// self-generated movement (01 §1.4).
    pub min_effort: f32,
    /// Effort at or above which the agent sprints.
    pub sprint_effort: f32,
    /// Ticks for one chop, harvest, craft, eat and attack swing.
    pub chop_ticks: u32,
    pub farm_ticks: u32,
    pub craft_ticks: u32,
    pub eat_ticks: u32,
    pub swing_ticks: u32,
    /// Ticks to mine with a wood, stone and iron pickaxe.
    pub mine_ticks: [u32; 3],
    /// Damage per swing, and most swings in one attack action.
    pub swing_damage: f32,
    pub max_swings: u32,
    /// Longest rest in one action, in ticks.
    pub rest_ticks: u32,
    /// Success multiplier when a hostile is within 2 tiles and the agent is
    /// not sheltered.
    pub threat_penalty: f32,
    /// Ticks spent when an action is blocked before it starts.
    pub blocked_ticks: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            map_size: 96,
            day_ticks: 2400,
            food_decay: 1.0 / 2400.0,
            starve_damage: 1.0 / 300.0,
            regen: 1.0 / 600.0,
            regen_food_min: 0.8,
            view_radius: 8,
            walk_ticks: 4,
            sprint_ticks: 2,
            max_move: 8,
            night_spawn: 0.02,
            cave_spawn: 0.005,
            spawn_min: 6,
            spawn_max: 12,
            max_hostiles: 6,
            mob_move_ticks: 6,
            chase_radius: 16,
            mob_damage: 0.1,
            mob_hit_ticks: 20,
            meat_food: 0.3,
            bread_food: 0.5,
            shelter_ticks_blocks: 20,
            shelter_ticks_dig: 60,
            base_success: 0.95,
            tree_chops: 3,
            animal_density: 1.0,
            min_effort: 0.05,
            sprint_effort: 0.6,
            chop_ticks: 30,
            farm_ticks: 20,
            craft_ticks: 10,
            eat_ticks: 30,
            swing_ticks: 10,
            mine_ticks: [40, 30, 20],
            swing_damage: 0.34,
            max_swings: 4,
            rest_ticks: 300,
            threat_penalty: 0.75,
            blocked_ticks: 5,
        }
    }
}

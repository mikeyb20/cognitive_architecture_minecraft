//! The runner ↔ bot link (spec §2, §22): the runner owns every decision and
//! drives; the TypeScript bot is the body and answers. One JSON object per
//! line each way, one reply per request. The bot also keeps the survival
//! interrupt (#42), which can cut an action short.

use serde::{Deserialize, Serialize};

use crate::reflex::ReflexThresholds;
use crate::{Candidate, Intensity, Observation, Outcome};

/// How the bot's skills behave (proposed until the pilot).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SkillConfig {
    /// How far to look for a searched block.
    pub search_radius: u32,
    /// Height to dig down to when searching for ore (iron is common near 16).
    pub ore_depth: i32,
    /// Stay this close when following a player.
    pub follow_range: u32,
    /// How long one follow or come action lasts, in ms.
    pub follow_ms: u32,
    /// How long one rest without a bed lasts, in ms.
    pub rest_ms: u32,
    /// How far one search step walks when nothing is in range.
    pub search_step: u32,
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            search_radius: 64,
            ore_depth: 16,
            follow_range: 3,
            follow_ms: 5000,
            rest_ms: 5000,
            search_step: 24,
        }
    }
}

/// How much of the world the bot reports (spec §5; proposed until the pilot).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Limits {
    /// Most blocks in one observation.
    pub max_visible_blocks: u32,
    /// Nearest blocks kept per name before the overall cap.
    pub max_per_kind: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_visible_blocks: 32,
            max_per_kind: 4,
        }
    }
}

/// The survival interrupt's thresholds, as the bot reads them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Thresholds {
    /// Health (0..1) at or below which a hostile nearby is an emergency.
    pub critical_health: f32,
    /// Food (0..1) at or below which eating is an emergency.
    pub starving_food: f32,
}

impl From<ReflexThresholds> for Thresholds {
    fn from(t: ReflexThresholds) -> Self {
        Self {
            critical_health: t.critical_health,
            starving_food: t.starving_food,
        }
    }
}

/// Everything the bot needs from the runner, sent once at connect, so each
/// value has one home (the Rust defaults).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BotConfig {
    /// How the bot's skills search and act.
    pub skills: SkillConfig,
    /// Time limits on skills.
    pub limits: Limits,
    /// When the survival interrupt fires.
    pub thresholds: Thresholds,
    /// Danger interrupt (sim #68 in Minecraft): a hostile this close, in
    /// blocks, to the unsheltered bot cuts short a skill begun with none
    /// that close. 0 turns it off (the default here); the Minecraft host
    /// sends its candidates' threat range.
    #[serde(rename = "dangerRange", default)]
    pub danger_range: f32,
    /// Fight gate (#87 in Minecraft; Michael, 15 Nov 2026: fight only
    /// with enough health): an attack begun at or above this health (0..1)
    /// stops once the bot's health falls below it, so the next decision
    /// sees the weak bot. The sim checks the gate before each swing; a bot
    /// attack lasts until the mob dies. 0 turns it off (the default here);
    /// the Minecraft host sends the sim's `fight_health`.
    #[serde(rename = "fightHealth", default)]
    pub fight_health: f32,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            skills: SkillConfig::default(),
            limits: Limits::default(),
            thresholds: ReflexThresholds::default().into(),
            danger_range: 0.0,
            fight_health: 0.0,
        }
    }
}

/// For a craft: what the tech tree says it needs (spec §21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RecipeHint {
    /// Station to craft at (`crafting_table`, `furnace`), if any.
    pub station: Option<String>,
    /// Items used up, with counts.
    pub inputs: Vec<(String, u32)>,
}

impl RecipeHint {
    /// The hint for crafting `item`, if the tech tree has it.
    pub fn for_item(item: &str) -> Option<Self> {
        let r = crate::craft::recipe(item)?;
        Some(Self {
            station: r.needs.map(str::to_string),
            inputs: r.inputs.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        })
    }
}

/// Runner → bot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum LinkRequest {
    /// First message: settings for this run.
    Hello {
        /// The bot's settings.
        config: BotConfig,
    },
    /// Read the world now.
    Perceive,
    /// Carry out one action; the reply comes when it ends.
    Execute {
        /// The chosen candidate.
        action: Candidate,
        /// How hard to carry it out.
        intensity: Intensity,
        /// For a craft, what it needs.
        recipe: Option<RecipeHint>,
    },
    /// Say something in game chat (replies to player commands).
    Say {
        /// The chat line.
        text: String,
    },
}

/// Bot → runner, one per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum LinkReply {
    /// Connected and configured. `names` is every block and item name the
    /// server's version knows, so chat commands can be checked.
    Ready {
        /// Block and item names.
        names: Vec<String>,
    },
    /// Answer to `Perceive`.
    Observed {
        /// What the bot sees now.
        obs: Observation,
    },
    /// Answer to `Execute`, when the action ends.
    Done {
        /// What the action did.
        outcome: Outcome,
    },
    /// Answer to `Say`: the line was sent.
    Said,
    /// The request could not be served (bad JSON, not connected, ...).
    Failed {
        /// Why.
        message: String,
    },
}

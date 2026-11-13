//! Player commands (spec §22, kept from v0.2 Step 18 and R8): read a chat
//! line into a [`Command`], turn the names players use into game names, and
//! mark the candidates that serve the open command so they get the
//! external-drive floor (spec §16, #41). The queue that holds commands is in
//! [`queue`]. Both worlds use this, so the sim's scripted player and a real
//! player in Minecraft are handled the same way.

pub mod queue;

use crate::craft::same_block;
use crate::planner::{in_ontology, need};
use crate::{Candidate, Inventory, Observation, Pos, Target};

pub use queue::{CommandQueue, QueueParams, Task};

/// What a player asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Hold `count` more of `item` (mine, chop, craft, ...). `None` means no
    /// number was given: "more" repeats the last count for that item.
    Get { item: String, count: Option<u32> },
    /// Follow the speaker until told to stay.
    Follow,
    /// Stop following.
    Stay,
    /// Walk to the speaker once.
    Come,
    /// Report the current task and the queue.
    Status,
    /// Drop the current task and start the next one.
    Cancel,
    /// Drop everything.
    Stop,
}

/// How one chat line reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Command(Command),
    /// Looked like a command but could not be read; the text says why.
    Unknown(String),
    /// Ordinary chat.
    Chat,
}

/// Verbs that ask for an item (v0.2 Step 18).
const GET_VERBS: [&str; 10] = [
    "mine", "get", "collect", "gather", "chop", "dig", "fetch", "bring", "make", "craft",
];

/// Words that mean "one" or "unspecified" before a name.
const FILLER: [&str; 5] = ["a", "an", "some", "the", "more"];

/// Names players use that differ from game names (v0.2 R8). Extend this
/// when the logs show a new miss.
const ALIASES: [(&str, &str); 21] = [
    ("wood", "oak_log"),
    ("log", "oak_log"),
    ("oak wood", "oak_log"),
    ("stone", "cobblestone"),
    ("cobble", "cobblestone"),
    ("iron", "iron_ore"),
    ("gold", "gold_ore"),
    ("plank", "oak_planks"),
    ("wood plank", "oak_planks"),
    ("workbench", "crafting_table"),
    ("table", "crafting_table"),
    ("iron bar", "iron_ingot"),
    ("pickaxe", "stone_pickaxe"),
    ("pick", "stone_pickaxe"),
    ("wood pickaxe", "wooden_pickaxe"),
    ("wooden pick", "wooden_pickaxe"),
    ("stone pick", "stone_pickaxe"),
    ("iron pick", "iron_pickaxe"),
    ("axe", "stone_axe"),
    ("sword", "stone_sword"),
    ("steak", "cooked_beef"),
];

fn alias(phrase: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(a, _)| *a == phrase).map(|(_, n)| *n)
}

/// The phrase with its last word made singular ("oak logs" → "oak log").
fn singular(phrase: &str) -> Option<String> {
    let (head, last) = phrase.rsplit_once(' ').unwrap_or(("", phrase));
    let one = last
        .strip_suffix("es")
        .filter(|s| s.ends_with("ch") || s.ends_with("sh"))
        .or_else(|| last.strip_suffix('s'))?;
    Some(if head.is_empty() {
        one.to_string()
    } else {
        format!("{head} {one}")
    })
}

/// The game name for what a player called something, if `known` accepts it
/// (v0.2 R8): lower-case, drop a leading article, try the alias table, then
/// underscores, then the singular of both.
pub fn normalize(input: &str, known: impl Fn(&str) -> bool) -> Option<String> {
    let words: Vec<String> = input
        .split_whitespace()
        .map(str::to_lowercase)
        .skip_while(|w| FILLER.contains(&w.as_str()))
        .collect();
    let phrase = words.join(" ").replace(['_', '-'], " ");
    let try_one = |p: &str| {
        let name = alias(p).map_or_else(|| p.replace(' ', "_"), str::to_string);
        known(&name).then_some(name)
    };
    try_one(&phrase).or_else(|| singular(&phrase).and_then(|s| try_one(&s)))
}

/// Read one chat line (v0.2 Step 18). An optional "bot," prefix is dropped;
/// "!word" is a control command; "<verb> [count] <name>" asks for an item.
pub fn parse(text: &str, known: impl Fn(&str) -> bool) -> Parsed {
    let t = text.trim();
    let lower = t.to_lowercase();
    let body = lower
        .strip_prefix("bot,")
        .or_else(|| lower.strip_prefix("bot "))
        .unwrap_or(&lower)
        .trim();
    if let Some(word) = body.strip_prefix('!') {
        return match word.trim() {
            "status" => Parsed::Command(Command::Status),
            "cancel" => Parsed::Command(Command::Cancel),
            "stop" => Parsed::Command(Command::Stop),
            "follow" => Parsed::Command(Command::Follow),
            "stay" => Parsed::Command(Command::Stay),
            "come" => Parsed::Command(Command::Come),
            other => Parsed::Unknown(format!("I don't know the command !{other}.")),
        };
    }
    let mut words = body.split_whitespace();
    let Some(verb) = words.next().filter(|v| GET_VERBS.contains(v)) else {
        return Parsed::Chat;
    };
    let rest: Vec<&str> = words.collect();
    let (count, name) = match rest.first().and_then(|w| w.parse::<u32>().ok()) {
        Some(n) => (Some(n), &rest[1..]),
        None if rest.first() == Some(&"a") || rest.first() == Some(&"an") => (Some(1), &rest[..]),
        None => (None, &rest[..]),
    };
    if count == Some(0) {
        return Parsed::Unknown("Zero of something is already done.".into());
    }
    let name = name.join(" ");
    match normalize(&name, known) {
        Some(item) => Parsed::Command(Command::Get { item, count }),
        None if name.is_empty() => Parsed::Unknown(format!("{verb} what?")),
        None => Parsed::Unknown(format!("I don't know what \"{name}\" is.")),
    }
}

/// Where things are known to be (spec §22): what is seen now and, for a
/// host that remembers, where things were seen before.
pub trait Whereabouts {
    /// Whether `thing` (a memory key such as "block:oak_log") is at `p`.
    fn is_at(&self, p: Pos, thing: &str) -> bool;
}

/// What is in view.
impl Whereabouts for Observation {
    fn is_at(&self, p: Pos, thing: &str) -> bool {
        self.visible
            .iter()
            .any(|s| s.pos == p && same_block(&s.thing.0, thing))
    }
}

/// Whether candidate `c` acts on `thing` (a memory key such as
/// "block:oak_log" or "item:stick"): an item target by name, a position
/// where that thing is known to be, or a search for it.
pub fn serves(known: &impl Whereabouts, c: &Candidate, thing: &str) -> bool {
    match &c.target {
        Some(Target::Item(i)) => thing.strip_prefix("item:") == Some(i.as_str()),
        Some(Target::Pos(p)) => known.is_at(*p, thing),
        Some(Target::Thing(t)) => same_block(&t.0, thing),
        Some(Target::Player(_)) | None => false,
    }
}

/// Mark the candidates that do the planner's next step toward holding `qty`
/// of `goal` with inventory `inv` (spec §16, #41). Marks nothing when the
/// goal is held or has no route.
pub fn mark_step(
    inv: &Inventory,
    known: &impl Whereabouts,
    cands: &mut [Candidate],
    goal: &str,
    qty: u32,
) {
    let Some(step) = need(inv, goal, qty) else {
        return;
    };
    let (skill, thing, _) = in_ontology(step);
    for c in cands.iter_mut() {
        if c.skill == skill && serves(known, c, &thing) {
            c.commanded = true;
        }
    }
}

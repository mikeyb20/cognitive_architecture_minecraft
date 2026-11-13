//! How the agent's state reads to an LLM (spec §23): a short status, then
//! the caller's lines, kept under the input cap by dropping the least
//! useful status lines first (far things, then small inventory stacks).
//! The caller's lines are never cut. Shared by arm B and the LLM tools.

use host_api::{Observation, Pos};

/// Most visible things listed, nearest first.
const MAX_SEEN: usize = 8;
/// Most recent chat lines listed.
const MAX_CHAT: usize = 2;

/// A whole number of percent.
fn pct(x: f32) -> i32 {
    (x * 100.0).round() as i32
}

/// A position as `(x, y, z)`.
pub fn at(p: Pos) -> String {
    format!("({}, {}, {})", p.x, p.y, p.z)
}

/// Body, time and place, then the `items` largest stacks, the `seen`
/// nearest things and the latest chat.
pub fn status(obs: &Observation, items: usize, seen: usize) -> Vec<String> {
    let time = if obs.is_night() { "night" } else { "day" };
    let shelter = if obs.sheltered { ", sheltered" } else { "" };
    let mut lines = vec![format!(
        "health {}%, food {}%, {time}{shelter}, at {}",
        pct(obs.health),
        pct(obs.food),
        at(obs.position)
    )];
    let mut inv = obs.inventory.items.clone();
    inv.sort_by_key(|(n, c)| (std::cmp::Reverse(*c), n.clone()));
    let inv: Vec<String> = inv
        .iter()
        .take(items)
        .map(|(n, c)| format!("{n} x{c}"))
        .collect();
    let inv = if inv.is_empty() {
        "empty".into()
    } else {
        inv.join(", ")
    };
    lines.push(format!("inventory: {inv}"));
    let mut near: Vec<_> = obs.visible.iter().collect();
    near.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    for s in near.into_iter().take(seen) {
        lines.push(format!("see {} {:.0} away", s.thing.0, s.distance));
    }
    for m in obs.chat.iter().rev().take(MAX_CHAT) {
        lines.push(format!("{} says: {}", m.from, m.text));
    }
    lines
}

/// The status followed by `tail`, shrunk until it fits `max_tokens` with a
/// system part of `system_chars` (at four characters a token, as
/// [`crate::Prompt::estimated_tokens`]). If even the bare form is too long
/// it is returned anyway.
pub fn fit(obs: &Observation, tail: &[String], system_chars: usize, max_tokens: u32) -> String {
    let budget = (max_tokens as usize * 4).saturating_sub(system_chars);
    let items = obs.inventory.items.len();
    let steps = (0..=MAX_SEEN)
        .rev()
        .map(|seen| (items, seen))
        .chain((0..items).rev().map(|i| (i, 0)));
    let mut text = String::new();
    for (items, seen) in steps {
        let mut all = status(obs, items, seen);
        all.extend(tail.iter().cloned());
        text = all.join("\n");
        if text.chars().count() <= budget {
            break;
        }
    }
    text
}

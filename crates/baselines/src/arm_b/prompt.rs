//! What arm B's LLM sees (spec §23, §24): a short status, then one line per
//! candidate. Kept under the input cap by dropping the least useful lines
//! first (far things, then small inventory stacks); the candidates are
//! never cut.

use host_api::{Candidate, Observation, Pos, Target};

/// Most visible things listed, nearest first.
const MAX_SEEN: usize = 8;

/// A whole number of percent, for the status line.
fn pct(x: f32) -> i32 {
    (x * 100.0).round() as i32
}

fn at(p: Pos) -> String {
    format!("({}, {}, {})", p.x, p.y, p.z)
}

/// What a candidate is aimed at, in words.
fn aim(obs: &Observation, t: Option<&Target>) -> String {
    match t {
        None => String::new(),
        Some(Target::Pos(p)) => {
            let what = obs.visible.iter().find(|s| s.pos == *p);
            match what {
                Some(s) => format!(" {} at {}", s.thing.0, at(*p)),
                None => format!(" to {}", at(*p)),
            }
        }
        Some(Target::Thing(k)) => format!(" (search for {})", k.0),
        Some(Target::Item(i)) => format!(" {i}"),
        Some(Target::Player(n)) => format!(" player {n}"),
    }
}

/// One line per candidate: `id: skill target [task] cost risk`, plus a
/// novelty bonus when B+N supplies one.
pub(crate) fn candidate_line(obs: &Observation, c: &Candidate, bonus: Option<f32>) -> String {
    let novelty = bonus.map_or(String::new(), |b| format!(" novelty {b:.2}"));
    let cmd = if c.commanded {
        " (asked by a player)"
    } else {
        ""
    };
    format!(
        "{}: {}{} [{:?}] cost {:.2} risk {:.2}{novelty}{cmd}",
        c.id,
        c.skill.name(),
        aim(obs, c.target.as_ref()),
        c.task_type,
        c.est_cost,
        c.est_risk,
    )
}

/// The status block: body, time, place, then inventory and what is in view.
fn status(obs: &Observation, items: usize, seen: usize) -> Vec<String> {
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
    lines.push(format!(
        "inventory: {}",
        if inv.is_empty() {
            "empty".into()
        } else {
            inv.join(", ")
        }
    ));
    let mut near: Vec<_> = obs.visible.iter().collect();
    near.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    for s in near.into_iter().take(seen) {
        lines.push(format!("see {} {:.0} away", s.thing.0, s.distance));
    }
    for m in obs.chat.iter().rev().take(2) {
        lines.push(format!("{} says: {}", m.from, m.text));
    }
    lines
}

/// The user part of the prompt, shrunk until it fits `max_tokens`
/// (estimated at four characters a token, as `llm::Prompt` does): fewer
/// things in view first, then fewer inventory stacks. If even the bare
/// list is too long it goes out anyway; the candidates are never cut.
pub(crate) fn user_prompt(
    obs: &Observation,
    lines: &[String],
    system_chars: usize,
    max_tokens: u32,
) -> String {
    let budget = (max_tokens as usize * 4).saturating_sub(system_chars);
    let items = obs.inventory.items.len();
    let steps = (0..=MAX_SEEN)
        .rev()
        .map(|seen| (items, seen))
        .chain((0..items).rev().map(|i| (i, 0)));
    let mut text = String::new();
    for (items, seen) in steps {
        let mut all = status(obs, items, seen);
        all.push("Options:".into());
        all.extend(lines.iter().cloned());
        text = all.join("\n");
        if text.chars().count() <= budget {
            break;
        }
    }
    text
}

/// The candidate id the reply names: the first whole number in it that is
/// one of the offered ids.
pub(crate) fn chosen_id(reply: &str, ids: &[u32]) -> Option<u32> {
    reply
        .split(|ch: char| !ch.is_ascii_digit())
        .filter_map(|w| w.parse().ok())
        .find(|n| ids.contains(n))
}

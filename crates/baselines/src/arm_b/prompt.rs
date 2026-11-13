//! What arm B's LLM sees (spec §23, §24): the shared status
//! ([`llm::describe`]), then one line per candidate, which are never cut.

use host_api::{Candidate, Observation, Target};
use llm::describe::{self, at};

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

/// The user part of the prompt: status, then the options (spec §23).
pub(crate) fn user_prompt(
    obs: &Observation,
    lines: &[String],
    system_chars: usize,
    max_tokens: u32,
) -> String {
    let mut tail = vec!["Options:".to_string()];
    tail.extend(lines.iter().cloned());
    describe::fit(obs, &tail, system_chars, max_tokens)
}

/// The candidate id the reply names: the first whole number in it that is
/// one of the offered ids.
pub(crate) fn chosen_id(reply: &str, ids: &[u32]) -> Option<u32> {
    reply
        .split(|ch: char| !ch.is_ascii_digit())
        .filter_map(|w| w.parse().ok())
        .find(|n| ids.contains(n))
}

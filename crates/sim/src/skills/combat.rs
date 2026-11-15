//! Fighting a hostile with spacing (Michael, 15 Nov 2026: "time the hits
//! from the zombie in order to hit it without taking damage"). The motor
//! skill, not a brain rule: hit only when the swing has recharged and the
//! hostile is in reach, let the knockback push it back, step back if it
//! gets next to you, and hit again as it comes in. Minecraft: an attack
//! does full damage only once the cooldown `T = 20 / attackSpeed` ticks has
//! recharged (minecraft.fandom.com/wiki/Damage), and a hit pushes the mob
//! back. A zombie hits only from the next tile, so with the reach and the
//! push the agent can keep it off; a missed swing lets it close.

use host_api::{Intensity, Pos};

use super::Exec;
use crate::geom::{cheb, dist};
use crate::mob::MobKind;
use crate::world::SimWorld;

/// Fight the hostile of `kind` at `start` (spec §21 attack, spaced). Each
/// turn of the loop does one thing: hit (when recharged and in reach),
/// step back (when it is next to the agent and the swing is not ready),
/// close in (when out of reach and recharged), or let a tick pass. Gives up after
/// `max_swings` swings or `2 × max_swings × swing_ticks` ticks.
pub(crate) fn fight(w: &mut SimWorld, kind: MobKind, start: Pos, it: Intensity) -> Exec {
    let end = w.tick + 2 * u64::from(w.rules.max_swings * w.rules.swing_ticks);
    let mut ready = w.tick;
    let mut swings = 0;
    let mut at = start;
    while swings < w.rules.max_swings && w.tick < end {
        let Some(i) = w.track(kind, at) else {
            return (false, 0.0, false);
        };
        at = w.mobs[i].pos;
        let me = w.body.pos;
        let go_on = if w.tick >= ready && dist(at, me) <= w.rules.attack_reach {
            ready = w.tick + u64::from(w.rules.swing_ticks);
            swings += 1;
            if w.roll(it.focus) {
                w.mobs[i].health -= w.rules.swing_damage;
                if w.mobs[i].health <= 0.0 {
                    w.mobs.remove(i);
                    return (true, 1.0, false);
                }
                w.knock_back(i);
            }
            true
        } else if let Some(p) = next_step(w, at, me, w.tick >= ready) {
            w.step_once(p, it.effort)
        } else {
            w.wait(1)
        };
        if !go_on {
            return (false, 0.0, false);
        }
    }
    (false, 0.5, false)
}

/// Where to step: back, if the hostile is on the next tile; toward it, if
/// it is out of reach and the swing is `ready`; nowhere otherwise (wait for
/// the swing, or for the hostile to come back in after a push).
fn next_step(w: &SimWorld, at: Pos, me: Pos, ready: bool) -> Option<Pos> {
    if cheb(at, me) <= 1 {
        w.back_off(at)
    } else if ready && dist(at, me) > w.rules.attack_reach {
        w.path_to(at, true).and_then(|p| p.first().copied())
    } else {
        None
    }
}

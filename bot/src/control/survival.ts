// The bot's half of the emergency reflexes (spec §20, #42). Deciding what
// to do stays with the shared Rust reflex at the next decision; the bot's
// job is only to stop a long skill the moment an emergency starts, so that
// decision comes at once instead of after the skill ends (v0.2 R7).

import type { Thresholds } from "../host.js";
import type { DangerState } from "./danger.js";
import { type ActionLock, Priority } from "./lock.js";

export type { Thresholds };

export interface BodyState extends DangerState {
  /** 0..1 */
  health: number;
  /** 0..1 */
  food: number;
  hostileNear: boolean;
  hasFood: boolean;
}

/** The same conditions as `host_api::reflex::emergency_reflex`. */
export function emergency(s: BodyState, t: Thresholds): boolean {
  const flee = s.health <= t.criticalHealth && !s.sheltered && s.hostileNear;
  const eat = s.food <= t.starvingFood && s.hasFood;
  return flee || eat;
}

/**
 * Stop whatever skill holds the body, if there is an emergency. Returns
 * whether it interrupted. Takes the lock at the top priority (which aborts
 * the running skill) and gives it straight back.
 */
export function interruptIfEmergency(lock: ActionLock, s: BodyState, t: Thresholds): boolean {
  if (!emergency(s, t) || lock.holder() === null) {
    return false;
  }
  lock.acquire("survival", Priority.Critical);
  lock.release("survival");
  return true;
}

/**
 * Emergencies cut short only a skill begun outside one. The reflex's own
 * flee (or eat) starts in the emergency it answers; aborting it the next
 * tick left the bot fleeing for 0 ticks again and again while a zombie
 * killed it (live runs, 15 Nov 2026). With no body seen yet, a skill
 * counts as begun calm.
 */
export class EmergencyWatch {
  #calm = true;

  /** Call as a skill starts. */
  begin(s: BodyState | null, t: Thresholds): void {
    this.#calm = s === null || !emergency(s, t);
  }

  /** `interruptIfEmergency`, for a skill begun calm; once per skill. */
  check(lock: ActionLock, s: BodyState, t: Thresholds): boolean {
    if (!this.#calm || !interruptIfEmergency(lock, s, t)) {
      return false;
    }
    this.#calm = false;
    return true;
  }
}

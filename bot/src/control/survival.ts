// The bot's half of the emergency reflexes (spec §20, #42). Deciding what
// to do stays with the shared Rust reflex at the next decision; the bot's
// job is only to stop a long skill the moment an emergency starts, so that
// decision comes at once instead of after the skill ends (v0.2 R7).

import { type ActionLock, Priority } from "./lock.js";

/** The runner sends these at connect (host-api `ReflexThresholds`), 0..1. */
export interface Thresholds {
  criticalHealth: number;
  starvingFood: number;
}

export interface BodyState {
  /** 0..1 */
  health: number;
  /** 0..1 */
  food: number;
  sheltered: boolean;
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

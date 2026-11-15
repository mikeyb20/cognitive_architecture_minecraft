// The danger interrupt (sim #68, in Minecraft): a skill begun with no
// hostile near stops the moment one comes within range of the unsheltered
// bot, so the runner can choose again (flee, dig in) instead of the bot
// taking free hits mid-chop. A skill begun in danger runs on: it was chosen
// with the hostile in view.

import { type ActionLock, Priority } from "./lock.js";

export interface DangerState {
  sheltered: boolean;
  /** Blocks to the nearest hostile in view, or null if none. */
  nearestHostile: number | null;
}

/** Who takes the lock for an instant to stop the running skill. */
export const DANGER = "danger";

/** A hostile within `range` blocks of the unsheltered bot. `range` 0 is off. */
export function inDanger(s: DangerState, range: number): boolean {
  return range > 0 && !s.sheltered && s.nearestHostile !== null && s.nearestHostile <= range;
}

export class DangerWatch {
  #calm = false;

  /** Call as a skill starts: only a skill begun calm can be cut short. */
  begin(s: DangerState | null, range: number): void {
    this.#calm = s !== null && !inDanger(s, range);
  }

  /**
   * Stop the running skill if danger arrived since it began calm. Fires once
   * per skill. Returns whether it interrupted.
   */
  check(lock: ActionLock, s: DangerState, range: number): boolean {
    if (!this.#calm || lock.holder() === null || !inDanger(s, range)) {
      return false;
    }
    this.#calm = false;
    lock.acquire(DANGER, Priority.Critical);
    lock.release(DANGER);
    return true;
  }
}

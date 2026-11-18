// The fight gate in Minecraft (#87; Michael, 15 Nov 2026: "fighting should
// only occur with enough energy otherwise run hide and get health and food
// bars up"). The sim checks the gate before each swing; a bot attack runs
// until the mob dies, and live bots went from 0.6 to 0.15 health inside one
// attack. So an attack begun at or above the gate stops once health falls
// below it, and the next decision sees the weak bot (escape_zombie).

import type { ActionLock } from "./lock.js";
import { Onset } from "./onset.js";

/** Who takes the lock for an instant to stop the fight. */
export const FIGHT = "fight-gate";

/** Below the gate. A gate of 0 is off. */
export function weak(health: number, gate: number): boolean {
  return health < gate;
}

export class FightWatch {
  #onset = new Onset(false);

  /**
   * Call as a skill starts: only an attack begun strong can be stopped.
   * Pass health 0 when the body has not been seen yet.
   */
  begin(attack: boolean, health: number, gate: number): void {
    this.#onset.begin(!attack || weak(health, gate));
  }

  /** Stop the attack once health falls below the gate. */
  check(lock: ActionLock, health: number, gate: number): boolean {
    return this.#onset.fire(lock, weak(health, gate), FIGHT);
  }
}

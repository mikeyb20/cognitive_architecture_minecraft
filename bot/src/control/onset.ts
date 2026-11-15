// The shape every interrupt shares (danger sim #68, emergency spec §20,
// fight gate #87): a condition that held when the skill began was in view
// when it was chosen, so the skill runs on; one that starts mid-skill stops
// it, once, so the runner can choose again.

import { type ActionLock, Priority } from "./lock.js";

export class Onset {
  #armed: boolean;

  /** `armed`: whether a condition seen before any `begin` can interrupt. */
  constructor(armed: boolean) {
    this.#armed = armed;
  }

  /** Call as a skill starts, with whether the condition holds already. */
  begin(holds: boolean): void {
    this.#armed = !holds;
  }

  /**
   * If the condition `holds` now, began after the skill and a skill runs,
   * stop it: `who` takes the lock at the top priority for an instant.
   * Fires once per skill. Returns whether it interrupted.
   */
  fire(lock: ActionLock, holds: boolean, who: string): boolean {
    if (!this.#armed || !holds || lock.holder() === null) {
      return false;
    }
    this.#armed = false;
    lock.acquire(who, Priority.Critical);
    lock.release(who);
    return true;
  }
}

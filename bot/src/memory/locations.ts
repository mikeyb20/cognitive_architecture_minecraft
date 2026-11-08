// Where things were seen (v0.2 R4.9; Step 20 test 4: "mine more iron" goes
// straight to the stored spot). The host keeps this, not the brain: it is how
// the body finds its way back, and every arm gets the same map.

import { distance } from "../geom.js";
import type { Pos } from "../host.js";

/** Most spots kept per thing; the oldest is dropped first. */
export const MAX_PER_THING = 32;

function key(p: Pos): string {
  return `${p.x},${p.y},${p.z}`;
}

export class LocationMemory {
  #spots = new Map<string, Map<string, Pos>>();

  constructor(readonly maxPerThing: number = MAX_PER_THING) {}

  /** Remember that `thing` (a memory key such as "block:iron_ore") is at `p`. */
  remember(thing: string, p: Pos): void {
    let spots = this.#spots.get(thing);
    if (spots === undefined) {
      spots = new Map();
      this.#spots.set(thing, spots);
    }
    spots.delete(key(p));
    spots.set(key(p), p);
    if (spots.size > this.maxPerThing) {
      const oldest = spots.keys().next().value as string;
      spots.delete(oldest);
    }
  }

  /** The thing is gone from `p` (mined, or not there on arrival). */
  forget(thing: string, p: Pos): void {
    this.#spots.get(thing)?.delete(key(p));
  }

  /** The nearest stored spot of `thing` to `from`; ties go to the newer. */
  nearest(thing: string, from: Pos): Pos | null {
    const spots = this.#spots.get(thing);
    if (spots === undefined) {
      return null;
    }
    let best: Pos | null = null;
    let bestD = Infinity;
    for (const p of spots.values()) {
      const d = distance(p, from);
      if (d <= bestD) {
        best = p;
        bestD = d;
      }
    }
    return best;
  }

  /** Things with at least one stored spot, sorted. */
  things(): string[] {
    return [...this.#spots.entries()]
      .filter(([, s]) => s.size > 0)
      .map(([t]) => t)
      .sort();
  }

  count(thing: string): number {
    return this.#spots.get(thing)?.size ?? 0;
  }
}

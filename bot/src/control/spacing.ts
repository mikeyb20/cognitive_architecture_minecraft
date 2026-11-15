// Spaced melee (Michael, 15 Nov 2026: "time the hits from the zombie in
// order to hit it without taking damage"; the sim's `skills::combat`, #80).
// A motor skill, not a choice: hit only when the swing has recharged and the
// mob is within reach, so every hit does full damage and knocks it back;
// step back if it gets too close while the swing recharges; close in when
// recharged and out of reach; otherwise hold. Minecraft: an attack does full
// damage only once the cooldown, 20 / attackSpeed ticks, has passed
// (minecraft.fandom.com/wiki/Damage).

/** What the body does this tick. */
export type Move = "hit" | "back" | "close" | "hold";

export interface Spacing {
  /** Blocks the bot can hit from (survival reach 3, not verified this session). */
  reach: number;
  /** Nearer than this (blocks) the mob can hit back (proposed). */
  tooClose: number;
}

export const SPACING: Spacing = { reach: 3, tooClose: 2 };

/** The move for a mob `distance` blocks away, with the swing `ready` or not. */
export function nextMove(
  distance: number,
  ready: boolean,
  s: Spacing = SPACING,
): Move {
  if (ready) {
    return distance <= s.reach ? "hit" : "close";
  }
  return distance < s.tooClose ? "back" : "hold";
}

/** Counts game ticks since the last swing against the held weapon's cooldown. */
export class Swing {
  #since: number;

  /** Starts recharged: the first hit can land at once. */
  constructor(readonly cooldown: number) {
    this.#since = cooldown;
  }

  /** One game tick passed. */
  tick(): void {
    this.#since += 1;
  }

  ready(): boolean {
    return this.#since >= this.cooldown;
  }

  /** The bot swung. */
  swung(): void {
    this.#since = 0;
  }
}

/**
 * Entity types the bot may attack: living mobs only. Dropped items, arrows
 * and XP orbs are entities too, and the server kicks a client that attacks
 * one ("Attempting to attack an invalid entity", live Peaceful run 15 Nov
 * 2026).
 */
const ATTACKABLE: readonly string[] = ["hostile", "mob", "animal", "passive"];

/** Whether an entity of Mineflayer type `type` can be attacked. */
export function attackable(type: string): boolean {
  return ATTACKABLE.includes(type);
}

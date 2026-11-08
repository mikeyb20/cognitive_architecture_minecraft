// What a skill may ask of the body (spec §22). The Mineflayer glue in
// src/mineflayer/ implements this; tests use a fake. Every long call takes
// the action lock's signal and gives up promptly when it aborts. Calls
// return whether they did what was asked; they never throw for an ordinary
// failure such as "no path".

import type { Pos } from "../host.js";

export interface Body {
  /** World age in game ticks. */
  age(): number;
  position(): Pos;
  /** Item counts, stacks merged. */
  inventory(): ReadonlyMap<string, number>;
  /** Health points, 0..20. */
  health(): number;

  /** Walk to within `range` blocks of `to`. */
  goto(to: Pos, range: number, signal: AbortSignal): Promise<boolean>;
  /** Walk, digging as needed, until standing at height `y`. */
  gotoY(y: number, signal: AbortSignal): Promise<boolean>;
  /** Stay within `range` of the player for `ms`. False if the player is gone. */
  follow(player: string, range: number, ms: number, signal: AbortSignal): Promise<boolean>;

  /** The nearest block with one of these names within `radius`, if any. */
  findBlock(names: readonly string[], radius: number): Pos | null;
  /** Mine the block at `at` (walking there) and pick up what drops. */
  dig(at: Pos, signal: AbortSignal): Promise<boolean>;
  /** Place one `item` against the block at `at`, standing where it is. */
  place(item: string, at: Pos, signal: AbortSignal): Promise<boolean>;

  /** Fight the mob nearest `at` until it dies, flees out of reach, or the signal aborts. */
  attack(at: Pos, signal: AbortSignal): Promise<boolean>;
  eat(item: string, signal: AbortSignal): Promise<boolean>;
  /** Craft one batch of `item`, using the crafting table at `table` if given. */
  craft(item: string, table: Pos | null, signal: AbortSignal): Promise<boolean>;
  /** Smelt one `input` in the furnace at `furnace`, waiting for the result. */
  smelt(input: string, furnace: Pos, signal: AbortSignal): Promise<boolean>;
  sleep(bed: Pos, signal: AbortSignal): Promise<boolean>;
  /** Stand still for `ms`. */
  wait(ms: number, signal: AbortSignal): Promise<void>;
}

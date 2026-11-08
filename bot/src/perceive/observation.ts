// Mineflayer state → the shared Observation (spec §2, §5). The Mineflayer
// glue copies what it reads into a plain Snapshot; this file does the
// conversion, so it can be tested without a server. Thing ids use the same
// forms as the sim: "block:<name>", "entity:<name>", "player:<name>".

import { type Vec3Like, distance as dist } from "../geom.js";
import type { ChatMsg, Observation, Pos, Seen } from "../host.js";

/** Blocks the bot reports (spec §5 things): ores, food, stations, danger. */
const NOTABLE = new Set([
  "stone",
  "iron_ore",
  "deepslate_iron_ore",
  "coal_ore",
  "deepslate_coal_ore",
  "gold_ore",
  "deepslate_gold_ore",
  "diamond_ore",
  "deepslate_diamond_ore",
  "wheat",
  "crafting_table",
  "furnace",
  "water",
  "lava",
]);

/** Whether the bot reports this block: the set above, any log, any bed. */
export function notable(name: string): boolean {
  return NOTABLE.has(name) || name.endsWith("_log") || name.endsWith("_bed");
}

/** Minecraft's own scales. */
export const DAY_TICKS = 24000;
export const MAX_HEALTH = 20;
export const MAX_FOOD = 20;

export type { Vec3Like };

export interface SeenBlock {
  name: string;
  pos: Vec3Like;
}

export interface SeenEntity {
  name: string;
  kind: "hostile" | "passive" | "player";
  pos: Vec3Like;
}

/** What the glue reads from the bot in one go. */
export interface Snapshot {
  /** World age in game ticks. */
  age: number;
  /** 0..24000; 0 is sunrise, 12000 sunset. */
  timeOfDay: number;
  /** 0..20 */
  health: number;
  /** 0..20 */
  food: number;
  position: Vec3Like;
  inventory: Array<{ name: string; count: number }>;
  blocks: SeenBlock[];
  entities: SeenEntity[];
  chat: ChatMsg[];
  /** Health points lost since the last snapshot. */
  damage: number;
  sleeping: boolean;
  sheltered: boolean;
}

/** How much the host shows the brain; set by the runner at connect. */
export interface Limits {
  maxVisibleBlocks: number;
  /**
   * Nearest blocks kept per name before the overall cap, so stone all
   * around underground cannot hide the one ore in view.
   */
  maxPerKind: number;
}

export function blockPos(v: Vec3Like): Pos {
  return { x: Math.floor(v.x), y: Math.floor(v.y), z: Math.floor(v.z) };
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/** Stacks of the same item merged, sorted by name. */
function inventory(stacks: Snapshot["inventory"]): Array<[string, number]> {
  const total = new Map<string, number>();
  for (const s of stacks) {
    total.set(s.name, (total.get(s.name) ?? 0) + s.count);
  }
  return [...total.keys()].sort().map((k) => [k, total.get(k) as number]);
}

export function toObservation(s: Snapshot, limits: Limits, agent = 0): Observation {
  const me = s.position;
  const perKind = new Map<string, number>();
  const blocks: Seen[] = s.blocks
    .map((b) => ({
      thing: `block:${b.name}`,
      kind: "Block" as const,
      pos: blockPos(b.pos),
      distance: dist(me, b.pos),
    }))
    .sort((a, b) => a.distance - b.distance)
    .filter((b) => {
      const n = (perKind.get(b.thing) ?? 0) + 1;
      perKind.set(b.thing, n);
      return n <= limits.maxPerKind;
    })
    .slice(0, limits.maxVisibleBlocks);
  const entities: Seen[] = s.entities.map((e) => ({
    thing: e.kind === "player" ? `player:${e.name}` : `entity:${e.name}`,
    kind: e.kind === "player" ? ("Player" as const) : { Entity: { hostile: e.kind === "hostile" } },
    pos: blockPos(e.pos),
    distance: dist(me, e.pos),
  }));
  return {
    agent,
    tick: s.age,
    day_phase: (((s.timeOfDay % DAY_TICKS) + DAY_TICKS) % DAY_TICKS) / DAY_TICKS,
    health: clamp01(s.health / MAX_HEALTH),
    food: clamp01(s.food / MAX_FOOD),
    position: blockPos(me),
    inventory: { items: inventory(s.inventory) },
    visible: [...entities, ...blocks].sort((a, b) => a.distance - b.distance),
    chat: s.chat,
    damage_taken: Math.max(0, s.damage) / MAX_HEALTH,
    resting: s.sleeping,
    sheltered: s.sheltered,
  };
}

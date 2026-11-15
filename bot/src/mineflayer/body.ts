// The Body interface on a live Mineflayer bot (spec §22). Glue only: each
// method calls the library and turns errors and timeouts into `false`.
// Checked by the live acceptance tests, not unit tests (ARCHITECTURE.md §1).

import type { EventEmitter } from "node:events";

import type { Bot } from "mineflayer";
// Type imports for the plugins' additions to `Bot` (collectBlock); the
// weapon cooldown table comes from mineflayer-pvp.
import type {} from "mineflayer-collectblock";
import pathfinderPkg from "mineflayer-pathfinder";
import { getCooldown } from "mineflayer-pvp/lib/Cooldown.js";
import type { Block } from "prismarine-block";
import type { Entity } from "prismarine-entity";
import { Vec3 } from "vec3";

import { type Move, SPACING, Swing, nextMove } from "../control/spacing.js";
import type { Pos } from "../host.js";
import type { Body } from "../skills/body.js";

const { goals } = pathfinderPkg;

/** Longest any single body call may take, in ms. */
export const CALL_LIMIT_MS = 60_000;

/** Crop age at which wheat, carrots and potatoes are grown (Minecraft 1.21). */
const RIPE_AGE = 7;
/** How near to stand to use an item on a block. */
const USE_RANGE = 3;
/** Ticks to wait after using an item for the server's update. */
const USE_SETTLE_TICKS = 4;

/** A mob further than this (blocks) has got away: the attack gives up. */
const GIVE_UP_RANGE = 16;

const v = (p: Pos) => new Vec3(p.x, p.y, p.z);

/** Where the pathfinder heads for each spaced-melee move. */
function goalFor(move: Move, mob: Entity): InstanceType<typeof goals.Goal> | null {
  switch (move) {
    case "back":
      return new goals.GoalInvert(new goals.GoalFollow(mob, SPACING.tooClose));
    case "close":
      return new goals.GoalFollow(mob, SPACING.reach - 0.5);
    default:
      return null;
  }
}
const posOf = (p: Vec3): Pos => ({ x: p.x, y: p.y, z: p.z });

/** Run `work`, giving up (and calling `stop`) on abort or after `ms`. */
async function bounded<T>(
  signal: AbortSignal,
  ms: number,
  work: () => Promise<T>,
  stop: () => void,
  fallback: T,
): Promise<T> {
  if (signal.aborted) {
    return fallback;
  }
  let timer: NodeJS.Timeout | undefined;
  const cut = new Promise<T>((resolve) => {
    const end = () => {
      stop();
      resolve(fallback);
    };
    timer = setTimeout(end, ms);
    signal.addEventListener("abort", end, { once: true });
  });
  try {
    return await Promise.race([work().catch(() => fallback), cut]);
  } finally {
    clearTimeout(timer);
  }
}

export class MineflayerBody implements Body {
  constructor(private readonly bot: Bot) {}

  age(): number {
    return Number(this.bot.time.age);
  }

  position(): Pos {
    const p = this.bot.entity.position;
    return { x: Math.floor(p.x), y: Math.floor(p.y), z: Math.floor(p.z) };
  }

  inventory(): ReadonlyMap<string, number> {
    const m = new Map<string, number>();
    for (const it of this.bot.inventory.items()) {
      m.set(it.name, (m.get(it.name) ?? 0) + it.count);
    }
    return m;
  }

  health(): number {
    return this.bot.health;
  }

  #stopMoving = () => this.bot.pathfinder.stop();

  async #go(goal: InstanceType<typeof goals.Goal>, signal: AbortSignal): Promise<boolean> {
    const ok = async () => {
      await this.bot.pathfinder.goto(goal);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, ok, this.#stopMoving, false);
  }

  goto(to: Pos, range: number, signal: AbortSignal): Promise<boolean> {
    return this.#go(new goals.GoalNear(to.x, to.y, to.z, range), signal);
  }

  gotoY(y: number, signal: AbortSignal): Promise<boolean> {
    return this.#go(new goals.GoalY(y), signal);
  }

  async follow(player: string, range: number, ms: number, signal: AbortSignal): Promise<boolean> {
    const target = this.bot.players[player]?.entity;
    if (target === undefined) {
      return false;
    }
    this.bot.pathfinder.setGoal(new goals.GoalFollow(target, range), true);
    const stay = new Promise<boolean>((resolve) => setTimeout(() => resolve(true), ms));
    const kept = await bounded(signal, ms + 1000, () => stay, this.#stopMoving, false);
    this.bot.pathfinder.setGoal(null);
    return kept;
  }

  findBlock(names: readonly string[], radius: number): Pos | null {
    const ids = names.flatMap((n) => this.bot.registry.blocksByName[n]?.id ?? []);
    const found = this.bot.findBlock({ matching: ids, maxDistance: radius });
    return found === null ? null : posOf(found.position);
  }

  /** Block properties arrive as strings ("7"), so the age is parsed. */
  findRipe(name: string, radius: number): Pos | null {
    const id = this.bot.registry.blocksByName[name]?.id;
    const found = this.bot.findBlock({
      matching: (b: Block) => b.type === id && Number(b.getProperties().age) === RIPE_AGE,
      maxDistance: radius,
      useExtraInfo: true,
    });
    return found === null ? null : posOf(found.position);
  }

  findSoil(radius: number): Pos | null {
    const ids = ["dirt", "grass_block"].flatMap((n) => this.bot.registry.blocksByName[n]?.id ?? []);
    const found = this.bot.findBlock({
      matching: (b: Block) =>
        ids.includes(b.type) && this.bot.blockAt(b.position.offset(0, 1, 0))?.name === "air" && !this.#underfoot(b.position),
      maxDistance: radius,
      useExtraInfo: true,
    });
    return found === null ? null : posOf(found.position);
  }

  /**
   * Whether `p` is the block the bot stands on. Tilling it failed or got
   * trampled about half the time in the live probe of 15 Nov 2026.
   */
  #underfoot(p: Vec3): boolean {
    const me = this.position();
    return p.x === me.x && p.z === me.z && p.y === me.y - 1;
  }

  blockName(at: Pos): string | null {
    return this.bot.blockAt(v(at))?.name ?? null;
  }

  async use(item: string, at: Pos, signal: AbortSignal): Promise<boolean> {
    const stack = this.bot.inventory.items().find((i) => i.name === item);
    if (stack === undefined || !(await this.goto(at, USE_RANGE, signal))) {
      return false;
    }
    const apply = async () => {
      const block = this.bot.blockAt(v(at));
      if (block === null) {
        return false;
      }
      await this.bot.equip(stack, "hand");
      await this.bot.activateBlock(block, new Vec3(0, 1, 0));
      // Let the server's block update arrive before the caller looks.
      await this.bot.waitForTicks(USE_SETTLE_TICKS);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, apply, () => undefined, false);
  }

  async dig(at: Pos, signal: AbortSignal): Promise<boolean> {
    const block = this.bot.blockAt(v(at));
    if (block === null || block.name === "air" || !this.#canHarvest(block)) {
      return false;
    }
    // Next to crops and grass the pathfinder often gives up deciding a
    // path ("Took to long to decide path to goal") on the first try and
    // finds one on the second (live probe, 15 Nov 2026), so try twice.
    const collect = async () => {
      await this.bot.collectBlock.collect(block).catch(() => this.bot.collectBlock.collect(block));
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, collect, () => this.bot.collectBlock.cancelTask(), false);
  }

  /**
   * Whether a held tool can harvest `block` (stone needs a pickaxe). Asking
   * collectBlock for a block it cannot harvest ran the bot out of memory
   * in the live run of 14 Nov 2026 (dig-in on stone, no pickaxe).
   */
  #canHarvest(block: Block): boolean {
    const tools = block.harvestTools;
    return tools === undefined || this.bot.inventory.items().some((i) => tools[i.type] === true);
  }

  /** A solid neighbour of `at` to place against, and the face to use. */
  #reference(at: Pos): [Block, Vec3] | null {
    const faces = [new Vec3(0, -1, 0), new Vec3(1, 0, 0), new Vec3(-1, 0, 0), new Vec3(0, 0, 1), new Vec3(0, 0, -1), new Vec3(0, 1, 0)];
    for (const f of faces) {
      const ref = this.bot.blockAt(v(at).plus(f));
      if (ref !== null && ref.boundingBox === "block") {
        return [ref, f.scaled(-1)];
      }
    }
    return null;
  }

  async place(item: string, at: Pos, signal: AbortSignal): Promise<boolean> {
    const stack = this.bot.inventory.items().find((i) => i.name === item);
    const ref = this.#reference(at);
    if (stack === undefined || ref === null) {
      return false;
    }
    const put = async () => {
      await this.bot.equip(stack, "hand");
      await this.bot.placeBlock(ref[0], ref[1]);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, put, () => undefined, false);
  }

  #mobNear(at: Pos): Entity | null {
    return this.bot.nearestEntity(
      (e) => e !== this.bot.entity && e.type !== "player" && e.position.distanceTo(v(at)) <= 3,
    );
  }

  /**
   * Spaced melee (control/spacing.ts): each game tick, hit when the swing has
   * recharged and the mob is in reach, back off when it is too close while
   * recharging, close in when recharged and out of reach, else hold. Done
   * when the mob is gone (dead or despawned); gives up if it gets away.
   */
  async attack(at: Pos, signal: AbortSignal): Promise<boolean> {
    const mob = this.#mobNear(at);
    if (mob === null) {
      return false;
    }
    const held = this.bot.heldItem?.name ?? "";
    const swing = new Swing(getCooldown(held));
    const events = this.bot as unknown as EventEmitter;
    let onTick = () => undefined as void;
    const stop = () => {
      events.removeListener("physicsTick", onTick);
      this.bot.pathfinder.setGoal(null);
    };
    const fight = () =>
      new Promise<boolean>((resolve) => {
        let last: Move | null = null;
        onTick = () => {
          swing.tick();
          const d = mob.isValid ? mob.position.distanceTo(this.bot.entity.position) : 0;
          if (!mob.isValid || d > GIVE_UP_RANGE) {
            stop();
            resolve(!mob.isValid);
            return;
          }
          const move = nextMove(d, swing.ready());
          if (move === "hit") {
            void this.bot.lookAt(mob.position.offset(0, mob.height * 0.8, 0), true);
            this.bot.attack(mob);
            swing.swung();
          }
          if (move !== last) {
            this.bot.pathfinder.setGoal(goalFor(move, mob), true);
            last = move;
          }
        };
        events.on("physicsTick", onTick);
      });
    return bounded(signal, CALL_LIMIT_MS, fight, stop, false);
  }

  async eat(item: string, signal: AbortSignal): Promise<boolean> {
    const food = this.bot.inventory.items().find((i) => i.name === item);
    if (food === undefined) {
      return false;
    }
    const consume = async () => {
      await this.bot.equip(food, "hand");
      await this.bot.consume();
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, consume, () => undefined, false);
  }

  async craft(item: string, table: Pos | null, signal: AbortSignal): Promise<boolean> {
    const id = this.bot.registry.itemsByName[item]?.id;
    if (id === undefined) {
      return false;
    }
    if (table !== null && !(await this.goto(table, 2, signal))) {
      return false;
    }
    const station = table === null ? null : this.bot.blockAt(v(table));
    const recipe = this.bot.recipesFor(id, null, 1, station)[0];
    if (recipe === undefined) {
      return false;
    }
    const make = async () => {
      await this.bot.craft(recipe, 1, station ?? undefined);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, make, () => undefined, false);
  }

  async smelt(input: string, furnace: Pos, signal: AbortSignal): Promise<boolean> {
    const reg = this.bot.registry.itemsByName;
    const inId = reg[input]?.id;
    const fuel = ["coal", "charcoal", "oak_planks", "oak_log"].find((f) => this.inventory().get(f));
    const block = this.bot.blockAt(v(furnace));
    if (inId === undefined || fuel === undefined || block === null) {
      return false;
    }
    if (!(await this.goto(furnace, 2, signal))) {
      return false;
    }
    const cook = async () => {
      const f = await this.bot.openFurnace(block);
      try {
        await f.putFuel(reg[fuel]?.id as number, null, 1);
        await f.putInput(inId, null, 1);
        while (f.outputItem() === null) {
          await new Promise((r) => setTimeout(r, 500));
        }
        await f.takeOutput();
        return true;
      } finally {
        f.close();
      }
    };
    return bounded(signal, CALL_LIMIT_MS, cook, () => undefined, false);
  }

  async sleep(bed: Pos, signal: AbortSignal): Promise<boolean> {
    const block = this.bot.blockAt(v(bed));
    if (block === null || !(await this.goto(bed, 2, signal))) {
      return false;
    }
    const lie = async () => {
      await this.bot.sleep(block);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, lie, () => undefined, false);
  }

  async wait(ms: number, signal: AbortSignal): Promise<void> {
    await bounded(signal, ms, () => new Promise<void>(() => undefined), () => undefined, undefined);
  }
}

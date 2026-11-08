// The Body interface on a live Mineflayer bot (spec §22). Glue only: each
// method calls the library and turns errors and timeouts into `false`.
// Checked by the live acceptance tests, not unit tests (ARCHITECTURE.md §1).

import type { EventEmitter } from "node:events";

import type { Bot } from "mineflayer";
// Type imports for the plugins' additions to `Bot` (collectBlock, pvp).
import type {} from "mineflayer-collectblock";
import pathfinderPkg from "mineflayer-pathfinder";
import type {} from "mineflayer-pvp";
import type { Block } from "prismarine-block";
import type { Entity } from "prismarine-entity";
import { Vec3 } from "vec3";

import type { Pos } from "../host.js";
import type { Body } from "../skills/body.js";

const { goals } = pathfinderPkg;

/** Longest any single body call may take, in ms. */
export const CALL_LIMIT_MS = 60_000;

const v = (p: Pos) => new Vec3(p.x, p.y, p.z);

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
    return found === null ? null : { x: found.position.x, y: found.position.y, z: found.position.z };
  }

  async dig(at: Pos, signal: AbortSignal): Promise<boolean> {
    const block = this.bot.blockAt(v(at));
    if (block === null || block.name === "air") {
      return false;
    }
    const collect = async () => {
      await this.bot.collectBlock.collect(block);
      return true;
    };
    return bounded(signal, CALL_LIMIT_MS, collect, () => this.bot.collectBlock.cancelTask(), false);
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

  async attack(at: Pos, signal: AbortSignal): Promise<boolean> {
    const mob = this.#mobNear(at);
    if (mob === null) {
      return false;
    }
    const fight = () =>
      new Promise<boolean>((resolve) => {
        const events = this.bot as unknown as EventEmitter;
        events.once("stoppedAttacking", () => resolve(!mob.isValid || (mob.health ?? 0) <= 0));
        void this.bot.pvp.attack(mob);
      });
    return bounded(signal, CALL_LIMIT_MS, fight, () => void this.bot.pvp.stop(), false);
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

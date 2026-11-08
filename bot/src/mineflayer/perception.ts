// Read a live Mineflayer bot into a Snapshot (spec §2, §5). Glue only: the
// choices about what to report are in perceive/ and unit-tested there.

import type { Bot } from "mineflayer";
import { Vec3 } from "vec3";

import type { ChatMsg } from "../host.js";
import type { BodyState } from "../control/survival.js";
import { MAX_FOOD, MAX_HEALTH, type SeenEntity, type Snapshot, notable } from "../perceive/observation.js";
import { enclosed } from "../perceive/shelter.js";

/** Most block positions asked of Mineflayer per scan. */
const SCAN_COUNT = 512;

export class Perception {
  #chat: ChatMsg[] = [];
  #lastHealth: number;
  readonly #ids: number[];

  constructor(
    private readonly bot: Bot,
    private readonly viewRadius: number,
  ) {
    this.#lastHealth = bot.health;
    this.#ids = bot.registry.blocksArray.filter((b) => notable(b.name)).map((b) => b.id);
    bot.on("chat", (from, text) => {
      if (from !== bot.username) {
        this.#chat.push({ from, text });
      }
    });
  }

  #entities(): SeenEntity[] {
    const me = this.bot.entity;
    return Object.values(this.bot.entities)
      .filter((e) => e !== me && e.position.distanceTo(me.position) <= this.viewRadius)
      .map((e) => ({
        name: e.type === "player" ? (e.username ?? "player") : (e.name ?? "unknown"),
        kind: e.type === "player" ? "player" : e.type === "hostile" ? "hostile" : "passive",
        pos: e.position,
      }));
  }

  #sheltered(): boolean {
    const feet = this.bot.entity.position.floored();
    const solid = (dx: number, dy: number, dz: number) =>
      this.bot.blockAt(feet.plus(new Vec3(dx, dy, dz)))?.boundingBox === "block";
    return enclosed(solid);
  }

  /** What the survival interrupt checks; reads nothing it would consume. */
  body(isFood: (item: string) => boolean): BodyState {
    const bot = this.bot;
    return {
      health: bot.health / MAX_HEALTH,
      food: bot.food / MAX_FOOD,
      sheltered: this.#sheltered(),
      hostileNear: this.#entities().some((e) => e.kind === "hostile"),
      hasFood: bot.inventory.items().some((i) => isFood(i.name)),
    };
  }

  snapshot(): Snapshot {
    const bot = this.bot;
    const blocks = bot
      .findBlocks({ matching: this.#ids, maxDistance: this.viewRadius, count: SCAN_COUNT })
      .flatMap((p) => {
        const b = bot.blockAt(p);
        return b === null ? [] : [{ name: b.name, pos: p }];
      });
    const damage = Math.max(0, this.#lastHealth - bot.health);
    this.#lastHealth = bot.health;
    const chat = this.#chat;
    this.#chat = [];
    return {
      age: Number(bot.time.age),
      timeOfDay: bot.time.timeOfDay,
      health: bot.health,
      food: bot.food,
      position: bot.entity.position,
      inventory: bot.inventory.items().map((i) => ({ name: i.name, count: i.count })),
      blocks,
      entities: this.#entities(),
      chat,
      damage,
      sleeping: bot.isSleeping,
      sheltered: this.#sheltered(),
    };
  }
}

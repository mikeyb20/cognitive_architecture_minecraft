// The bot's home in Minecraft (Michael, 15 Nov 2026): the pathfinder may
// not dig through it, and the bot goes in and out by the door, closing it
// behind. Glue only: where the door is comes from skills/run.ts, when to
// close it from control/door.ts. Checked by live probes, not unit tests.

import type { Bot } from "mineflayer";
import { Vec3 } from "vec3";

import { shouldClose } from "../control/door.js";
import type { Pos } from "../host.js";
import { homeDoor, homeInside, homeOutside, inHome, inRoom } from "../skills/run.js";

/** Pathfinder cost that rules a move out: the home is never dug through. */
const NEVER = 1000;
/** Longest walk through the doorway, in ms. */
const WALK_MS = 4000;

const v = (p: Pos) => new Vec3(p.x, p.y, p.z);

export class Home {
  at: Pos | null = null;
  #busy = false;

  constructor(private readonly bot: Bot) {
    bot.on("physicsTick", () => this.#shut());
  }

  set(at: Pos | null): void {
    this.at = at;
    const away = (b: { position: Vec3 }) => (at !== null && inHome(at, b.position) ? NEVER : 0);
    // Neither dug through nor scaffolded into.
    this.bot.pathfinder.movements.exclusionAreasBreak = [away];
    this.bot.pathfinder.movements.exclusionAreasPlace = [away];
  }

  #feet(): Pos {
    const p = this.bot.entity.position.floored();
    return { x: p.x, y: p.y, z: p.z };
  }

  #door() {
    const door = this.at === null ? null : this.bot.blockAt(v(homeDoor(this.at)));
    return door?.name.endsWith("_door") === true ? door : null;
  }

  /** Close the door if it was left open (say, a walk cut short); runs every tick. */
  #shut(): void {
    const door = this.#door();
    if (door === null || this.#busy) {
      return;
    }
    const feet = this.#feet();
    const view = {
      open: String(door.getProperties().open) === "true",
      distance: this.bot.entity.position.distanceTo(door.position.offset(0.5, 0, 0.5)),
      inDoorway: feet.x === door.position.x && feet.z === door.position.z,
      moving: this.bot.pathfinder.isMoving(),
    };
    if (shouldClose(view)) {
      this.#busy = true;
      void this.bot.activateBlock(door).finally(() => {
        this.#busy = false;
      });
    }
  }

  /** Close the door behind the bot, if open. */
  async #close(): Promise<void> {
    const door = this.#door();
    if (door !== null && String(door.getProperties().open) === "true") {
      await this.bot.activateBlock(door);
      await this.bot.waitForTicks(2);
    }
  }

  /** Open the door, walk straight through it to `to`, and close it behind. */
  async #through(to: Pos, signal: AbortSignal): Promise<boolean> {
    const door = this.#door();
    if (door === null) {
      return false;
    }
    this.#busy = true;
    try {
      if (String(door.getProperties().open) !== "true") {
        await this.bot.activateBlock(door);
      }
      await this.bot.lookAt(v(to).offset(0.5, this.bot.entity.height, 0.5), true);
      this.bot.setControlState("forward", true);
      const end = Date.now() + WALK_MS;
      while (!signal.aborted && Date.now() < end) {
        const f = this.#feet();
        if (f.x === to.x && f.z === to.z) {
          this.bot.setControlState("forward", false);
          await this.#close();
          return true;
        }
        await this.bot.waitForTicks(1);
      }
      return false;
    } finally {
      this.bot.setControlState("forward", false);
      this.#busy = false;
    }
  }

  /** Go out by the door when in the room and bound for `target` outside it. */
  async leave(target: Pos | null, signal: AbortSignal, go: (to: Pos) => Promise<boolean>): Promise<void> {
    const o = this.at;
    if (o === null || !inRoom(o, this.#feet()) || (target !== null && inRoom(o, target)) || this.#door() === null) {
      return;
    }
    if (await go(homeInside(o))) {
      await this.#through(homeOutside(o), signal);
    }
  }

  /** Come in by the door, or make sure it is shut when already in; true when in the room. */
  async enter(signal: AbortSignal, go: (to: Pos) => Promise<boolean>): Promise<boolean> {
    const o = this.at;
    if (o === null) {
      return false;
    }
    if (inRoom(o, this.#feet())) {
      await this.#close();
      return true;
    }
    return (await go(homeOutside(o))) && (await this.#through(homeInside(o), signal));
  }
}

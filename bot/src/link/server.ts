// The bot's end of the runner link (spec §22): the runner decides, the bot
// answers. Each request gets one reply; the body runs one action at a time
// under the action lock, and the survival interrupt can cut it short.

import { type BodyState, interruptIfEmergency } from "../control/survival.js";
import { ActionLock, Priority } from "../control/lock.js";
import type { BotConfig, LinkReply, LinkRequest } from "../host.js";
import { type Snapshot, toObservation } from "../perceive/observation.js";
import type { Body } from "../skills/body.js";
import { run } from "../skills/run.js";

/** What the server needs from the live bot (the glue supplies it). */
export interface Deps {
  /** Every block and item name the server's version knows. */
  names(): string[];
  snapshot(): Snapshot;
  body: Body;
  say(text: string): void;
}

/** Who holds the lock while an `execute` runs. */
export const TASK = "runner";

export class BotServer {
  #config: BotConfig | null = null;
  #held = new Set<string>();
  readonly lock = new ActionLock();

  constructor(private readonly deps: Deps) {}

  /** The settings from the runner's hello, once it has said hello. */
  config(): BotConfig | null {
    return this.#config;
  }

  async handle(req: LinkRequest): Promise<LinkReply> {
    if (req.op === "hello") {
      this.#config = req.config;
      return { reply: "ready", names: this.deps.names() };
    }
    const cfg = this.#config;
    if (cfg === null) {
      return { reply: "failed", message: "say hello first" };
    }
    switch (req.op) {
      case "perceive":
        return { reply: "observed", obs: toObservation(this.deps.snapshot(), cfg.limits) };
      case "execute": {
        const go = (signal: AbortSignal) => run(this.deps.body, req, this.#held, signal, cfg.skills);
        const outcome = await this.lock.with(TASK, Priority.Task, go);
        return outcome === null ? { reply: "failed", message: "the body is busy" } : { reply: "done", outcome };
      }
      case "say":
        this.deps.say(req.text);
        return { reply: "said" };
    }
  }

  /**
   * One JSON line in, one JSON line out. A line that is not a request gets
   * a `failed` reply; a skill that throws is reported the same way.
   */
  async answer(line: string): Promise<string> {
    let reply: LinkReply;
    try {
      reply = await this.handle(JSON.parse(line) as LinkRequest);
    } catch (e) {
      reply = { reply: "failed", message: String(e) };
    }
    return JSON.stringify(reply);
  }

  /** Call on every health or food change: stops the running skill in an emergency. */
  watch(s: BodyState): boolean {
    const cfg = this.#config;
    return cfg !== null && interruptIfEmergency(this.lock, s, cfg.thresholds);
  }

  /** Death or reconnect: abort the running skill and start clean. */
  reset(): void {
    this.lock.reset();
  }
}

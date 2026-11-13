// A scripted body for skill tests: records every call and answers from a
// table. Not shipped logic; excluded from mutation testing.

import type { Pos } from "../host.js";
import type { Body } from "./body.js";

export type Call = [string, ...unknown[]];

export class FakeBody implements Body {
  calls: Call[] = [];
  inv = new Map<string, number>();
  hp = 20;
  tick = 0;
  at: Pos = { x: 0, y: 64, z: 0 };
  /** Answer for each method; default true. */
  answers = new Map<string, boolean>();
  /** Blocks findBlock returns, by first name asked. */
  blocks = new Map<string, Pos>();
  /** Side effects per method (e.g. add items, move). */
  effects = new Map<string, (...args: unknown[]) => void>();

  #do(name: string, ...args: unknown[]): boolean {
    this.calls.push([name, ...args]);
    this.effects.get(name)?.(...args);
    return this.answers.get(name) ?? true;
  }

  age(): number {
    return this.tick;
  }
  position(): Pos {
    return this.at;
  }
  inventory(): ReadonlyMap<string, number> {
    return this.inv;
  }
  health(): number {
    return this.hp;
  }
  async goto(to: Pos, range: number): Promise<boolean> {
    return this.#do("goto", to, range);
  }
  async gotoY(y: number): Promise<boolean> {
    return this.#do("gotoY", y);
  }
  async follow(player: string, range: number, ms: number): Promise<boolean> {
    return this.#do("follow", player, range, ms);
  }
  findBlock(names: readonly string[], radius: number): Pos | null {
    this.calls.push(["findBlock", [...names], radius]);
    return this.blocks.get(names[0] as string) ?? null;
  }
  async dig(at: Pos): Promise<boolean> {
    return this.#do("dig", at);
  }
  async place(item: string, at: Pos): Promise<boolean> {
    return this.#do("place", item, at);
  }
  async attack(at: Pos): Promise<boolean> {
    return this.#do("attack", at);
  }
  async eat(item: string): Promise<boolean> {
    return this.#do("eat", item);
  }
  async craft(item: string, table: Pos | null): Promise<boolean> {
    return this.#do("craft", item, table);
  }
  async smelt(input: string, furnace: Pos): Promise<boolean> {
    return this.#do("smelt", input, furnace);
  }
  async sleep(bed: Pos): Promise<boolean> {
    return this.#do("sleep", bed);
  }
  async wait(ms: number): Promise<void> {
    this.#do("wait", ms);
  }
}

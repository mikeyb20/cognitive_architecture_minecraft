// Run one candidate with the body (spec §2, §22) and report the standard
// skill result (kept from v0.2 R6.1) as the shared Outcome. Each skill is a
// short plan over Body calls; all the Minecraft detail lives in the glue.

import type { Candidate, Intensity, Outcome, Pos, Target } from "../host.js";
import type { Body } from "./body.js";

/** Tuning the runner sends at connect (proposed until the pilot). */
export interface SkillConfig {
  /** How far to look for a searched block. */
  searchRadius: number;
  /** Height to dig down to when searching for ore (iron is common near 16). */
  oreDepth: number;
  /** Stay this close when following a player. */
  followRange: number;
  /** How long one follow or come action lasts. */
  followMs: number;
  /** How long one rest without a bed lasts. */
  restMs: number;
  /** How far one search step walks when nothing is in range. */
  searchStep: number;
}

export const DEFAULT_SKILLS: SkillConfig = {
  searchRadius: 64,
  oreDepth: 16,
  followRange: 3,
  followMs: 5000,
  restMs: 5000,
  searchStep: 24,
};

/** For crafts: what the tech tree says the recipe needs (sent by the runner). */
export interface RecipeHint {
  station: string | null;
  inputs: Array<[string, number]>;
}

export interface Request {
  action: Candidate;
  intensity: Intensity;
  recipe?: RecipeHint;
}

interface Done {
  success: boolean;
  progress: number;
  blocked: boolean;
}

const BLOCKED: Done = { success: false, progress: 0, blocked: true };
const ORES = ["iron_ore", "coal_ore", "gold_ore", "diamond_ore"];
const BUILDING = ["cobblestone", "dirt", "oak_planks", "cobbled_deepslate"];

function done(ok: boolean): Done {
  return { success: ok, progress: ok ? 1 : 0, blocked: false };
}

function pos(t: Target | null): Pos | null {
  return t !== null && "Pos" in t ? t.Pos : null;
}

function item(t: Target | null): string | null {
  return t !== null && "Item" in t ? t.Item : null;
}

function player(t: Target | null): string | null {
  return t !== null && "Player" in t ? t.Player : null;
}

function thing(t: Target | null): string | null {
  return t !== null && "Thing" in t ? t.Thing : null;
}

/** Names to look for when searching for a memory key ("block:iron_ore"). */
export function searchNames(key: string): string[] {
  const name = key.slice(key.indexOf(":") + 1);
  return ORES.includes(name) ? [name, `deepslate_${name}`] : [name];
}

/** Find a searched block: in range, else (for ore) after digging down, else walk on. */
async function search(body: Body, key: string, cfg: SkillConfig, signal: AbortSignal): Promise<Done> {
  const names = searchNames(key);
  let at = body.findBlock(names, cfg.searchRadius);
  if (at === null && ORES.includes(names[0] as string) && body.position().y > cfg.oreDepth) {
    await body.gotoY(cfg.oreDepth, signal);
    at = body.findBlock(names, cfg.searchRadius);
  }
  if (at !== null) {
    return done(await body.dig(at, signal));
  }
  // Nothing in range: one step in a direction that turns with time.
  const turn = Math.floor(body.age() / 1200) % 4;
  const [dx, dz] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
  ][turn] as [number, number];
  const me = body.position();
  const to = { x: me.x + dx * cfg.searchStep, y: me.y, z: me.z + dz * cfg.searchStep };
  const moved = await body.goto(to, 2, signal);
  return { success: false, progress: moved ? 0.25 : 0, blocked: false };
}

/** The station block near the agent, placing one from the inventory if needed. */
async function station(body: Body, name: string, signal: AbortSignal): Promise<Pos | null> {
  const near = body.findBlock([name], 16);
  if (near !== null || (body.inventory().get(name) ?? 0) === 0) {
    return near;
  }
  const me = body.position();
  const spot = { x: me.x + 1, y: me.y, z: me.z };
  return (await body.place(name, spot, signal)) ? spot : null;
}

async function craft(body: Body, out: string, hint: RecipeHint | undefined, signal: AbortSignal): Promise<Done> {
  if (hint === undefined || hint.station === null) {
    return done(await body.craft(out, null, signal));
  }
  const need = hint.station;
  const at = await station(body, need, signal);
  if (at === null) {
    return BLOCKED;
  }
  if (need === "furnace") {
    const input = hint.inputs[0]?.[0];
    return input === undefined ? BLOCKED : done(await body.smelt(input, at, signal));
  }
  return done(await body.craft(out, at, signal));
}

/**
 * Dig in: three blocks down, then close the top of the hole (needs one
 * building block). The top is placed one below the old floor, so the
 * ground around it holds it.
 */
async function shelter(body: Body, signal: AbortSignal): Promise<Done> {
  const block = BUILDING.find((b) => (body.inventory().get(b) ?? 0) > 0);
  if (block === undefined) {
    return BLOCKED;
  }
  const top = body.position();
  const below = (n: number) => ({ x: top.x, y: top.y - n, z: top.z });
  let dug = true;
  for (const n of [1, 2, 3]) {
    dug = dug && (await body.dig(below(n), signal));
  }
  return done(dug && (await body.place(block, below(1), signal)));
}

async function act(body: Body, req: Request, cfg: SkillConfig, signal: AbortSignal): Promise<Done> {
  const t = req.action.target;
  const at = pos(t);
  switch (req.action.skill) {
    case "explore":
    case "flee":
      return at === null ? BLOCKED : done(await body.goto(at, 2, signal));
    case "move_to": {
      const who = player(t);
      if (who !== null) {
        return done(await body.follow(who, cfg.followRange, cfg.followMs, signal));
      }
      return at === null ? BLOCKED : done(await body.goto(at, 1, signal));
    }
    case "mine":
    case "chop":
    case "farm": {
      const key = thing(t);
      if (key !== null) {
        return search(body, key, cfg, signal);
      }
      return at === null ? BLOCKED : done(await body.dig(at, signal));
    }
    case "attack":
      return at === null ? BLOCKED : done(await body.attack(at, signal));
    case "eat": {
      const food = item(t);
      return food === null ? BLOCKED : done(await body.eat(food, signal));
    }
    case "craft": {
      const out = item(t);
      return out === null ? BLOCKED : craft(body, out, req.recipe, signal);
    }
    case "shelter":
      return shelter(body, signal);
    case "rest":
      if (at !== null) {
        return done(await body.sleep(at, signal));
      }
      await body.wait(cfg.restMs, signal);
      return done(!signal.aborted);
    case "follow_player": {
      const who = player(t);
      return who === null ? BLOCKED : done(await body.follow(who, cfg.followRange, cfg.followMs, signal));
    }
    case "place":
    case "chat":
      return BLOCKED;
  }
}

/** Item counts that went up, as an inventory. */
export function gained(before: ReadonlyMap<string, number>, after: ReadonlyMap<string, number>): Array<[string, number]> {
  return [...after.keys()]
    .sort()
    .map((name): [string, number] => [name, (after.get(name) ?? 0) - (before.get(name) ?? 0)])
    .filter(([, d]) => d > 0);
}

/**
 * Run one action and report it (v0.2 R6.1 standard result). `held` is
 * every item name the agent has ever held; first-time items are reported
 * as new things ("item:<name>") and added to it. An interrupted action
 * (the signal aborted) is never a success.
 */
export async function run(
  body: Body,
  req: Request,
  held: Set<string>,
  signal: AbortSignal,
  cfg: SkillConfig = DEFAULT_SKILLS,
): Promise<Outcome> {
  const start = body.age();
  const before = new Map(body.inventory());
  const hp = body.health();
  const r = await act(body, req, cfg, signal);
  const got = gained(before, body.inventory());
  const fresh = got.map(([n]) => n).filter((n) => !held.has(n));
  for (const n of fresh) {
    held.add(n);
  }
  return {
    success: r.success && !signal.aborted,
    progress: r.progress,
    items_gained: { items: got },
    damage: Math.max(0, hp - body.health()) / 20,
    new_things: fresh.map((n) => `item:${n}`),
    blocked: r.blocked,
    duration_ticks: Math.max(0, body.age() - start),
  };
}

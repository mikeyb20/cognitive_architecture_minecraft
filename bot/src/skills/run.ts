// Run one candidate with the body (spec §2, §22) and report the standard
// skill result (kept from v0.2 R6.1) as the shared Outcome. Each skill is a
// short plan over Body calls; all the Minecraft detail lives in the glue.

import type { Candidate, Intensity, Outcome, Pos, RecipeHint, SkillConfig, Target } from "../host.js";
import { ENCLOSURE } from "../perceive/shelter.js";
import type { Body } from "./body.js";

/** One `execute` from the runner (host-api `LinkRequest::Execute`). */
export interface Request {
  action: Candidate;
  intensity: Intensity;
  /** For crafts: what the tech tree says the recipe needs. */
  recipe: RecipeHint | null;
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

async function craft(body: Body, out: string, hint: RecipeHint | null, signal: AbortSignal): Promise<Done> {
  if (hint === null || hint.station === null) {
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
 * The enclosure to build: walls, then one block beside the roof, then the
 * roof. A block can only be placed against a neighbour, and the roof has
 * none until that one is in.
 */
const BUILD = [...ENCLOSURE.filter(([, dy]) => dy < 2), [1, 2, 0] as const, ...ENCLOSURE.filter(([, dy]) => dy >= 2)];

/** Blocks a placed block may take the place of. */
const OPEN = ["air", "cave_air", "short_grass", "tall_grass"];

/**
 * The quick shelter: three blocks down, then close the top of the hole (needs one
 * building block). The top is placed one below the old floor, so the
 * ground around it holds it. Where the ground cannot be dug (stone with
 * no pickaxe), build the enclosure around the agent instead.
 */
async function hole(body: Body, signal: AbortSignal): Promise<Done> {
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
  return dug ? done(await body.place(block, below(1), signal)) : build(body, signal);
}

/**
 * Wall the agent in where it stands: every open block of the enclosure the
 * `sheltered` check asks for (feet walls, head walls, roof), from any
 * building blocks held. Progress is the share of `BUILD` solid.
 */
async function build(body: Body, signal: AbortSignal): Promise<Done> {
  const me = body.position();
  let solid = 0;
  for (const [dx, dy, dz] of BUILD) {
    const at = { x: me.x + dx, y: me.y + dy, z: me.z + dz };
    const block = BUILDING.find((b) => (body.inventory().get(b) ?? 0) > 0);
    const open = OPEN.includes(body.blockName(at) ?? "air");
    if (open && (block === undefined || !(await body.place(block, at, signal)))) {
      break;
    }
    solid += 1;
  }
  const share = solid / BUILD.length;
  return { success: share === 1, progress: share, blocked: false };
}

/** Whether the inventory holds every input of `r`. */
function holds(body: Body, r: RecipeHint): boolean {
  return r.inputs.every(([i, n]) => (body.inventory().get(i) ?? 0) >= n);
}

const DOOR = "oak_door";
/** Minecraft 1.21's door recipe (3 doors); kept out of the shared tech tree, so the sim is unchanged. */
const DOOR_RECIPE: RecipeHint = { station: "crafting_table", inputs: [["oak_planks", 6]] };
/**
 * Building blocks to hold before starting a home (proposed): the head-high
 * wall ring (16) and the roof (25), less the 9 dug from the floor.
 */
export const HOME_BLOCKS = 32;
/** Blocks a home's digging leaves alone: its door, and what was put inside. */
const KEEP = ["_door", "_bed", "crafting_table", "chest", "furnace"];

type Step = { kind: "dig" | "stand" | "wall" | "door"; at: Pos };

/** Cells (dx, dz) at ring distance `r`, going round from (r, -r); the cell (r, 0) last. */
function ring(r: number): Array<[number, number]> {
  const cells: Array<[number, number]> = [];
  for (let dx = -r; dx <= r; dx++) {
    for (let dz = -r; dz <= r; dz++) {
      if (Math.max(Math.abs(dx), Math.abs(dz)) === r && !(dx === r && dz === 0)) {
        cells.push([dx, dz]);
      }
    }
  }
  return [...cells, [r, 0]];
}

/**
 * The home (Michael, 15 Nov 2026): a 3x3 room around `o` sunk one block
 * into the ground, walls two high, a roof, and a door on the +x side. In
 * build order: dig the floor and the doorway, stand inside, wall, roof
 * (each block placed against one already there), then the door.
 */
export function homePlan(o: Pos): Step[] {
  const at = (dx: number, dy: number, dz: number) => ({ x: o.x + dx, y: o.y + dy, z: o.z + dz });
  const inner = [...ring(1), [0, 0] as [number, number]];
  const walls = ring(2).filter(([dx, dz]) => !(dx === 2 && dz === 0));
  return [
    ...inner.map(([dx, dz]): Step => ({ kind: "dig", at: at(dx, -1, dz) })),
    { kind: "dig", at: at(2, -1, 0) },
    { kind: "dig", at: at(2, 0, 0) },
    { kind: "stand", at: at(0, -1, 0) },
    ...[-1, 0].flatMap((dy) => walls.map(([dx, dz]): Step => ({ kind: "wall", at: at(dx, dy, dz) }))),
    ...[...ring(2), ...inner].map(([dx, dz]): Step => ({ kind: "wall", at: at(dx, 1, dz) })),
    { kind: "door", at: at(2, -1, 0) },
  ];
}

/** Do one step of a home; true when the block is as the plan wants it. */
async function homeStep(body: Body, s: Step, signal: AbortSignal): Promise<boolean> {
  const name = body.blockName(s.at) ?? "air";
  const open = OPEN.includes(name);
  const held = (n: string) => (body.inventory().get(n) ?? 0) > 0;
  switch (s.kind) {
    case "dig":
      return open || KEEP.some((k) => name.endsWith(k)) || body.dig(s.at, signal);
    case "stand":
      return body.goto(s.at, 0, signal);
    case "wall": {
      const block = BUILDING.find(held);
      return !open || (block !== undefined && body.place(block, s.at, signal));
    }
    case "door":
      if (name === DOOR) {
        return true;
      }
      if (!held(DOOR) && !(holds(body, DOOR_RECIPE) && (await craft(body, DOOR, DOOR_RECIPE, signal)).success)) {
        return false;
      }
      return body.place(DOOR, s.at, signal);
  }
}

/**
 * Shelter (spec §2): go home and finish it, or start a home here when
 * holding enough blocks; otherwise the quick hole. Progress is the share
 * of the home's plan done; success is a home that passes the shelter
 * check with the agent inside.
 */
async function shelter(body: Body, cfg: SkillConfig, signal: AbortSignal): Promise<Done> {
  const count = BUILDING.reduce((n, b) => n + (body.inventory().get(b) ?? 0), 0);
  const me = body.position();
  const known = body.home();
  const near = known !== null && Math.max(Math.abs(known.x - me.x), Math.abs(known.z - me.z)) <= cfg.searchRadius;
  if (!near && count < HOME_BLOCKS) {
    return hole(body, signal);
  }
  const o = near ? known : me;
  body.setHome(o);
  const steps = homePlan(o);
  let done = 0;
  while (done < steps.length && (await homeStep(body, steps[done] as Step, signal))) {
    done += 1;
  }
  return { success: done === steps.length && body.sheltered(), progress: done / steps.length, blocked: false };
}

const GRASS = ["short_grass", "tall_grass"];
const SEEDS = "wheat_seeds";
const HOE = "wooden_hoe";
/** Minecraft 1.21's wooden hoe recipe; not in the shared tech tree, so the sim is unchanged. */
const HOE_RECIPE: RecipeHint = {
  station: "crafting_table",
  inputs: [
    ["oak_planks", 2],
    ["stick", 2],
  ],
};

/**
 * Farm (host-api `Skill::Farm`, "harvest or plant wheat"). Wild wheat does
 * not grow in Minecraft, so the agent grows its own: harvest ripe wheat in
 * range; else break grass for seeds; else make a wooden hoe; else till one
 * soil block and sow it. Harvest and sowing succeed; the steps before them
 * only make progress.
 */
async function farm(body: Body, cfg: SkillConfig, signal: AbortSignal): Promise<Done> {
  const ripe = body.findRipe("wheat", cfg.searchRadius);
  if (ripe !== null) {
    return done(await body.dig(ripe, signal));
  }
  const count = (n: string) => body.inventory().get(n) ?? 0;
  if (count(SEEDS) === 0) {
    const grass = body.findBlock(GRASS, cfg.searchRadius);
    if (grass === null) {
      return BLOCKED;
    }
    const broke = await body.dig(grass, signal);
    return { success: false, progress: broke && count(SEEDS) > 0 ? 0.5 : 0.25, blocked: false };
  }
  if (count(HOE) === 0) {
    if (!holds(body, HOE_RECIPE)) {
      return BLOCKED;
    }
    const made = await craft(body, HOE, HOE_RECIPE, signal);
    return { ...made, success: false, progress: made.success ? 0.5 : 0 };
  }
  const soil = body.findSoil(cfg.searchRadius);
  if (soil === null) {
    return BLOCKED;
  }
  const crop = { x: soil.x, y: soil.y + 1, z: soil.z };
  const sown = (await body.use(HOE, soil, signal)) && (await body.use(SEEDS, soil, signal));
  return done(sown && body.blockName(crop) === "wheat");
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
    case "farm":
      return farm(body, cfg, signal);
    case "mine":
    case "chop": {
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
      return shelter(body, cfg, signal);
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
 * (the signal aborted) is never a success. `cfg` comes from the runner
 * at connect (host-api `SkillConfig`).
 */
export async function run(
  body: Body,
  req: Request,
  held: Set<string>,
  signal: AbortSignal,
  cfg: SkillConfig,
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

// Shared helpers for the skill tests: positions, a live signal, the
// runner's skill defaults and a request builder. Not shipped logic.

import type { Candidate, Outcome, Skill, SkillConfig, Target } from "../host.js";
import type { Body } from "./body.js";
import { type Request, run } from "./run.js";

export const P = (x: number, y: number, z: number) => ({ x, y, z });
export const live = () => new AbortController().signal;

/** The runner's defaults (host-api `SkillConfig::default`). */
export const SKILLS: SkillConfig = {
  searchRadius: 64,
  oreDepth: 16,
  followRange: 3,
  followMs: 5000,
  restMs: 5000,
  searchStep: 24,
};

export function go(b: Body, r: Request, held: Set<string>, s: AbortSignal, cfg = SKILLS): Promise<Outcome> {
  return run(b, r, held, s, cfg);
}

export function req(skill: Skill, target: Target | null, recipe: Request["recipe"] = null): Request {
  const action: Candidate = {
    id: 0,
    skill,
    target,
    task_type: "mining",
    est_cost: 0.1,
    est_risk: 0,
    commanded: false,
  };
  return { action, intensity: { effort: 0.5, focus: 1, external_drive: false }, recipe };
}


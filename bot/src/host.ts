// Host contract mirrored from crates/host-api (spec §2). The Rust types are the
// source of truth; the napi-rs bridge (bindings/node) will generate or check
// these in a later build step. Keep field names identical (serde snake_case).

export type AgentId = number;

export interface Pos { x: number; y: number; z: number }

export type Skill =
  | "explore" | "move_to" | "mine" | "chop" | "craft" | "place" | "attack"
  | "flee" | "eat" | "farm" | "shelter" | "rest" | "follow_player" | "chat";

/** The 7 tenacity task types (#36). */
export type TaskType =
  | "mining" | "building" | "combat" | "crafting" | "farming" | "travel" | "helping";

export interface Candidate {
  id: number;
  skill: Skill;
  target: unknown | null;
  task_type: TaskType;
  est_cost: number;
  est_risk: number;
  /** Step of a player command; gets the external-drive floor (#41). */
  commanded: boolean;
}

export interface Intensity { effort: number; focus: number; external_drive: boolean }

export interface Outcome {
  success: boolean;
  progress: number;
  items_gained: { items: Array<[string, number]> };
  damage: number;
  new_things: string[];
  blocked: boolean;
  duration_ticks: number;
}

export interface Host<Observation> {
  perceive(): Promise<Observation>;
  candidateActions(obs: Observation): Promise<Candidate[]>;
  execute(action: Candidate, intensity: Intensity): Promise<Outcome>;
}

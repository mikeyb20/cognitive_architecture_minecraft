// Is the agent walled in so mobs cannot reach it (spec §2 `sheltered`)?
// The glue answers "is the block at this offset from my feet solid?".

/** Offsets (dx, dy, dz) from the feet that must all be solid. */
export const ENCLOSURE: ReadonlyArray<readonly [number, number, number]> = [
  // Walls at feet and head height.
  [1, 0, 0],
  [-1, 0, 0],
  [0, 0, 1],
  [0, 0, -1],
  [1, 1, 0],
  [-1, 1, 0],
  [0, 1, 1],
  [0, 1, -1],
  // A roof right over the head.
  [0, 2, 0],
];

export function enclosed(solid: (dx: number, dy: number, dz: number) => boolean): boolean {
  return ENCLOSURE.every(([dx, dy, dz]) => solid(dx, dy, dz));
}

// Is the agent walled in so mobs cannot reach it (spec §2 `sheltered`)?
// The glue answers "is the block at this offset from my feet solid?".

/** The smallest shelter, a 1x1 hole: offsets (dx, dy, dz) from the feet, all solid. */
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

/** How far a room may reach from the feet: a 3x3 room from any spot in it. */
export const ROOM_REACH = 2;

/**
 * Whether the agent is in a closed room: every column a mob could stand
 * in that can be reached from the feet lies within `ROOM_REACH` of them
 * and has a roof two above the feet. A column is closed off when it is
 * solid at feet and head height. A 1x1 hole is the `ENCLOSURE` case; a
 * 3x3 home (Michael, 15 Nov 2026) counts too.
 */
export function enclosed(solid: (dx: number, dy: number, dz: number) => boolean): boolean {
  const seen = new Set<string>();
  const todo: Array<[number, number]> = [[0, 0]];
  for (let next = todo.pop(); next !== undefined; next = todo.pop()) {
    const [x, z] = next;
    if (seen.has(`${x},${z}`)) {
      continue;
    }
    seen.add(`${x},${z}`);
    if (!solid(x, 2, z)) {
      return false;
    }
    const sides: Array<[number, number]> = [
      [x + 1, z],
      [x - 1, z],
      [x, z + 1],
      [x, z - 1],
    ];
    for (const [nx, nz] of sides) {
      if (solid(nx, 0, nz) && solid(nx, 1, nz)) {
        continue;
      }
      if (Math.max(Math.abs(nx), Math.abs(nz)) > ROOM_REACH) {
        return false;
      }
      todo.push([nx, nz]);
    }
  }
  return true;
}

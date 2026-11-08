// Small geometry shared by the bot's modules.

export interface Vec3Like {
  x: number;
  y: number;
  z: number;
}

/** Straight-line distance in blocks. */
export function distance(a: Vec3Like, b: Vec3Like): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

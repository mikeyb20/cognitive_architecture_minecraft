// Keep the home shut (Michael, 15 Nov 2026: the bot goes in and out of
// its home by the door). The pathfinder opens the door to pass; this
// says when the bot should close it again.

/** How far the bot can reach to close a door, in blocks. */
export const DOOR_REACH = 4.5;
/** How far past the door a moving bot is before it closes it behind it. */
export const DOOR_PASSED = 1.8;

/** What the glue sees of the home's door each tick. */
export interface DoorView {
  open: boolean;
  /** From the bot to the door's lower block, in blocks. */
  distance: number;
  /** The bot stands in the doorway itself. */
  inDoorway: boolean;
  /** The pathfinder is walking the bot somewhere. */
  moving: boolean;
}

/**
 * Close the open door when the bot is within reach and out of the
 * doorway: at once when standing still (it has come in, or is about to
 * go out by itself), and once it is past the door when walking (it has
 * just gone out, or is still on its way in).
 */
export function shouldClose(d: DoorView): boolean {
  return d.open && !d.inDoorway && d.distance <= DOOR_REACH && (!d.moving || d.distance > DOOR_PASSED);
}

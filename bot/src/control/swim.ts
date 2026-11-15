// Keep the head out of water (drowning was 5 of 24 deaths in the first live
// nights, 14–15 Nov 2026, several while idle in a flooded hole). A body
// reflex below any decision, like a player holding jump in water: while the
// bot is in a liquid it holds jump, so it floats up; on leaving it lets go
// once and otherwise leaves the jump control to whatever skill is running.

export class SwimUp {
  #holding = false;

  /** The jump control to set this tick, or null to leave it alone. */
  tick(inLiquid: boolean): boolean | null {
    if (inLiquid) {
      this.#holding = true;
      return true;
    }
    if (this.#holding) {
      this.#holding = false;
      return false;
    }
    return null;
  }
}

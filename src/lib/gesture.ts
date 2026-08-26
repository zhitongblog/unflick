/**
 * Right-drag gesture recognition.
 *
 * Press the right button, drag, release: the stroke's dominant direction
 * picks a trigger. The hard part isn't the geometry, it's coexisting with
 * the context menu — a right-click that happens to wobble two pixels must
 * still open the menu, and a deliberate stroke must not.
 *
 * The rule: a gesture is only recognised past `MIN_DISTANCE`, and when one
 * is recognised the caller suppresses the menu for that release.
 */

export type GestureDirection = "left" | "right" | "up" | "down";

/**
 * Minimum travel, in CSS pixels, before a right-drag counts as a gesture
 * rather than a click that moved. Generous enough that a shaky hand or a
 * trackpad tap can't trigger one by accident.
 */
const MIN_DISTANCE = 45;

/**
 * How much longer the dominant axis must be than the other. Without this a
 * 45° drag would resolve to whichever axis won by a pixel, which feels
 * random; requiring a clear lead makes an ambiguous stroke do nothing at
 * all, which is easier to understand than the wrong thing happening.
 */
const AXIS_DOMINANCE = 1.5;

export interface GestureTracker {
  /** Call on right-button mousedown. */
  begin(x: number, y: number): void;
  /**
   * Call on right-button mouseup. Returns the direction if the stroke was
   * a gesture, or null if it was a plain right-click (menu should open).
   */
  end(x: number, y: number): GestureDirection | null;
  /** True between a begin() and its end(). */
  active(): boolean;
  cancel(): void;
}

export function createGestureTracker(): GestureTracker {
  let origin: { x: number; y: number } | null = null;

  return {
    begin(x, y) {
      origin = { x, y };
    },
    active() {
      return origin !== null;
    },
    cancel() {
      origin = null;
    },
    end(x, y) {
      if (!origin) return null;
      const dx = x - origin.x;
      const dy = y - origin.y;
      origin = null;

      if (Math.hypot(dx, dy) < MIN_DISTANCE) return null;

      const ax = Math.abs(dx);
      const ay = Math.abs(dy);
      if (ax > ay * AXIS_DOMINANCE) return dx > 0 ? "right" : "left";
      if (ay > ax * AXIS_DOMINANCE) return dy > 0 ? "down" : "up";
      // Diagonal: deliberately nothing.
      return null;
    },
  };
}

/**
 * Wheel accumulator.
 *
 * A mouse wheel sends one chunky event per notch; a trackpad sends a
 * stream of small ones. Acting on every event makes a trackpad flick jump
 * the volume twenty steps. Accumulating deltas and emitting one step per
 * threshold gives both devices the same feel.
 */
export function createWheelAccumulator(threshold = 40) {
  let total = 0;

  return {
    /** Returns how many steps to apply, and their sign. */
    push(deltaY: number): { steps: number; direction: "up" | "down" } {
      // A direction change should respond immediately rather than having
      // to first unwind the momentum built up the other way.
      if ((total > 0 && deltaY < 0) || (total < 0 && deltaY > 0)) total = 0;

      total += deltaY;
      const steps = Math.trunc(Math.abs(total) / threshold);
      if (steps > 0) total -= Math.sign(total) * steps * threshold;

      // deltaY is positive when scrolling *down* the page.
      return { steps, direction: deltaY > 0 ? "down" : "up" };
    },
    reset() {
      total = 0;
    },
  };
}

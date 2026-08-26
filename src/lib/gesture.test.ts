import { describe, it, expect } from "vitest";
import { createGestureTracker, createWheelAccumulator } from "./gesture";

/**
 * Gesture recognition is pure geometry, and every one of these thresholds
 * is a judgement call about feel. Testing them means a later tweak has to
 * be deliberate rather than accidental.
 */

describe("gesture tracker", () => {
  it("recognises the four directions", () => {
    const cases = [
      { to: [200, 0], want: "right" },
      { to: [-200, 0], want: "left" },
      { to: [0, 200], want: "down" },
      { to: [0, -200], want: "up" },
    ] as const;

    for (const { to, want } of cases) {
      const g = createGestureTracker();
      g.begin(500, 500);
      expect(g.end(500 + to[0], 500 + to[1])).toBe(want);
    }
  });

  it("ignores a right-click that barely moved", () => {
    // The whole point: a plain right-click must still open the context
    // menu, and hands are not steady.
    const g = createGestureTracker();
    g.begin(500, 500);
    expect(g.end(503, 498)).toBeNull();
  });

  it("ignores a diagonal stroke rather than guessing", () => {
    // A 45° drag resolving to whichever axis won by a pixel feels random.
    const g = createGestureTracker();
    g.begin(500, 500);
    expect(g.end(640, 640)).toBeNull();
  });

  it("needs a clear axis lead, not just a longer one", () => {
    const g = createGestureTracker();
    g.begin(500, 500);
    // 100 across, 90 down: longer horizontally, but not decisively.
    expect(g.end(600, 590)).toBeNull();

    const g2 = createGestureTracker();
    g2.begin(500, 500);
    // 200 across, 50 down: unambiguous.
    expect(g2.end(700, 550)).toBe("right");
  });

  it("reports whether a stroke is in progress", () => {
    const g = createGestureTracker();
    expect(g.active()).toBe(false);
    g.begin(0, 0);
    expect(g.active()).toBe(true);
    g.end(200, 0);
    expect(g.active()).toBe(false);
  });

  it("produces nothing after a cancel", () => {
    // Releasing outside the video area cancels; the next stroke must not
    // inherit the abandoned origin.
    const g = createGestureTracker();
    g.begin(0, 0);
    g.cancel();
    expect(g.end(500, 0)).toBeNull();
  });

  it("produces nothing from an end with no begin", () => {
    const g = createGestureTracker();
    expect(g.end(500, 500)).toBeNull();
  });
});

describe("wheel accumulator", () => {
  it("emits one step per notch of a real mouse wheel", () => {
    const w = createWheelAccumulator(40);
    // Windows sends 120 per notch; at a threshold of 40 that's 3 steps.
    expect(w.push(-120)).toEqual({ steps: 3, direction: "up" });
  });

  it("holds back trackpad dust until it adds up", () => {
    const w = createWheelAccumulator(40);
    expect(w.push(-10).steps).toBe(0);
    expect(w.push(-10).steps).toBe(0);
    expect(w.push(-10).steps).toBe(0);
    // Fourth small delta crosses the threshold.
    expect(w.push(-10).steps).toBe(1);
  });

  it("keeps the remainder instead of dropping it", () => {
    const w = createWheelAccumulator(40);
    expect(w.push(-50).steps).toBe(1); // 10 left over
    expect(w.push(-30).steps).toBe(1); // 10 + 30 = 40
  });

  it("responds immediately when the direction reverses", () => {
    // Without clearing on reversal, scrolling back would first have to
    // unwind the momentum built up the other way.
    const w = createWheelAccumulator(40);
    w.push(-120);
    expect(w.push(40)).toEqual({ steps: 1, direction: "down" });
  });

  it("reports direction from the event, not the accumulated total", () => {
    const w = createWheelAccumulator(40);
    expect(w.push(120).direction).toBe("down");
    expect(w.push(-120).direction).toBe("up");
  });

  it("starts clean after a reset", () => {
    const w = createWheelAccumulator(40);
    w.push(-30);
    w.reset();
    expect(w.push(-30).steps).toBe(0);
  });
});

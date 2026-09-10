import { describe, it, expect } from "vitest";
import { shouldShowOnboarding, type OnboardingGate } from "./onboarding";

/** A fresh install sitting on the idle screen. */
const fresh: OnboardingGate = {
  settingsLoaded: true,
  onboardingSeen: false,
  dismissedThisSession: false,
  playerState: "stopped",
  modalOpen: false,
};

describe("shouldShowOnboarding", () => {
  it("shows on a fresh idle launch", () => {
    expect(shouldShowOnboarding(fresh)).toBe(true);
  });

  it("stays hidden until settings have been read", () => {
    // `onboardingSeen` defaults to false, so without this gate the card
    // renders for one frame on a machine that dismissed it long ago.
    expect(shouldShowOnboarding({ ...fresh, settingsLoaded: false })).toBe(false);
  });

  it("stays hidden once the flag is persisted", () => {
    expect(shouldShowOnboarding({ ...fresh, onboardingSeen: true })).toBe(false);
  });

  it("stays hidden over a file opened by association", () => {
    expect(shouldShowOnboarding({ ...fresh, playerState: "playing" })).toBe(false);
    expect(shouldShowOnboarding({ ...fresh, playerState: "paused" })).toBe(false);
  });

  it("does not come back after a dismissal whose write failed", () => {
    expect(
      shouldShowOnboarding({
        ...fresh,
        dismissedThisSession: true,
        onboardingSeen: false,
      }),
    ).toBe(false);
  });

  it("never stacks on top of another modal", () => {
    expect(shouldShowOnboarding({ ...fresh, modalOpen: true })).toBe(false);
  });
});

/**
 * When the welcome screen is allowed on screen.
 *
 * Pulled out of the component because every clause here is a judgement that
 * is wrong in a way you only notice in the wild — a card that flashes for
 * one frame on a machine that dismissed it a year ago, or one that covers a
 * film the user just double-clicked in Finder. Those are cheap to assert and
 * expensive to discover.
 */
export interface OnboardingGate {
  /** settings.json has been read. Until it has, `onboardingSeen` is a guess. */
  settingsLoaded: boolean;
  /** The persisted `onboarding_seen` flag. */
  onboardingSeen: boolean;
  /** Dismissed in this run. Survives a failed write to settings.json. */
  dismissedThisSession: boolean;
  playerState: "playing" | "paused" | "stopped";
  /** Settings / URL dialog / clip dialog — anything that owns the screen. */
  modalOpen: boolean;
}

export function shouldShowOnboarding(gate: OnboardingGate): boolean {
  // The store's default for `onboardingSeen` is false, so without this the
  // card renders for one frame on every launch before settings arrive.
  if (!gate.settingsLoaded) return false;
  if (gate.onboardingSeen) return false;
  // The persisted write can fail (read-only config dir, disk full). The
  // session flag is what guarantees the card cannot come back mid-run.
  if (gate.dismissedThisSession) return false;
  // Launched by double-clicking a file: the video is the welcome.
  if (gate.playerState !== "stopped") return false;
  if (gate.modalOpen) return false;
  return true;
}

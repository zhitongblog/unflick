/**
 * Shared time formatting. Both the progress bar and the clip dialog grew
 * their own copies of this; new callers use these instead.
 */

/** `h:mm:ss` past an hour, `m:ss` below it. Negative or non-finite → `0:00`. */
export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
}

/**
 * A signed offset in seconds, e.g. `+0.30s` / `-1.20s`. Always shows the
 * sign — for a delay control, "0.30s" alone doesn't say which direction it
 * moved.
 */
export function formatDelay(seconds: number): string {
  const value = Number.isFinite(seconds) ? seconds : 0;
  const sign = value < 0 ? "-" : "+";
  return `${sign}${Math.abs(value).toFixed(2)}s`;
}

/**
 * A playback rate, e.g. `1×`, `1.5×`, `1.35×`. Rounded to hundredths: repeated
 * relative nudges accumulate float noise, and `1.3000000000000003×` on a
 * button is worse than being a thousandth off.
 */
export function formatSpeed(rate: number): string {
  const value = Number.isFinite(rate) ? rate : 1;
  return `${parseFloat(value.toFixed(2))}×`;
}

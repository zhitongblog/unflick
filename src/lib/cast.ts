/**
 * Casting, as far as the window needs to understand it.
 *
 * Everything that talks to a television lives in Rust — discovery, the SOAP
 * calls, the HTTP server that feeds it — and the panel reaches all of it
 * through one `cast` command. What is left over is the handful of decisions
 * a list and a progress bar need, and those are here rather than inside the
 * component so they can be tested without a DOM: which of four things the
 * panel is currently showing, and how a click on a bar becomes a position.
 */

/** A television, exactly as `core::dlna::Renderer` serialises it. */
export interface Renderer {
  name: string;
  id: string;
  control_url: string;
  address: string;
}

/** A cast in progress, as the `status` action reports it. */
export interface CastSession {
  renderer: Renderer;
  file: string;
  url: string;
  position: number;
  duration: number;
  /** The renderer's own word: `PLAYING`, `PAUSED_PLAYBACK`, `STOPPED`, … */
  state: string;
}

/** What the panel is showing. */
export type CastView = "casting" | "searching" | "empty" | "renderers";

export interface CastSnapshot {
  discovering: boolean;
  /** True once a discovery has finished — an empty list means nothing until then. */
  searched: boolean;
  renderers: Renderer[];
  session: CastSession | null;
}

/**
 * Which of the four the panel is in.
 *
 * The order matters in one place in particular: `searched` gates the empty
 * state, so the "no televisions found" message can only appear after a
 * search actually came back with nothing. Before the first search has even
 * started — the instant between mounting and the `discovering` flag being
 * set — this reports `searching`, because that is what is about to be true
 * and an empty-state flash would be a lie about a search nobody ran.
 */
export function castView(s: CastSnapshot): CastView {
  if (s.session) return "casting";
  if (s.discovering) return "searching";
  if (s.renderers.length > 0) return "renderers";
  if (s.searched) return "empty";
  return "searching";
}

/** The renderer list out of a `cast list` reply. Anything unexpected is empty. */
export function parseRenderers(data: unknown): Renderer[] {
  if (!Array.isArray(data)) return [];
  return data.filter((r): r is Renderer => {
    if (!r || typeof r !== "object") return false;
    const c = r as Partial<Renderer>;
    return typeof c.name === "string" && typeof c.id === "string";
  });
}

/**
 * The session out of a `cast status` reply, or null for "not casting".
 *
 * `cast status` answers with `data: null` when there is no cast, which is a
 * success and not an error — so the null has to survive the trip rather
 * than being mistaken for a parse failure.
 */
export function parseSession(data: unknown): CastSession | null {
  if (!data || typeof data !== "object") return null;
  const c = data as Partial<CastSession>;
  if (!c.renderer || typeof c.renderer !== "object") return null;
  if (typeof c.renderer.name !== "string") return null;
  return {
    renderer: c.renderer as Renderer,
    file: typeof c.file === "string" ? c.file : "",
    url: typeof c.url === "string" ? c.url : "",
    position: typeof c.position === "number" ? c.position : 0,
    duration: typeof c.duration === "number" ? c.duration : 0,
    state: typeof c.state === "string" ? c.state : "UNKNOWN",
  };
}

/**
 * Whether the television is stopped rather than playing.
 *
 * AVTransport's vocabulary, not ours: `PAUSED_PLAYBACK` is the pause a
 * renderer reports, and `PAUSED_RECORDING` exists too. Anything we don't
 * recognise counts as playing, so the button offers to pause rather than
 * offering to resume something that is already running.
 */
export function isPaused(state: string): boolean {
  return state.toUpperCase().startsWith("PAUSED");
}

/** Fraction of the way through, clamped to 0..1. Zero when there is no duration. */
export function castProgress(session: Pick<CastSession, "position" | "duration">): number {
  if (!(session.duration > 0)) return 0;
  const ratio = session.position / session.duration;
  if (!Number.isFinite(ratio)) return 0;
  return Math.min(1, Math.max(0, ratio));
}

/**
 * Where a click at `ratio` along the bar lands, in seconds.
 *
 * Rounded to whole seconds because that is what AVTransport's `hms` format
 * carries anyway — sending 93.5 and being seeked to 93 looks like a bug in
 * the bar, and is not one.
 */
export function seekTarget(ratio: number, duration: number): number {
  if (!(duration > 0)) return 0;
  const clamped = Math.min(1, Math.max(0, Number.isFinite(ratio) ? ratio : 0));
  return Math.round(clamped * duration);
}

/** Last path component, extension dropped — what to call the file on screen. */
export function castFileLabel(file: string): string {
  const parts = file.replace(/\\/g, "/").split("/");
  const name = parts[parts.length - 1] || file;
  return name.replace(/\.[^/.]+$/, "");
}

/** Whether two renderers are the same box. Identity, never the display name. */
export function sameRenderer(a: Renderer | null, b: Renderer | null): boolean {
  if (!a || !b) return false;
  return a.id === b.id;
}

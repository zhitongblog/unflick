import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "./playerStore";
import {
  parseRenderers,
  parseSession,
  type CastSession,
  type Renderer,
} from "../lib/cast";

/**
 * The casting panel's state.
 *
 * Every action here is one `cast` invoke, and that command forwards to the
 * same `core::daemon` arm `unflick cast …` reaches through the control
 * socket. There is no discovery, no session and no renderer list on this
 * side that Rust does not already own — the store holds the *answers*, never
 * a second copy of the thing being answered about.
 *
 * Which is why `refresh()` exists and why the panel calls it on every open:
 * a cast can be started, paused or stopped from the CLI or by an AI agent
 * between one opening of this panel and the next, and a list drawn from what
 * was true last time would be describing a television nobody is watching.
 */

/** What a `cast` invoke hands back: the CLI's two fields, unchanged. */
interface CastReply {
  message: string;
  data: unknown;
}

interface CastState {
  /** Renderers from the last completed discovery. */
  renderers: Renderer[];
  /** A discovery is in flight. */
  discovering: boolean;
  /** A discovery has completed at least once since the panel was opened. */
  searched: boolean;
  /** The live cast, or null. Mirrors `unflick cast status`. */
  session: CastSession | null;
  /** Which renderer we are in the middle of handing the film to. */
  connecting: Renderer | null;
  /** A control action (pause / resume / stop / seek) is in flight. */
  busy: boolean;
  /** The last backend message that came back as a failure. English, from Rust. */
  error: string | null;

  /** Forget the last search. */
  reset: () => void;
  /**
   * What opening the panel does: forget the last search, read the live
   * state, and — unless something is already casting — look again.
   *
   * Deliberately not the panel's mount effect. `AnimatePresence` reverses
   * an exit rather than remounting when a popover is reopened before its
   * 120 ms close has finished, so a component that reads state on mount
   * reads it once and then shows whatever was true the first time. The
   * open is an event; this is it.
   */
  open: () => Promise<void>;
  /** Re-read `cast status`. Cheap when nothing is casting — no network at all. */
  refresh: () => Promise<void>;
  /** Search every interface for televisions. Takes seconds by design. */
  discover: () => Promise<void>;
  /** Hand the file on screen to a television. */
  castTo: (renderer: Renderer) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  /** Stop the television and pick playback back up in this window. */
  stop: () => Promise<void>;
  seek: (seconds: number) => Promise<void>;
}

/**
 * How long to listen for SSDP replies.
 *
 * Not latency to be minimised: renderers deliberately spread their answers
 * across the window so a room full of devices does not reply at once, so a
 * shorter search simply misses the slower television. The backend clamps
 * this to 1..15 anyway.
 */
const DISCOVER_SECONDS = 4;

async function castCommand(
  action: string,
  extra: Record<string, unknown> = {},
): Promise<CastReply> {
  return await invoke<CastReply>("cast", { action, ...extra });
}

export const useCastStore = create<CastState>((set, get) => ({
  renderers: [],
  discovering: false,
  searched: false,
  session: null,
  connecting: null,
  busy: false,
  error: null,

  reset: () =>
    set({
      renderers: [],
      searched: false,
      discovering: false,
      connecting: null,
      busy: false,
      error: null,
    }),

  open: async () => {
    get().reset();
    await get().refresh();
    if (!get().session) await get().discover();
  },

  refresh: async () => {
    try {
      const reply = await castCommand("status");
      set({ session: parseSession(reply.data) });
    } catch (e) {
      // A renderer that has been switched off stops answering
      // GetPositionInfo, and `cast status` reports that as a failure. The
      // session is still held on the Rust side — only `stop` releases it —
      // so keep showing it and say what went wrong.
      set({ error: String(e) });
    }
  },

  discover: async () => {
    set({ discovering: true, renderers: [], error: null });
    try {
      const reply = await castCommand("list", { seconds: DISCOVER_SECONDS });
      set({ renderers: parseRenderers(reply.data), searched: true });
    } catch (e) {
      set({ error: String(e), searched: true });
    } finally {
      set({ discovering: false });
    }
  },

  castTo: async (renderer) => {
    set({ connecting: renderer, error: null });
    try {
      // Identity, not the display name: `pick_renderer` matches the id
      // exactly and only falls back to a substring of the name, which two
      // televisions of the same model would both answer to.
      await castCommand("to", { renderer: renderer.id });
      await get().refresh();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ connecting: null });
    }
  },

  pause: async () => {
    set({ busy: true, error: null });
    try {
      await castCommand("pause");
      await get().refresh();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  resume: async () => {
    set({ busy: true, error: null });
    try {
      await castCommand("resume");
      await get().refresh();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  stop: async () => {
    set({ busy: true, error: null });
    try {
      await castCommand("stop");
      set({ session: null });
      // Coming back. Starting a cast pauses the local player rather than
      // closing the file, precisely so there is something to come back to —
      // this is the other half of that, and without it stopping a cast
      // leaves a frozen frame and no clue that the film is still loaded.
      await usePlayerStore.getState().resume();
    } catch (e) {
      set({ error: String(e) });
      await get().refresh();
    } finally {
      set({ busy: false });
    }
  },

  seek: async (seconds) => {
    set({ busy: true, error: null });
    try {
      await castCommand("seek", { seconds });
      await get().refresh();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },
}));

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/**
 * Keyboard bindings, loaded from the Rust action catalogue.
 *
 * The frontend deliberately keeps no copy of the defaults: `core::keybind`
 * owns them, so `unflick keybind list`, the settings panel, and the key
 * handler can't disagree about what a fresh install does.
 */

export interface Binding {
  id: string;
  label: string;
  group: string;
  /** Currently in effect. */
  key: string;
  default: string;
  customized: boolean;
}

interface KeybindState {
  bindings: Binding[];
  /** Chord → action id. Rebuilt whenever bindings change. */
  lookup: Map<string, string>;
  loaded: boolean;
  error: string | null;

  load: () => Promise<void>;
  /** Bind a key. Returns null on success, or a message explaining the refusal. */
  setBinding: (action: string, key: string) => Promise<string | null>;
  /** Reset one action, or every action when `action` is omitted. */
  reset: (action?: string) => Promise<void>;
  /** Which action a chord triggers, if any. */
  actionFor: (chord: string) => string | undefined;
}

function buildLookup(bindings: Binding[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const b of bindings) {
    // The backend rejects duplicates, so a collision here would mean a
    // hand-edited settings.json. First one wins and the rest are inert —
    // same as the backend's own precedence.
    if (!map.has(b.key)) map.set(b.key, b.id);
  }
  return map;
}

export const useKeybindStore = create<KeybindState>((set, get) => ({
  bindings: [],
  lookup: new Map(),
  loaded: false,
  error: null,

  load: async () => {
    try {
      const bindings = await invoke<Binding[]>("keybind_list");
      set({ bindings, lookup: buildLookup(bindings), loaded: true, error: null });
    } catch (e) {
      // Leaving `loaded` false keeps the key handler inert rather than
      // firing the wrong actions from a half-built table.
      console.error("keybind_list failed:", e);
      set({ error: String(e), loaded: false });
    }
  },

  setBinding: async (action: string, key: string) => {
    try {
      await invoke("keybind_set", { action, key });
      await get().load();
      return null;
    } catch (e) {
      // The backend refuses a key that's already taken and says which
      // action holds it — surface that verbatim, it's the useful part.
      return typeof e === "string" ? e : String(e);
    }
  },

  reset: async (action?: string) => {
    try {
      await invoke("keybind_reset", { action: action ?? null });
      await get().load();
    } catch (e) {
      console.error("keybind_reset failed:", e);
    }
  },

  actionFor: (chord: string) => get().lookup.get(chord),
}));

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

/** Why a rebind did not happen, in a form the window can translate. */
export type BindFailure =
  | { kind: "conflict"; key: string; actionId: string; label: string }
  | { kind: "rejected"; detail: string };

interface KeybindState {
  bindings: Binding[];
  /** Chord → action id. Rebuilt whenever bindings change. */
  lookup: Map<string, string>;
  loaded: boolean;
  error: string | null;

  load: () => Promise<void>;
  /** Bind a key. Returns null on success, or a message explaining the refusal. */
  setBinding: (action: string, key: string) => Promise<BindFailure | null>;
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
    // Answered here rather than by the backend's refusal. The backend
    // composes its sentence in Rust — including the action's English label —
    // so a Chinese interface showed an English refusal, and there was no key
    // to translate because the string never passed through i18n. The table
    // this store already holds is the same one the backend checks, so the
    // conflict can be reported as data and the sentence written where every
    // other sentence is written. The backend still refuses; this only stops
    // the refusal from being the thing the user reads.
    const taken = get().bindings.find((b) => b.key === key && b.id !== action);
    if (taken) {
      return { kind: "conflict", key, actionId: taken.id, label: taken.label };
    }
    try {
      await invoke("keybind_set", { action, key });
      await get().load();
      return null;
    } catch (e) {
      // Anything else — an unknown action, a key the normaliser rejects —
      // is a bug or a bad call rather than something a user chose, so the
      // backend's own words are the useful ones.
      return { kind: "rejected", detail: typeof e === "string" ? e : String(e) };
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

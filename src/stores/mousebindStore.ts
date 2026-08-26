import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/**
 * Mouse bindings, loaded from the Rust trigger catalogue.
 *
 * Mirrors `keybindStore`: the backend owns the triggers, their defaults,
 * and which action each runs, so the CLI, the settings panel and the video
 * area can't disagree.
 */

export interface MouseBinding {
  id: string;
  label: string;
  /** Action id, or "none" when the trigger is disabled. */
  action: string;
  action_label: string;
  default: string;
  customized: boolean;
}

interface MousebindState {
  bindings: MouseBinding[];
  /** Trigger id → action id. Excludes disabled triggers. */
  lookup: Map<string, string>;
  loaded: boolean;

  load: () => Promise<void>;
  setBinding: (trigger: string, action: string) => Promise<string | null>;
  reset: (trigger?: string) => Promise<void>;
  /** Action a trigger runs, or undefined when it's unbound. */
  actionFor: (trigger: string) => string | undefined;
}

function buildLookup(bindings: MouseBinding[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const b of bindings) {
    if (b.action && b.action !== "none") map.set(b.id, b.action);
  }
  return map;
}

export const useMousebindStore = create<MousebindState>((set, get) => ({
  bindings: [],
  lookup: new Map(),
  loaded: false,

  load: async () => {
    try {
      const bindings = await invoke<MouseBinding[]>("mouse_list");
      set({ bindings, lookup: buildLookup(bindings), loaded: true });
    } catch (e) {
      console.error("mouse_list failed:", e);
      set({ loaded: false });
    }
  },

  setBinding: async (trigger: string, action: string) => {
    try {
      await invoke("mouse_set", { trigger, action });
      await get().load();
      return null;
    } catch (e) {
      return typeof e === "string" ? e : String(e);
    }
  },

  reset: async (trigger?: string) => {
    try {
      await invoke("mouse_reset", { trigger: trigger ?? null });
      await get().load();
    } catch (e) {
      console.error("mouse_reset failed:", e);
    }
  },

  actionFor: (trigger: string) => get().lookup.get(trigger),
}));

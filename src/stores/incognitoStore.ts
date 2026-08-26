import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/**
 * Session-only "incognito" toggle. Intentionally NOT persisted in
 * settings.json — turning it on lasts only for this app run, matching
 * the mental model of private browsing in Chrome/Firefox.
 *
 * When active, record_play, save_position and similar history-leaving
 * calls are skipped at their call sites in playerStore.
 *
 * The flag is also pushed to the backend. It used to live only here, which
 * was safe while the CLI drove a separate process — but since v0.10 the
 * window hosts the control server, so `unflick play` and MCP `play` reach
 * this very player and would otherwise write history the user had just
 * switched off.
 */
interface IncognitoState {
  enabled: boolean;
  toggle: () => void;
  set: (v: boolean) => void;
}

function push(enabled: boolean) {
  invoke("set_incognito", { enabled }).catch(() => {
    // Pre-init the command may not be ready. The next toggle re-sends,
    // and the backend defaults to off, which is the safe direction for
    // a failure — history keeps working rather than silently stopping.
  });
}

export const useIncognitoStore = create<IncognitoState>((set) => ({
  enabled: false,
  toggle: () =>
    set((s) => {
      const enabled = !s.enabled;
      push(enabled);
      return { enabled };
    }),
  set: (v) => {
    push(v);
    return set({ enabled: v });
  },
}));

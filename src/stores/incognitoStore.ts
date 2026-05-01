import { create } from "zustand";

/**
 * Session-only "incognito" toggle. Intentionally NOT persisted in
 * settings.json — turning it on lasts only for this app run, matching
 * the mental model of private browsing in Chrome/Firefox.
 *
 * When active, record_play, save_position and similar history-leaving
 * calls are skipped at their call sites in playerStore.
 */
interface IncognitoState {
  enabled: boolean;
  toggle: () => void;
  set: (v: boolean) => void;
}

export const useIncognitoStore = create<IncognitoState>((set) => ({
  enabled: false,
  toggle: () => set((s) => ({ enabled: !s.enabled })),
  set: (v) => set({ enabled: v }),
}));

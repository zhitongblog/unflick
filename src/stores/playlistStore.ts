import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { getStrings } from "../i18n/utils";

export interface PlaylistItem {
  index: number;
  path: string;
  title: string;
  current: boolean;
}

export type RepeatMode = "off" | "one" | "all";

interface PlaylistState {
  items: PlaylistItem[];
  currentIndex: number;
  showPlaylist: boolean;
  isLoading: boolean;
  /** What happens when a file ends. Auto-advance is driven in Rust. */
  repeat: RepeatMode;
  shuffle: boolean;
  fetchPlaylist: () => Promise<void>;
  /** Read repeat + shuffle back from the backend, which owns the truth. */
  fetchModes: () => Promise<void>;
  cycleRepeat: () => Promise<void>;
  toggleShuffle: () => Promise<void>;
  add: (path: string) => Promise<void>;
  remove: (index: number) => Promise<void>;
  next: () => Promise<void>;
  prev: () => Promise<void>;
  clear: () => Promise<void>;
  togglePlaylist: () => void;
  playAt: (index: number) => Promise<void>;
}

/**
 * Report a failure the user can act on. Playback failures used to be
 * console-only, which was survivable while `play` could not fail — now that
 * a dead share or a moved file comes back as an error, silence would look
 * like the button doing nothing.
 *
 * The fallback is a finished, translated sentence rather than a log prefix.
 * It used to be one of the "Failed to skip to next:" strings below, so a
 * rejection that carried no message rendered a toast ending in a colon and
 * saying nothing at all.
 */
function reportError(fallback: string, e: unknown) {
  const message = typeof e === "string" ? e : e instanceof Error ? e.message : fallback;
  console.error(fallback, e);
  window.dispatchEvent(
    new CustomEvent("unflick:toast", { detail: { kind: "error", message } }),
  );
}

export const usePlaylistStore = create<PlaylistState>((set, get) => ({
  items: [],
  currentIndex: -1,
  showPlaylist: false,
  isLoading: false,
  repeat: "off",
  shuffle: false,

  togglePlaylist: () => set((s) => ({ showPlaylist: !s.showPlaylist })),

  fetchModes: async () => {
    try {
      const [r, s] = await Promise.all([
        invoke<{ mode: RepeatMode }>("playlist_repeat", {}),
        invoke<{ enabled: boolean }>("playlist_shuffle", {}),
      ]);
      set({ repeat: r.mode, shuffle: s.enabled });
    } catch (e) {
      console.error("Failed to read playlist modes:", e);
    }
  },

  cycleRepeat: async () => {
    const order: RepeatMode[] = ["off", "all", "one"];
    const next = order[(order.indexOf(get().repeat) + 1) % order.length];
    try {
      const res = await invoke<{ mode: RepeatMode }>("playlist_repeat", { mode: next });
      set({ repeat: res.mode });
    } catch (e) {
      console.error("Failed to set repeat mode:", e);
    }
  },

  toggleShuffle: async () => {
    try {
      const res = await invoke<{ enabled: boolean }>("playlist_shuffle", {
        enabled: !get().shuffle,
      });
      set({ shuffle: res.enabled });
    } catch (e) {
      console.error("Failed to toggle shuffle:", e);
    }
  },

  fetchPlaylist: async () => {
    set({ isLoading: true });
    try {
      const items = await invoke<PlaylistItem[]>("playlist_list");
      const current = items.find((i) => i.current);
      set({
        items,
        currentIndex: current ? current.index : -1,
        isLoading: false,
      });
    } catch (e) {
      console.error("Failed to fetch playlist:", e);
      set({ isLoading: false });
    }
  },

  add: async (path: string) => {
    try {
      await invoke("playlist_add", { path });
      await get().fetchPlaylist();
    } catch (e) {
      // Adding a file that has since moved used to look like a dead button.
      reportError(getStrings().playlist.addFailed, e);
    }
  },

  remove: async (index: number) => {
    try {
      await invoke("playlist_remove", { index });
      await get().fetchPlaylist();
    } catch (e) {
      reportError(getStrings().playlist.removeFailed, e);
    }
  },

  next: async () => {
    try {
      await invoke("playlist_next");
      await get().fetchPlaylist();
    } catch (e) {
      reportError(getStrings().playlist.nextFailed, e);
    }
  },

  prev: async () => {
    try {
      await invoke("playlist_prev");
      await get().fetchPlaylist();
    } catch (e) {
      reportError(getStrings().playlist.prevFailed, e);
    }
  },

  clear: async () => {
    try {
      await invoke("playlist_clear");
      set({ items: [], currentIndex: -1 });
    } catch (e) {
      console.error("Failed to clear playlist:", e);
    }
  },

  playAt: async (index: number) => {
    try {
      await invoke("playlist_play_index", { index });
      await get().fetchPlaylist();
    } catch (e) {
      reportError(getStrings().playlist.playFailed, e);
    }
  },
}));

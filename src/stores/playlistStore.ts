import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface PlaylistItem {
  index: number;
  path: string;
  title: string;
  current: boolean;
}

interface PlaylistState {
  items: PlaylistItem[];
  currentIndex: number;
  showPlaylist: boolean;
  isLoading: boolean;
  fetchPlaylist: () => Promise<void>;
  add: (path: string) => Promise<void>;
  remove: (index: number) => Promise<void>;
  next: () => Promise<void>;
  prev: () => Promise<void>;
  clear: () => Promise<void>;
  togglePlaylist: () => void;
  playAt: (index: number) => Promise<void>;
}

export const usePlaylistStore = create<PlaylistState>((set, get) => ({
  items: [],
  currentIndex: -1,
  showPlaylist: false,
  isLoading: false,

  togglePlaylist: () => set((s) => ({ showPlaylist: !s.showPlaylist })),

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
      console.error("Failed to add to playlist:", e);
    }
  },

  remove: async (index: number) => {
    try {
      await invoke("playlist_remove", { index });
      await get().fetchPlaylist();
    } catch (e) {
      console.error("Failed to remove from playlist:", e);
    }
  },

  next: async () => {
    try {
      await invoke("playlist_next");
      await get().fetchPlaylist();
    } catch (e) {
      console.error("Failed to skip to next:", e);
    }
  },

  prev: async () => {
    try {
      await invoke("playlist_prev");
      await get().fetchPlaylist();
    } catch (e) {
      console.error("Failed to skip to prev:", e);
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
      console.error("Failed to play at index:", e);
    }
  },
}));

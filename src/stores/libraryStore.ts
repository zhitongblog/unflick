import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface MediaEntry {
  id: number;
  path: string;
  title: string;
  duration: number | null;
  width: number | null;
  height: number | null;
  video_codec: string | null;
  audio_codec: string | null;
  file_size: number | null;
  added_at: string;
  last_played: string | null;
  play_count: number;
}

interface LibraryState {
  entries: MediaEntry[];
  searchQuery: string;
  isLoading: boolean;
  showLibrary: boolean;
  setSearchQuery: (query: string) => void;
  toggleLibrary: () => void;
  fetchLibrary: () => Promise<void>;
  search: (query: string) => Promise<void>;
  scanDirectory: (dir: string) => Promise<void>;
  clearLibrary: () => Promise<number>;
}

export const useLibraryStore = create<LibraryState>((set) => ({
  entries: [],
  searchQuery: "",
  isLoading: false,
  showLibrary: false,

  setSearchQuery: (query: string) => set({ searchQuery: query }),

  toggleLibrary: () => set((s) => ({ showLibrary: !s.showLibrary })),

  fetchLibrary: async () => {
    set({ isLoading: true });
    try {
      const entries = await invoke<MediaEntry[]>("library_list");
      set({ entries, isLoading: false });
    } catch (e) {
      console.error("Failed to fetch library:", e);
      set({ isLoading: false });
    }
  },

  search: async (query: string) => {
    set({ isLoading: true, searchQuery: query });
    try {
      const entries = await invoke<MediaEntry[]>("library_search", { query });
      set({ entries, isLoading: false });
    } catch (e) {
      console.error("Failed to search library:", e);
      set({ isLoading: false });
    }
  },

  scanDirectory: async (dir: string) => {
    set({ isLoading: true });
    try {
      await invoke<{ scanned_dir: string; added: number; entries: MediaEntry[] }>(
        "library_scan",
        { dir },
      );
      // Re-fetch the full library so the panel reflects the new state
      const entries = await invoke<MediaEntry[]>("library_list");
      set({ entries, isLoading: false });
    } catch (e) {
      console.error("Failed to scan directory:", e);
      set({ isLoading: false });
    }
  },

  clearLibrary: async () => {
    set({ isLoading: true });
    try {
      const r = await invoke<{ removed: number }>("library_clear");
      set({ entries: [], isLoading: false });
      return r.removed;
    } catch (e) {
      console.error("Failed to clear library:", e);
      set({ isLoading: false });
      return 0;
    }
  },
}));

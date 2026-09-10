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
  /**
   * Why the last library call failed, raw from the backend.
   *
   * Every one of these used to end in `console.error` and nothing else, so
   * scanning a folder we could not read spun, stopped, and left the panel
   * showing "your library is empty" — a claim that we looked and found
   * nothing, which is not what happened.
   */
  error: string | null;
  setSearchQuery: (query: string) => void;
  toggleLibrary: () => void;
  clearError: () => void;
  fetchLibrary: () => Promise<void>;
  search: (query: string) => Promise<void>;
  scanDirectory: (dir: string) => Promise<void>;
  clearLibrary: () => Promise<number>;
}

/** The backend's own words, or something printable if it did not send any. */
function describe(e: unknown): string {
  if (typeof e === "string" && e.trim() !== "") return e;
  if (e instanceof Error && e.message.trim() !== "") return e.message;
  try {
    return JSON.stringify(e) ?? "unknown error";
  } catch {
    return "unknown error";
  }
}

export const useLibraryStore = create<LibraryState>((set) => ({
  entries: [],
  searchQuery: "",
  isLoading: false,
  showLibrary: false,
  error: null,

  setSearchQuery: (query: string) => set({ searchQuery: query }),

  clearError: () => set({ error: null }),

  toggleLibrary: () => set((s) => ({ showLibrary: !s.showLibrary })),

  fetchLibrary: async () => {
    set({ isLoading: true });
    try {
      const entries = await invoke<MediaEntry[]>("library_list");
      set({ entries, isLoading: false, error: null });
    } catch (e) {
      console.error("Failed to fetch library:", e);
      set({ isLoading: false, error: describe(e) });
    }
  },

  search: async (query: string) => {
    set({ isLoading: true, searchQuery: query });
    try {
      const entries = await invoke<MediaEntry[]>("library_search", { query });
      set({ entries, isLoading: false, error: null });
    } catch (e) {
      console.error("Failed to search library:", e);
      set({ isLoading: false, error: describe(e) });
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
      set({ entries, isLoading: false, error: null });
    } catch (e) {
      console.error("Failed to scan directory:", e);
      set({ isLoading: false, error: describe(e) });
    }
  },

  clearLibrary: async () => {
    set({ isLoading: true });
    try {
      const r = await invoke<{ removed: number }>("library_clear");
      set({ entries: [], isLoading: false, error: null });
      return r.removed;
    } catch (e) {
      console.error("Failed to clear library:", e);
      set({ isLoading: false, error: describe(e) });
      return 0;
    }
  },
}));

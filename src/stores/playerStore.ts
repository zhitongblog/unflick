import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

interface PlayerState {
  state: "stopped" | "playing" | "paused";
  file: string | null;
  position: number;
  duration: number;
  volume: number;
  speed: number;
  // Actions
  setStatus: (status: Partial<PlayerState>) => void;
  play: (file: string) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  seek: (seconds: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setSpeed: (rate: number) => Promise<void>;
}

export const usePlayerStore = create<PlayerState>((set) => ({
  state: "stopped",
  file: null,
  position: 0,
  duration: 0,
  volume: 100,
  speed: 1,

  setStatus: (status) => set(status),

  play: async (file: string) => {
    try {
      await invoke("player_play", { file });
      set({ state: "playing", file, position: 0 });
    } catch (e) {
      console.error("Failed to play:", e);
    }
  },

  pause: async () => {
    try {
      await invoke("player_pause");
      set({ state: "paused" });
    } catch (e) {
      console.error("Failed to pause:", e);
    }
  },

  resume: async () => {
    try {
      await invoke("player_resume");
      set({ state: "playing" });
    } catch (e) {
      console.error("Failed to resume:", e);
    }
  },

  stop: async () => {
    try {
      await invoke("player_stop");
      set({ state: "stopped", file: null, position: 0, duration: 0 });
    } catch (e) {
      console.error("Failed to stop:", e);
    }
  },

  seek: async (seconds: number) => {
    try {
      await invoke("player_seek", { seconds });
      set({ position: seconds });
    } catch (e) {
      console.error("Failed to seek:", e);
    }
  },

  setVolume: async (level: number) => {
    try {
      const clamped = Math.max(0, Math.min(100, level));
      await invoke("player_set_volume", { level: clamped });
      set({ volume: clamped });
    } catch (e) {
      console.error("Failed to set volume:", e);
    }
  },

  setSpeed: async (rate: number) => {
    try {
      await invoke("player_set_speed", { rate });
      set({ speed: rate });
    } catch (e) {
      console.error("Failed to set speed:", e);
    }
  },
}));

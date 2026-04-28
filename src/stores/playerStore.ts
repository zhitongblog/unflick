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
  pollStatus: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;
}

// Module-level variable for the polling interval
let _pollInterval: ReturnType<typeof setInterval> | null = null;

export const usePlayerStore = create<PlayerState>((set, get) => ({
  state: "stopped",
  file: null,
  position: 0,
  duration: 0,
  volume: 100,
  speed: 1,

  setStatus: (status) => set(status),

  pollStatus: async () => {
    try {
      const status = await invoke<{
        state: string;
        file: string | null;
        position: number;
        duration: number;
        volume: number;
        speed: number;
      }>("player_status");
      set({
        state: status.state as PlayerState["state"],
        file: status.file,
        position: status.position,
        duration: status.duration,
        volume: status.volume,
        speed: status.speed,
      });
      // Auto-stop polling if playback ended
      if (status.state === "stopped") {
        get().stopPolling();
        // Clear saved position if playback reached the end naturally
        // Use the previous store state (captured before set()) via get() is fine here
        // because we check status.file/duration which come from the same snapshot
        const stoppedFile = get().file; // set() already ran, but file may still reflect last known
        const checkFile = status.file ?? stoppedFile;
        if (checkFile && status.duration > 0 && status.position >= status.duration - 1) {
          invoke("clear_position", { path: checkFile }).catch(() => {});
        }
      }
    } catch (e) {
      console.error("Failed to poll status:", e);
    }
  },

  startPolling: () => {
    if (_pollInterval !== null) return;
    _pollInterval = setInterval(() => {
      get().pollStatus();
    }, 500);
  },

  stopPolling: () => {
    if (_pollInterval !== null) {
      clearInterval(_pollInterval);
      _pollInterval = null;
    }
  },

  play: async (file: string) => {
    try {
      // Check for a saved resume position
      let seekTo: number | undefined;
      try {
        const posResult = await invoke<{ position: number | null }>("get_position", { path: file });
        if (posResult.position != null && posResult.position > 5) {
          seekTo = posResult.position;
        }
      } catch {
        // ignore — proceed without seek
      }

      await invoke("player_play", { file, seek: seekTo ?? null });
      set({ state: "playing", file, position: seekTo ?? 0 });

      // Record this play in history (fire-and-forget)
      invoke("record_play", { path: file }).catch(() => {});

      // Fetch status after a short delay to get duration
      setTimeout(() => {
        get().pollStatus();
      }, 500);
      get().startPolling();
    } catch (e) {
      console.error("Failed to play:", e);
    }
  },

  pause: async () => {
    try {
      await invoke("player_pause");
      set({ state: "paused" });
      get().stopPolling();
    } catch (e) {
      console.error("Failed to pause:", e);
    }
  },

  resume: async () => {
    try {
      await invoke("player_resume");
      set({ state: "playing" });
      get().startPolling();
    } catch (e) {
      console.error("Failed to resume:", e);
    }
  },

  stop: async () => {
    try {
      // Save position before stopping so we can resume later
      const { file, position, duration } = get();
      if (file && position > 0) {
        // Only save if not near the very end (within 1 second of end)
        if (duration <= 0 || position < duration - 1) {
          invoke("save_position", { path: file, position }).catch(() => {});
        } else {
          // At the end — clear any stale saved position
          invoke("clear_position", { path: file }).catch(() => {});
        }
      }
      await invoke("player_stop");
      set({ state: "stopped", file: null, position: 0, duration: 0 });
      get().stopPolling();
    } catch (e) {
      console.error("Failed to stop:", e);
    }
  },

  seek: async (seconds: number) => {
    try {
      await invoke("player_seek", { seconds });
      set({ position: seconds });
      // Refresh status after seek
      setTimeout(() => {
        get().pollStatus();
      }, 200);
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

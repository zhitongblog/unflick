import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { detectLocale, isLocale, type Locale } from "../i18n/config";

interface SettingsState {
  showSettings: boolean;
  whisperMode: "off" | "local" | "api";
  whisperModelPath: string | null;
  whisperBinaryPath: string | null;
  openaiApiKey: string | null;
  theme: "dark" | "midnight" | "purple";
  volume: number;
  proxy: string | null;
  /** UI language. Auto-detected on first launch from navigator.language. */
  locale: Locale;
  /** When non-null, captureScreenshot() saves into this folder directly
   *  with an auto-generated filename, no save dialog. */
  screenshotDir: string | null;
  toggleSettings: () => void;
  setWhisperMode: (mode: "off" | "local" | "api") => void;
  setWhisperModelPath: (path: string | null) => void;
  setWhisperBinaryPath: (path: string | null) => void;
  setOpenaiApiKey: (key: string | null) => void;
  setTheme: (theme: "dark" | "midnight" | "purple") => void;
  setVolumeLevel: (volume: number) => void;
  setProxy: (p: string | null) => void;
  setLocale: (locale: Locale) => void;
  setScreenshotDir: (dir: string | null) => void;
  loadSettings: () => Promise<void>;
  saveSettings: () => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  showSettings: false,
  whisperMode: "off",
  whisperModelPath: null,
  whisperBinaryPath: null,
  openaiApiKey: null,
  theme: "dark",
  volume: 100,
  proxy: null,
  // Best-effort locale guess. Real value (auto-detected or user-saved) is
  // applied by loadSettings(); using navigator.language here keeps the
  // first paint reasonable even before that runs.
  locale: detectLocale(typeof navigator !== "undefined" ? navigator.language : undefined),
  screenshotDir: null,

  toggleSettings: () => set((s) => ({ showSettings: !s.showSettings })),

  setWhisperMode: (mode) => set({ whisperMode: mode }),
  setWhisperModelPath: (path) => set({ whisperModelPath: path }),
  setWhisperBinaryPath: (path) => set({ whisperBinaryPath: path }),
  setOpenaiApiKey: (key) => set({ openaiApiKey: key }),
  setTheme: (theme) => set({ theme }),
  setVolumeLevel: (volume) => set({ volume }),
  setProxy: (p) => set({ proxy: p }),
  setLocale: (locale) => set({ locale }),
  setScreenshotDir: (dir) => set({ screenshotDir: dir }),

  loadSettings: async () => {
    try {
      const data = await invoke<Record<string, unknown>>("load_settings");
      if (data && typeof data === "object") {
        const updates: Partial<SettingsState> = {};
        if (data.whisperMode === "local" || data.whisperMode === "api") {
          updates.whisperMode = data.whisperMode;
        }
        if (typeof data.whisperModelPath === "string" || data.whisperModelPath === null) {
          updates.whisperModelPath = data.whisperModelPath as string | null;
        }
        if (typeof data.whisperBinaryPath === "string" || data.whisperBinaryPath === null) {
          updates.whisperBinaryPath = data.whisperBinaryPath as string | null;
        }
        if (typeof data.openaiApiKey === "string" || data.openaiApiKey === null) {
          updates.openaiApiKey = data.openaiApiKey as string | null;
        }
        if (data.theme === "dark" || data.theme === "midnight" || data.theme === "purple") {
          updates.theme = data.theme;
        }
        if (typeof data.volume === "number") {
          updates.volume = data.volume;
        }
        if (typeof data.proxy === "string" || data.proxy === null) {
          updates.proxy = data.proxy as string | null;
        }
        // Persisted locale wins; if absent we keep whatever the navigator-
        // based default already set (so first launch picks the OS language).
        if (typeof data.locale === "string" && isLocale(data.locale)) {
          updates.locale = data.locale;
        }
        if (typeof data.screenshotDir === "string" || data.screenshotDir === null) {
          updates.screenshotDir = data.screenshotDir as string | null;
        }
        set(updates);
      }
    } catch {
      // Settings file doesn't exist yet — use defaults
    }
  },

  saveSettings: async () => {
    const { whisperMode, whisperModelPath, whisperBinaryPath, openaiApiKey, theme, volume, proxy, locale, screenshotDir } = get();
    const payload = JSON.stringify({
      whisperMode,
      whisperModelPath,
      whisperBinaryPath,
      openaiApiKey,
      theme,
      volume,
      proxy,
      locale,
      screenshotDir,
    });
    try {
      await invoke("save_settings", { settings: payload });
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  },
}));

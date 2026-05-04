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
  /** Pin the unflick window above all other apps. Useful for watching
   *  a tutorial / video chat while doing something else. Persisted. */
  alwaysOnTop: boolean;
  /** SponsorBlock auto-skip toggle. When true the player polls
   *  playback-time and seeks past sponsor segments fetched from
   *  sponsor.ajay.app for the current YouTube video. */
  sponsorblockEnabled: boolean;
  /** Which SponsorBlock categories to skip. Defaults to the common five. */
  sponsorblockCategories: string[];
  /** Auto-download external subtitles via yt-dlp when playing a streaming
   *  URL. Files are written to a temp dir and attached via mpv sub-add. */
  autoDownloadSubtitles: boolean;
  /** Languages to request from yt-dlp (e.g. ["en", "zh-CN"]). */
  subtitleLanguages: string[];
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
  setAlwaysOnTop: (v: boolean) => void;
  setSponsorblockEnabled: (v: boolean) => void;
  setSponsorblockCategories: (cats: string[]) => void;
  setAutoDownloadSubtitles: (v: boolean) => void;
  setSubtitleLanguages: (langs: string[]) => void;
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
  alwaysOnTop: false,
  sponsorblockEnabled: true,
  sponsorblockCategories: ["sponsor", "selfpromo", "intro", "outro", "interaction"],
  autoDownloadSubtitles: true,
  subtitleLanguages: ["en", "zh-CN"],

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
  setAlwaysOnTop: (v) => set({ alwaysOnTop: v }),
  setSponsorblockEnabled: (v) => set({ sponsorblockEnabled: v }),
  setSponsorblockCategories: (cats) => set({ sponsorblockCategories: cats }),
  setAutoDownloadSubtitles: (v) => set({ autoDownloadSubtitles: v }),
  setSubtitleLanguages: (langs) => set({ subtitleLanguages: langs }),

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
        if (typeof data.alwaysOnTop === "boolean") {
          updates.alwaysOnTop = data.alwaysOnTop;
        }
        // SponsorBlock + auto-subtitle keys are persisted in snake_case
        // because they're shared with CLI/MCP, which use snake_case
        // throughout. Read them via bracket access so TS doesn't whine
        // about index signatures.
        if (typeof data["sponsorblock_enabled"] === "boolean") {
          updates.sponsorblockEnabled = data["sponsorblock_enabled"] as boolean;
        }
        if (Array.isArray(data["sponsorblock_categories"])) {
          updates.sponsorblockCategories = (data["sponsorblock_categories"] as unknown[]).filter(
            (s): s is string => typeof s === "string"
          );
        }
        if (typeof data["auto_download_subtitles"] === "boolean") {
          updates.autoDownloadSubtitles = data["auto_download_subtitles"] as boolean;
        }
        if (Array.isArray(data["subtitle_languages"])) {
          updates.subtitleLanguages = (data["subtitle_languages"] as unknown[]).filter(
            (s): s is string => typeof s === "string"
          );
        }
        set(updates);
      }
    } catch {
      // Settings file doesn't exist yet — use defaults
    }
  },

  saveSettings: async () => {
    const {
      whisperMode, whisperModelPath, whisperBinaryPath, openaiApiKey, theme, volume, proxy, locale,
      screenshotDir, alwaysOnTop,
      sponsorblockEnabled, sponsorblockCategories, autoDownloadSubtitles, subtitleLanguages,
    } = get();
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
      alwaysOnTop,
      // Snake-case keys here so CLI/MCP (which write the same file) and
      // the GUI converge on a single schema for the v0.9 streaming feats.
      sponsorblock_enabled: sponsorblockEnabled,
      sponsorblock_categories: sponsorblockCategories,
      auto_download_subtitles: autoDownloadSubtitles,
      subtitle_languages: subtitleLanguages,
    });
    try {
      await invoke("save_settings", { settings: payload });
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  },
}));

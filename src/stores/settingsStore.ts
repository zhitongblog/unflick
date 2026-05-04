import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { detectLocale, isLocale, type Locale } from "../i18n/config";

/**
 * Allowed quality values for streaming URL extraction. `null` and `"auto"`
 * both mean "let yt-dlp pick the best single-URL format". The numeric
 * variants cap by height; `"audio_only"` strips the video stream.
 *
 * Mirrors the Rust side (`core::settings::QUALITY_VALUES`).
 */
export type PreferredQuality =
  | "auto"
  | "2160p"
  | "1440p"
  | "1080p"
  | "720p"
  | "480p"
  | "audio_only";

/**
 * Browser whose login cookies yt-dlp can borrow for age-gated / paywalled
 * pages. `null` and `"none"` both mean "don't pass `--cookies-from-browser`".
 *
 * Mirrors the Rust side (`core::settings::COOKIES_BROWSER_VALUES`).
 */
export type CookiesBrowser =
  | "none"
  | "firefox"
  | "chrome"
  | "chromium"
  | "safari"
  | "edge"
  | "brave";

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
  /** Default quality for streaming URL extraction (yt-dlp). `null` = auto. */
  preferredQuality: PreferredQuality | null;
  /** Borrow login cookies from this browser when extracting URLs. `null` = off. */
  cookiesBrowser: CookiesBrowser | null;
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
  setPreferredQuality: (q: PreferredQuality | null) => void;
  setCookiesBrowser: (b: CookiesBrowser | null) => void;
  loadSettings: () => Promise<void>;
  saveSettings: () => Promise<void>;
}

const QUALITY_VALUES: PreferredQuality[] = [
  "auto",
  "2160p",
  "1440p",
  "1080p",
  "720p",
  "480p",
  "audio_only",
];

const COOKIES_BROWSER_VALUES: CookiesBrowser[] = [
  "none",
  "firefox",
  "chrome",
  "chromium",
  "safari",
  "edge",
  "brave",
];

function isPreferredQuality(v: unknown): v is PreferredQuality {
  return typeof v === "string" && (QUALITY_VALUES as string[]).includes(v);
}

function isCookiesBrowser(v: unknown): v is CookiesBrowser {
  return typeof v === "string" && (COOKIES_BROWSER_VALUES as string[]).includes(v);
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
  preferredQuality: null,
  cookiesBrowser: null,

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
  setPreferredQuality: (q) => set({ preferredQuality: q }),
  setCookiesBrowser: (b) => set({ cookiesBrowser: b }),

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
        // Streaming knobs (v0.9 P1). Both default to null when missing
        // or unrecognised, which the Rust side treats as "auto" / "no
        // cookies", so existing settings.json files keep working.
        if (data.preferred_quality === null || isPreferredQuality(data.preferred_quality)) {
          updates.preferredQuality = data.preferred_quality;
        } else if (data.preferredQuality === null || isPreferredQuality(data.preferredQuality)) {
          updates.preferredQuality = data.preferredQuality;
        }
        if (data.cookies_browser === null || isCookiesBrowser(data.cookies_browser)) {
          updates.cookiesBrowser = data.cookies_browser;
        } else if (data.cookiesBrowser === null || isCookiesBrowser(data.cookiesBrowser)) {
          updates.cookiesBrowser = data.cookiesBrowser;
        }
        set(updates);
      }
    } catch {
      // Settings file doesn't exist yet — use defaults
    }
  },

  saveSettings: async () => {
    const {
      whisperMode, whisperModelPath, whisperBinaryPath, openaiApiKey,
      theme, volume, proxy, locale, screenshotDir, alwaysOnTop,
      preferredQuality, cookiesBrowser,
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
      // Snake_case keys here so the Rust core helpers
      // (`core::settings::preferred_quality`, `cookies_browser`) can read
      // them without an extra translation step. Prior camelCase keys above
      // are preserved as-is for backwards compatibility with the existing
      // GUI-only flow.
      preferred_quality: preferredQuality,
      cookies_browser: cookiesBrowser,
    });
    try {
      await invoke("save_settings", { settings: payload });
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  },
}));

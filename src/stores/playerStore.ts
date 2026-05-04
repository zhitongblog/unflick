import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useIncognitoStore } from "./incognitoStore";
import { detectStreamingSite } from "../lib/streamingSites";

/**
 * v0.8 playerStore — drives playback through libmpv via Tauri commands.
 * The HTML5 <video> path is gone: there's no DOM element to reference, and
 * position/duration/state are pulled from mpv via a polled `player_status`
 * call started by the App on mount.
 */

export interface SubtitleTrack {
  /** Numeric mpv track ID. UI keys off this. */
  id: number;
  label: string;
  lang: string | null;
  external: string | null;
  active: boolean;
}

interface BackendStatus {
  state: "stopped" | "playing" | "paused";
  file: string | null;
  position: number;
  duration: number;
  volume: number;
  speed: number;
}

interface PlayerState {
  state: "stopped" | "playing" | "paused";
  file: string | null;
  position: number;
  duration: number;
  volume: number;
  speed: number;
  /** Status of upstream URL extraction (yt-dlp). null = idle. */
  extracting: { url: string; site: string } | null;
  extractError: string | null;
  subtitles: SubtitleTrack[];

  /** Push the latest backend status snapshot. Called by the App's poller. */
  ingestStatus: (s: BackendStatus) => void;

  /**
   * Play a path or URL. The optional `qualityOverride` lets a caller (e.g.
   * the URL dialog) bypass the saved `preferredQuality` for a single
   * extraction; pass `null` / undefined to honour the saved setting.
   */
  play: (file: string, qualityOverride?: string | null) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  seek: (seconds: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setSpeed: (rate: number) => Promise<void>;
  clearExtractError: () => void;
  loadSubtitle: (path: string) => Promise<void>;
  selectSubtitle: (id: number | null) => Promise<void>;
  refreshSubtitles: () => Promise<void>;
  clearSubtitles: () => Promise<void>;
}

function isUrl(p: string): boolean {
  return /^(https?|file|blob|data):/i.test(p);
}

/**
 * Direct media URLs (`*.mp4`, `*.m3u8`, …) are fed straight to mpv —
 * yt-dlp would just round-trip them. Everything else goes through the
 * extractor, regardless of whether we recognise the host: yt-dlp
 * supports ~1500 sites and refusing to try cuts users off from most
 * of them. Match conservatively against the URL path to keep the
 * fast path for mpv's own HTTP(S) loader.
 */
const DIRECT_MEDIA_EXT =
  /\.(mp4|m4v|mov|webm|mkv|avi|flv|wmv|ts|mp3|m4a|aac|ogg|oga|opus|flac|wav|m3u8|mpd)(?:[?#]|$)/i;

function isDirectMediaUrl(url: string): boolean {
  if (!/^https?:/i.test(url)) return false;
  try {
    const u = new URL(url);
    return DIRECT_MEDIA_EXT.test(u.pathname);
  } catch {
    return DIRECT_MEDIA_EXT.test(url);
  }
}

interface MpvSubTrack {
  id: number;
  title: string | null;
  lang: string | null;
  external_file: string | null;
  selected: boolean;
}

export const usePlayerStore = create<PlayerState>((set, get) => ({
  state: "stopped",
  file: null,
  position: 0,
  duration: 0,
  volume: 100,
  speed: 1,
  extracting: null,
  extractError: null,
  subtitles: [],

  clearExtractError: () => set({ extractError: null }),

  ingestStatus: (s) => {
    // Don't clobber volume/speed: those are user inputs the UI owns until
    // the user changes them. mpv reflects them but UI can be ahead.
    set({
      state: s.state,
      file: s.file,
      position: s.position,
      duration: s.duration,
    });
  },

  refreshSubtitles: async () => {
    try {
      const tracks = await invoke<MpvSubTrack[]>("subtitle_list");
      const mapped: SubtitleTrack[] = tracks.map((t) => ({
        id: t.id,
        label: t.title ?? t.external_file?.split(/[\\/]/).pop() ?? `Track ${t.id}`,
        lang: t.lang,
        external: t.external_file,
        active: t.selected,
      }));
      set({ subtitles: mapped });
    } catch (e) {
      console.warn("subtitle_list failed:", e);
    }
  },

  loadSubtitle: async (path: string) => {
    await invoke("subtitle_load", { path });
    await get().refreshSubtitles();
  },

  selectSubtitle: async (id: number | null) => {
    // mpv uses "no" to disable subtitles; passing 0 also works in our wrapper.
    await invoke("subtitle_select", { id: id ?? 0 });
    await get().refreshSubtitles();
  },

  clearSubtitles: async () => {
    // Single sub-select to "off" deselects whatever's active. mpv keeps
    // tracks loaded but inactive; the local mirror just empties.
    try {
      await invoke("subtitle_select", { id: 0 });
    } catch {
      /* ignore */
    }
    set({ subtitles: [] });
  },

  play: async (file: string, qualityOverride?: string | null) => {
    set({ extractError: null });

    // URLs that aren't direct media files go through yt-dlp. We attempt
    // extraction even on hosts we don't recognise — yt-dlp supports
    // 1500+ sites and refusing to try is worse than letting it fail.
    let mediaTarget = file;
    const isHttp = /^https?:/i.test(file);
    const needsExtraction = isHttp && !isDirectMediaUrl(file);
    if (needsExtraction) {
      const site = detectStreamingSite(file) ?? "Link";
      set({ extracting: { url: file, site } });
      try {
        const { useSettingsStore } = await import("./settingsStore");
        const settingsState = useSettingsStore.getState();
        const proxy = settingsState.proxy ?? null;
        // Per-call quality overrides the saved default; null/undefined
        // means "use saved setting" (which the Rust side reads itself
        // from settings.json, so passing null is equivalent to omit).
        const quality =
          qualityOverride !== undefined && qualityOverride !== null
            ? qualityOverride
            : settingsState.preferredQuality;
        const cookiesBrowser = settingsState.cookiesBrowser;
        const r = await invoke<{ stream_url: string }>("extract_stream_url", {
          url: file,
          proxy,
          quality,
          cookiesBrowser,
        });
        mediaTarget = r.stream_url;
      } catch (err) {
        set({
          extracting: null,
          extractError:
            typeof err === "string"
              ? err
              : "Failed to extract stream URL. Make sure yt-dlp is installed.",
        });
        return;
      }
      set({ extracting: null });
    }

    // mpv handles its own resume via observed time-pos, but our DB tracks
    // positions across sessions — keep that, pass to mpv via --start.
    let resumeAt: number | null = null;
    try {
      const r = await invoke<{ position: number | null }>("get_position", {
        path: file,
      });
      if (r.position != null && r.position > 5) resumeAt = r.position;
    } catch {
      /* ignore */
    }

    try {
      await invoke("player_play", {
        file: mediaTarget,
        seek: resumeAt,
        volume: null,
        speed: null,
      });
      set({ state: "playing", file, position: resumeAt ?? 0 });
    } catch (e) {
      console.error("player_play failed:", e);
      throw e;
    }

    // History — skip in incognito mode.
    if (!useIncognitoStore.getState().enabled) {
      invoke("record_play", { path: file }).catch(() => {});
    }

    // Sidecar subtitle auto-load (movie.srt, movie.en.srt next to local video).
    if (!isUrl(file)) {
      invoke<{ subtitles: { path: string; lang: string | null; ext: string }[] }>(
        "find_sidecar_subtitles",
        { videoPath: file },
      )
        .then(async (r) => {
          for (const sub of r.subtitles) {
            try {
              await get().loadSubtitle(sub.path);
            } catch {
              /* skip unsupported formats — mpv reports + we move on */
            }
          }
        })
        .catch(() => {});
    }
  },

  pause: async () => {
    try {
      await invoke("player_pause");
      set({ state: "paused" });
    } catch (e) {
      console.error("player_pause failed:", e);
    }
  },

  resume: async () => {
    try {
      await invoke("player_resume");
      set({ state: "playing" });
    } catch (e) {
      console.error("player_resume failed:", e);
    }
  },

  stop: async () => {
    const { file, position, duration } = get();
    if (file && position > 0) {
      const incognito = useIncognitoStore.getState().enabled;
      if (duration <= 0 || position < duration - 1) {
        if (!incognito) {
          invoke("save_position", { path: file, position }).catch(() => {});
        }
      } else {
        invoke("clear_position", { path: file }).catch(() => {});
      }
    }
    try {
      await invoke("player_stop");
    } catch (e) {
      console.error("player_stop failed:", e);
    }
    set({ state: "stopped", file: null, position: 0, duration: 0, subtitles: [] });
  },

  seek: async (seconds: number) => {
    try {
      await invoke("player_seek", { seconds: Math.max(0, seconds) });
      set({ position: Math.max(0, seconds) });
    } catch (e) {
      console.error("player_seek failed:", e);
    }
  },

  setVolume: async (level: number) => {
    // Range is 0..150: above 100 lets users boost soft sources past
    // 100% the way VLC / Windows volume mixer can. mpv's volume-max
    // is set to 200 so 150 still has headroom.
    const clamped = Math.max(0, Math.min(150, level));
    set({ volume: clamped });
    try {
      await invoke("player_set_volume", { level: clamped });
    } catch (e) {
      console.error("player_set_volume failed:", e);
    }
    try {
      const { useSettingsStore } = await import("./settingsStore");
      useSettingsStore.getState().setVolumeLevel(clamped);
      useSettingsStore.getState().saveSettings();
    } catch (e) {
      console.error("Failed to persist volume:", e);
    }
  },

  setSpeed: async (rate: number) => {
    set({ speed: rate });
    try {
      await invoke("player_set_speed", { rate });
    } catch (e) {
      console.error("player_set_speed failed:", e);
    }
  },
}));

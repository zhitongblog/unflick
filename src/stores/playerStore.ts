import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";

export interface SubtitleTrack {
  id: string;
  label: string;
  src: string; // blob URL
  active: boolean;
}

interface PlayerState {
  state: "stopped" | "playing" | "paused";
  file: string | null;
  position: number;
  duration: number;
  volume: number;
  speed: number;
  // Status of upstream URL extraction (yt-dlp). null = idle.
  extracting: { url: string; site: string } | null;
  extractError: string | null;
  // Loaded subtitle tracks (HTML5 <track> elements)
  subtitles: SubtitleTrack[];
  // Internal: registered <video> element
  setVideoElement: (el: HTMLVideoElement | null) => void;
  // Actions
  setStatus: (status: Partial<PlayerState>) => void;
  play: (file: string) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  seek: (seconds: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setSpeed: (rate: number) => Promise<void>;
  clearExtractError: () => void;
  loadSubtitle: (path: string) => Promise<void>;
  selectSubtitle: (id: string | null) => void;
  clearSubtitles: () => void;
}

// URL hosts that need yt-dlp extraction (the URL is a webpage, not a media file).
const EXTRACT_HOSTS: { pattern: RegExp; label: string }[] = [
  { pattern: /(?:youtube\.com|youtu\.be)/i, label: "YouTube" },
  { pattern: /bilibili\.com/i, label: "Bilibili" },
  { pattern: /twitch\.tv/i, label: "Twitch" },
  { pattern: /vimeo\.com/i, label: "Vimeo" },
  { pattern: /weibo\.com/i, label: "Weibo" },
  { pattern: /douyin\.com/i, label: "Douyin" },
  { pattern: /tiktok\.com/i, label: "TikTok" },
];

function detectUpstreamSite(url: string): string | null {
  for (const { pattern, label } of EXTRACT_HOSTS) {
    if (pattern.test(url)) return label;
  }
  return null;
}

/// Convert an SRT subtitle file to WebVTT format.
function srtToVtt(srt: string): string {
  // SRT uses comma as decimal separator, WebVTT uses period.
  // Strip leading BOM if present.
  let s = srt.replace(/^﻿/, "");
  // Normalize line endings
  s = s.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  // Replace timestamp commas with periods (only inside cue timing lines)
  s = s.replace(
    /(\d\d:\d\d:\d\d),(\d{1,3})\s+-->\s+(\d\d:\d\d:\d\d),(\d{1,3})/g,
    "$1.$2 --> $3.$4",
  );
  return "WEBVTT\n\n" + s.trim() + "\n";
}

/// Detect subtitle format from file extension and convert to WebVTT.
function toWebVTT(text: string, ext: string): string {
  const e = ext.toLowerCase();
  if (e === "vtt") return text;
  if (e === "srt") return srtToVtt(text);
  // ASS/SSA not supported yet — render an empty VTT so the track shows but
  // doesn't display anything (better than crashing).
  return "WEBVTT\n\n";
}

let videoEl: HTMLVideoElement | null = null;

function isUrl(p: string): boolean {
  return /^(https?|file|blob|data):/i.test(p);
}

function toMediaSrc(path: string): string {
  if (isUrl(path)) return path;
  // Convert local path to Tauri asset URL
  return convertFileSrc(path);
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

  setStatus: (status) => set(status),

  clearExtractError: () => set({ extractError: null }),

  loadSubtitle: async (path: string) => {
    try {
      const r = await invoke<{ text: string }>("read_text_file", { path });
      const ext = path.split(".").pop() ?? "srt";
      const vtt = toWebVTT(r.text, ext);
      const blob = new Blob([vtt], { type: "text/vtt" });
      const blobUrl = URL.createObjectURL(blob);
      const label = path.split(/[\\/]/).pop() ?? "Subtitle";
      const id = `sub-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;

      // Deactivate other tracks; new one becomes active
      const current = get().subtitles.map((t) => ({ ...t, active: false }));
      set({ subtitles: [...current, { id, label, src: blobUrl, active: true }] });
    } catch (e) {
      console.error("Failed to load subtitle:", e);
      throw e;
    }
  },

  selectSubtitle: (id: string | null) => {
    const next = get().subtitles.map((t) => ({ ...t, active: t.id === id }));
    set({ subtitles: next });
    // Apply to the live <track> elements
    if (videoEl) {
      for (let i = 0; i < videoEl.textTracks.length; i++) {
        const tt = videoEl.textTracks[i];
        const target = next[i];
        tt.mode = target?.active ? "showing" : "disabled";
      }
    }
  },

  clearSubtitles: () => {
    // Revoke blob URLs to free memory
    for (const t of get().subtitles) {
      try { URL.revokeObjectURL(t.src); } catch { /* ignore */ }
    }
    set({ subtitles: [] });
  },

  setVideoElement: (el) => {
    videoEl = el;
    if (el) {
      // Apply current store state to the element
      const { volume, speed } = get();
      el.volume = Math.max(0, Math.min(1, volume / 100));
      el.playbackRate = speed;
    }
  },

  play: async (file: string) => {
    if (!videoEl) {
      console.error("video element not registered");
      return;
    }
    // Reset subtitles when starting a new file
    get().clearSubtitles();
    set({ extractError: null });

    try {
      // Detect URLs from upstream sites that need yt-dlp extraction
      let mediaTarget = file;
      const site = isUrl(file) ? detectUpstreamSite(file) : null;
      if (site) {
        set({ extracting: { url: file, site } });
        try {
          const { useSettingsStore } = await import("./settingsStore");
          const proxy = useSettingsStore.getState().proxy ?? null;
          const r = await invoke<{ stream_url: string }>("extract_stream_url", {
            url: file,
            proxy,
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

      // Load saved resume position (use the original input as the key, not the
      // resolved stream URL — stream URLs often expire or change)
      let resumeAt: number | null = null;
      try {
        const r = await invoke<{ position: number | null }>("get_position", { path: file });
        if (r.position != null && r.position > 5) resumeAt = r.position;
      } catch {
        // ignore
      }

      // CORS: external HTTPS streams (googlevideo, CDN) usually don't return
      // CORS headers, so requesting with crossOrigin="anonymous" causes the
      // browser to reject the response. Only set anonymous mode for local
      // files served via Tauri's asset protocol (which always sends CORS
      // headers and is needed for canvas screenshots to work).
      const isExternal = isUrl(mediaTarget) && !mediaTarget.startsWith("blob:");
      if (isExternal) {
        videoEl.removeAttribute("crossorigin");
      } else {
        videoEl.crossOrigin = "anonymous";
      }
      videoEl.src = toMediaSrc(mediaTarget);
      videoEl.load();
      set({ state: "playing", file, position: resumeAt ?? 0, duration: 0 });

      const startPlayback = () => {
        if (resumeAt != null && videoEl) {
          videoEl.currentTime = resumeAt;
        }
        videoEl?.play().catch((err) => {
          console.error("video play failed:", err);
        });
      };

      if (videoEl.readyState >= 1) {
        startPlayback();
      } else {
        videoEl.addEventListener("loadedmetadata", startPlayback, { once: true });
      }

      // Record play history (fire-and-forget) — store the user-facing input
      invoke("record_play", { path: file }).catch(() => {});

      // Auto-load any sidecar subtitle files (movie.srt, movie.en.srt, …)
      // sitting next to a local video — fire-and-forget, runs in background.
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
                // Skip subtitles we can't read (binary ASS, etc.)
              }
            }
          })
          .catch(() => {});
      }
    } catch (e) {
      set({ extracting: null });
      console.error("Failed to play:", e);
    }
  },

  pause: async () => {
    if (!videoEl) return;
    videoEl.pause();
    set({ state: "paused" });
  },

  resume: async () => {
    if (!videoEl) return;
    videoEl.play().catch((e) => console.error("resume failed:", e));
    set({ state: "playing" });
  },

  stop: async () => {
    const { file, position, duration } = get();
    if (file && position > 0) {
      if (duration <= 0 || position < duration - 1) {
        invoke("save_position", { path: file, position }).catch(() => {});
      } else {
        invoke("clear_position", { path: file }).catch(() => {});
      }
    }
    if (videoEl) {
      videoEl.pause();
      videoEl.removeAttribute("src");
      videoEl.load();
    }
    set({ state: "stopped", file: null, position: 0, duration: 0 });
  },

  seek: async (seconds: number) => {
    if (!videoEl) return;
    videoEl.currentTime = Math.max(0, seconds);
    set({ position: videoEl.currentTime });
  },

  setVolume: async (level: number) => {
    const clamped = Math.max(0, Math.min(100, level));
    if (videoEl) videoEl.volume = clamped / 100;
    set({ volume: clamped });
    try {
      const { useSettingsStore } = await import("./settingsStore");
      useSettingsStore.getState().setVolumeLevel(clamped);
      useSettingsStore.getState().saveSettings();
    } catch (e) {
      console.error("Failed to persist volume:", e);
    }
  },

  setSpeed: async (rate: number) => {
    if (videoEl) videoEl.playbackRate = rate;
    set({ speed: rate });
  },
}));

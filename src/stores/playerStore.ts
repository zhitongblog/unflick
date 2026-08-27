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

/** One entry from mpv's chapter list. `time` is the start, in seconds. */
export interface Chapter {
  index: number;
  title: string | null;
  time: number;
  current: boolean;
}

/**
 * A saved position in a file. `name` is null when the user never gave it
 * one, and every surface shows the timestamp instead.
 */
export interface Bookmark {
  id: number;
  path: string;
  position: number;
  name: string | null;
  created_at: string;
}

/** A-B loop bounds. Either may be unset; `active` means both are. */
/**
 * What is playing, described the way a person would. Mirrors
 * `core::nowplaying::NowPlaying`.
 */
export interface NowPlaying {
  file: string | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  /** False for a file whose only video track is an embedded cover. */
  has_video: boolean;
  /** Extracted cover art on disk, when the file carries any. */
  cover: string | null;
  duration: number;
}

export interface AbLoop {
  a: number | null;
  b: number | null;
  active: boolean;
}

export interface BackendStatus {
  /**
   * Fields below `speed` ride along on the same poll so state changed from
   * outside the GUI — a CLI call, an MCP tool — is reflected on screen.
   * Older backends omit them, hence all optional.
   */
  ab_loop?: AbLoop;
  sub_delay?: number;
  audio_delay?: number;
  chapter?: number | null;
  chapter_count?: number;
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
  /**
   * Why the last `play` never reached the screen: a path that isn't there,
   * a share that's down, a protocol this build has no support for. Distinct
   * from `extractError`, which is yt-dlp failing to resolve a page URL —
   * different causes, different fixes, and only one of them is about the
   * network being reachable.
   */
  openError: string | null;
  subtitles: SubtitleTrack[];

  /** Chapters of the current file. Empty for files without any. */
  chapters: Chapter[];
  /** Bookmarks for the current file, in timeline order. */
  bookmarks: Bookmark[];
  abLoop: AbLoop;
  /** Subtitle / audio timing offsets, in seconds. */
  subDelay: number;
  audioDelay: number;

  /** Push the latest backend status snapshot. Called by the App's poller. */
  ingestStatus: (s: BackendStatus) => void;

  /**
   * Play a path or URL. The optional `qualityOverride` lets a caller (e.g.
   * the URL dialog) bypass the saved `preferredQuality` for a single
   * extraction; pass `null` / undefined to honour the saved setting.
   */
  play: (
    file: string,
    qualityOverride?: string | null,
    startAt?: number | null,
  ) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  seek: (seconds: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setSpeed: (rate: number) => Promise<void>;
  clearExtractError: () => void;
  clearOpenError: () => void;
  loadSubtitle: (path: string) => Promise<void>;
  selectSubtitle: (id: number | null) => Promise<void>;
  refreshSubtitles: () => Promise<void>;
  clearSubtitles: () => Promise<void>;

  refreshChapters: () => Promise<void>;
  seekChapter: (index: number) => Promise<void>;
  /** Step one chapter: +1 forward, -1 back. Clamps at both ends. */
  stepChapter: (delta: number) => Promise<void>;
  /** Drive the A-B loop. Returns the resulting bounds. */
  abLoopAction: (action: "a" | "b" | "clear" | "status") => Promise<AbLoop>;
  /**
   * Set the subtitle delay. `relative` nudges the current value instead of
   * replacing it. Returns the delay actually applied.
   */
  setSubDelay: (seconds: number, relative?: boolean) => Promise<number>;
  setAudioDelay: (seconds: number, relative?: boolean) => Promise<number>;
  /** Step one frame: +1 forward, -1 back. Pauses playback. */
  stepFrame: (delta: number) => Promise<void>;

  /**
   * Tags for the open file: what music mode shows, and how anything else
   * knows there is no picture to show. `has_video` is false for a file whose
   * only video track is an embedded cover.
   */
  nowPlaying: NowPlaying | null;
  refreshNowPlaying: () => Promise<void>;

  /** Re-read the current file's bookmarks. */
  refreshBookmarks: () => Promise<void>;
  /**
   * Bookmark the current position. Returns the stored bookmark, which may
   * be one that already existed — a second press a moment later corrects
   * the first rather than stacking a duplicate on top of it.
   */
  addBookmark: (name?: string) => Promise<Bookmark | null>;
  /**
   * Jump to a bookmark. Seeks when it belongs to the file already open,
   * and otherwise opens its file at that position — the same two cases
   * `bookmark goto` handles for the CLI and MCP.
   */
  gotoBookmark: (bookmark: Bookmark) => Promise<void>;
  renameBookmark: (id: number, name: string | null) => Promise<void>;
  removeBookmark: (id: number) => Promise<void>;
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

/**
 * How long after a local volume/speed change we keep ignoring the polled
 * value. Long enough to cover the invoke round trip, short enough that an
 * external change shows up as soon as the user stops dragging.
 */
const LOCAL_CHANGE_GRACE_MS = 700;

/** When the UI last set volume or speed itself. */
let lastLocalChange = 0;

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
  openError: null,
  nowPlaying: null,
  subtitles: [],
  chapters: [],
  bookmarks: [],
  abLoop: { a: null, b: null, active: false },
  subDelay: 0,
  audioDelay: 0,

  clearExtractError: () => set({ extractError: null }),
  clearOpenError: () => set({ openError: null }),

  ingestStatus: (s) => {
    const previousFile = get().file;

    // Volume and speed are set optimistically: the UI moves first, then
    // tells mpv. Adopting the polled value unconditionally would fight
    // that and make a slider stutter. But ignoring it forever was wrong
    // too — since v0.10 the CLI and MCP drive this same player, so
    // `unflick volume 60` left the UI showing a stale number that the
    // next click would then write back over the top of.
    //
    // So: adopt the player's value, unless we set it ourselves a moment
    // ago and the poll may simply not have caught up yet.
    const settled = Date.now() - lastLocalChange > LOCAL_CHANGE_GRACE_MS;
    const reconciled: { volume?: number; speed?: number } = {};
    if (settled && typeof s.volume === "number" && s.volume !== get().volume) {
      reconciled.volume = s.volume;
    }
    if (settled && typeof s.speed === "number" && s.speed !== get().speed) {
      reconciled.speed = s.speed;
    }

    set({
      state: s.state,
      file: s.file,
      position: s.position,
      duration: s.duration,
      ...reconciled,
      // Mirror whatever the player reports, so a loop point or delay set
      // from the CLI / an AI agent shows up in the UI too.
      ...(s.ab_loop ? { abLoop: s.ab_loop } : {}),
      ...(typeof s.sub_delay === "number" ? { subDelay: s.sub_delay } : {}),
      ...(typeof s.audio_delay === "number" ? { audioDelay: s.audio_delay } : {}),
    });

    // Chapters can appear or vanish without a file change — an AI agent
    // calling generate_chapters, or clear_chapters. Refetch only when the
    // count actually moves, so the 250 ms poll stays cheap.
    if (
      typeof s.chapter_count === "number" &&
      s.chapter_count !== get().chapters.length &&
      s.file === previousFile
    ) {
      void get().refreshChapters();
    }

    // Keep the chapter list's "current" flag honest as playback crosses a
    // boundary on its own, without re-fetching the whole list every tick.
    if (typeof s.chapter === "number") {
      const chapters = get().chapters;
      if (chapters.length > 0 && !chapters[s.chapter]?.current) {
        set({
          chapters: chapters.map((c) => ({ ...c, current: c.index === s.chapter })),
        });
      }
    }

    // A new file means new chapters and a stale loop / timing offsets.
    // mpv resets sub-delay and ab-loop per file, so mirror that here
    // rather than showing values that no longer apply. Detecting the
    // change from the poller also covers files loaded from outside the
    // GUI — a CLI call or an AI agent over MCP.
    if (s.file !== previousFile) {
      set({
        abLoop: { a: null, b: null, active: false },
        subDelay: 0,
        audioDelay: 0,
        chapters: [],
        bookmarks: [],
        nowPlaying: null,
        subtitles: [],
      });
      if (s.file) {
        // mpv populates chapter-list slightly after the file loads;
        // a short delay avoids reading an empty list on fast switches.
        setTimeout(() => {
          void get().refreshChapters();
          // Tags land with the file, but mpv fills `metadata` a beat after
          // the load event — same reason the chapter read waits.
          void get().refreshNowPlaying();
          // And the tracks. mpv auto-loads a sidecar `.srt`, so a file
          // opened from the CLI or an agent arrives with a subtitle
          // already on screen — while the menu, which only refetched on
          // the GUI's own play path, still showed "Off" with a tick and
          // no way to switch tracks.
          void get().refreshSubtitles();
        }, 400);
        void get().refreshBookmarks();
      }
    }
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

  play: async (
    file: string,
    qualityOverride?: string | null,
    startAt?: number | null,
  ) => {
    set({ extractError: null, openError: null });

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
    // An explicit start (a bookmark being opened) wins over the resume
    // point: the user named that spot, and silently landing somewhere else
    // would make the bookmark look broken.
    let resumeAt: number | null = startAt ?? null;
    if (resumeAt == null) {
      try {
        const r = await invoke<{ position: number | null }>("get_position", {
          path: file,
        });
        if (r.position != null && r.position > 5) resumeAt = r.position;
      } catch {
        /* ignore */
      }
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
      // Until the backend learned to wait for mpv's verdict, this never
      // fired: a missing file or an unreachable share came back "playing"
      // and the window just sat there. Now that it does fire, it has to be
      // visible — every caller here fires and forgets, so a rethrow would
      // land nowhere but the console.
      const message =
        typeof e === "string" ? e : e instanceof Error ? e.message : "Could not open this file";
      console.error("player_play failed:", e);
      set({ openError: message, state: "stopped" });
      window.dispatchEvent(
        new CustomEvent("unflick:toast", { detail: { kind: "error", message } }),
      );
      return;
    }

    // URL play path: now that mpv is loading the resolved stream, fire the
    // post-play hooks against the *original* page URL so SponsorBlock can
    // fetch sponsor segments and yt-dlp can grab subtitle tracks. The hook
    // is fire-and-forget; everything happens on tokio in the backend and
    // never blocks the UI.
    if (isUrl(file)) {
      invoke("arm_post_play_hooks", { url: file }).catch(() => {});
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
    const { file, position } = get();
    if (file && position > 0) {
      // Whether this counts as "finished" (and so should forget its resume
      // point rather than save one) is decided in Rust — see
      // `db::remember_position`. Keeping the rule in one place stops the
      // GUI and the CLI from disagreeing about what a watched file is.
      if (!useIncognitoStore.getState().enabled) {
        invoke("save_position", { path: file, position }).catch(() => {});
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
    lastLocalChange = Date.now();
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
    lastLocalChange = Date.now();
    set({ speed: rate });
    try {
      await invoke("player_set_speed", { rate });
    } catch (e) {
      console.error("player_set_speed failed:", e);
    }
  },

  refreshChapters: async () => {
    try {
      const chapters = await invoke<Chapter[]>("chapter_list");
      set({ chapters });
    } catch (e) {
      console.error("chapter_list failed:", e);
      set({ chapters: [] });
    }
  },

  seekChapter: async (index: number) => {
    try {
      await invoke("chapter_seek", { index });
      await get().refreshChapters();
    } catch (e) {
      console.error("chapter_seek failed:", e);
    }
  },

  stepChapter: async (delta: number) => {
    try {
      await invoke("chapter_step", { delta });
      await get().refreshChapters();
    } catch (e) {
      // Files without chapters reject the call; that's expected, not an
      // error worth surfacing to the user.
      console.debug("chapter_step unavailable:", e);
    }
  },

  abLoopAction: async (action) => {
    try {
      const next = await invoke<AbLoop>("ab_loop", { action });
      set({ abLoop: next });
      return next;
    } catch (e) {
      console.error("ab_loop failed:", e);
      return get().abLoop;
    }
  },

  setSubDelay: async (seconds: number, relative = false) => {
    try {
      const res = await invoke<{ seconds: number }>("subtitle_delay", {
        seconds,
        relative,
      });
      set({ subDelay: res.seconds });
      return res.seconds;
    } catch (e) {
      console.error("subtitle_delay failed:", e);
      return get().subDelay;
    }
  },

  setAudioDelay: async (seconds: number, relative = false) => {
    try {
      const res = await invoke<{ seconds: number }>("audio_delay", {
        seconds,
        relative,
      });
      set({ audioDelay: res.seconds });
      return res.seconds;
    } catch (e) {
      console.error("audio_delay failed:", e);
      return get().audioDelay;
    }
  },

  stepFrame: async (delta: number) => {
    try {
      await invoke("frame_step", { delta });
    } catch (e) {
      console.error("frame_step failed:", e);
    }
  },

  // Bookmarks are re-read rather than tracked incrementally: the CLI and
  // MCP write to the same table, so the list on screen is only ever a
  // snapshot. Every mutation here ends in a refresh for that reason.
  refreshNowPlaying: async () => {
    try {
      const np = await invoke<NowPlaying>("now_playing", { cover: true });
      set({ nowPlaying: np });
    } catch (e) {
      // No tags is the normal case for a video file, and a missing cover
      // is not worth a toast — music mode renders a placeholder.
      console.debug("now_playing failed:", e);
      set({ nowPlaying: null });
    }
  },

  refreshBookmarks: async () => {
    if (!get().file) {
      set({ bookmarks: [] });
      return;
    }
    try {
      set({ bookmarks: await invoke<Bookmark[]>("bookmark_list", {}) });
    } catch (e) {
      console.debug("bookmark_list unavailable:", e);
      set({ bookmarks: [] });
    }
  },

  addBookmark: async (name?: string) => {
    try {
      const bookmark = await invoke<Bookmark>("bookmark_add", {
        name: name ?? null,
        position: null,
        file: null,
      });
      await get().refreshBookmarks();
      return bookmark;
    } catch (e) {
      console.error("bookmark_add failed:", e);
      return null;
    }
  },

  gotoBookmark: async (bookmark: Bookmark) => {
    if (bookmark.path === get().file) {
      await get().seek(bookmark.position);
      return;
    }
    await get().play(bookmark.path, null, bookmark.position);
  },

  renameBookmark: async (id: number, name: string | null) => {
    try {
      await invoke("bookmark_rename", { id, name });
      await get().refreshBookmarks();
    } catch (e) {
      console.error("bookmark_rename failed:", e);
    }
  },

  removeBookmark: async (id: number) => {
    try {
      await invoke("bookmark_remove", { id });
      await get().refreshBookmarks();
    } catch (e) {
      console.error("bookmark_remove failed:", e);
    }
  },
}));

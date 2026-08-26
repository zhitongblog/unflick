import { useEffect, useCallback, useMemo, useState, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import PlayerBar from "./components/Player/PlayerBar";
import MusicMode from "./components/MusicMode";
import TitleBar from "./components/TitleBar";
import LibraryPanel from "./components/Library/LibraryPanel";
import PlaylistPanel from "./components/Playlist/PlaylistPanel";
import ContextMenu, { type ContextMenuEntry } from "./components/ContextMenu";
import ClipDialog from "./components/ClipDialog";
import UrlDialog from "./components/UrlDialog";
import OnlineSubtitles from "./components/OnlineSubtitles";
import { FIND_SUBTITLES_EVENT } from "./lib/subtitleSearch";
import RecentFiles from "./components/RecentFiles";
import SettingsPanel from "./components/Settings/SettingsPanel";
import { usePlayerStore, type BackendStatus } from "./stores/playerStore";
import { useKeybindStore } from "./stores/keybindStore";
import { useMousebindStore } from "./stores/mousebindStore";
import { useLibraryStore } from "./stores/libraryStore";
import { usePlaylistStore } from "./stores/playlistStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useIncognitoStore } from "./stores/incognitoStore";
import { checkForUpdate, type UpdateResult } from "./lib/checkUpdate";
import { formatDelay, formatSpeed, formatTime } from "./lib/format";
import { eventToKey } from "./lib/keys";
import { createGestureTracker, createWheelAccumulator } from "./lib/gesture";
import { useStrings } from "./i18n/utils";

/**
 * Fire a transient toast through the app's toast bus. Hotkeys that change
 * something invisible — subtitle delay, loop points — need the confirmation;
 * otherwise pressing `z` looks like nothing happened.
 */
function showToast(message: string, kind: "success" | "error" = "success") {
  window.dispatchEvent(
    new CustomEvent("unflick:toast", { detail: { kind, message } }),
  );
}

async function openFileDialog() {
  const result = await invoke<{ path: string | null }>("open_file_dialog");
  return result.path;
}

function App() {
  const {
    state,
    play,
    pause,
    resume,
    seek,
    position,
    volume,
    setVolume,
    stepChapter,
    abLoopAction,
    setSubDelay,
    setAudioDelay,
    stepFrame,
    speed,
    setSpeed,
    addBookmark,
    nowPlaying,
  } = usePlayerStore();
  // Subtitles are now rendered by mpv natively (ASS/SSA/SRT/PGS), not via
  // <track> in HTML. The store still owns the list for the menu UI.
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const { showPlaylist, togglePlaylist } = usePlaylistStore();
  const { showSettings, toggleSettings, loadSettings, theme } = useSettingsStore();
  const musicModeAuto = useSettingsStore((s) => s.musicModeAuto);
  const incognito = useIncognitoStore((s) => s.enabled);
  const toggleIncognito = useIncognitoStore((s) => s.toggle);
  const [isDragging, setIsDragging] = useState(false);
  // (contextMenu state is declared below alongside the platform branch.)
  const [controlsVisible, setControlsVisible] = useState(true);
  // True while the OS is in fullscreen mode for our window. Drives
  // TitleBar / PlayerBar visibility — in fullscreen we hide them
  // unless the user has just moved the mouse, so the video really
  // does fill the screen.
  const [isFullscreen, setIsFullscreen] = useState(false);
  /**
   * Which shape the window is in. Driven entirely by the backend event, so
   * the button, the hotkey, `unflick window mode` and an MCP call all move
   * the UI the same way — the frontend never decides this on its own.
   */
  const [windowMode, setWindowMode] = useState<"normal" | "pip" | "music">("normal");

  const [showClipDialog, setShowClipDialog] = useState(false);
  const [showUrlDialog, setShowUrlDialog] = useState(false);
  const [showOnlineSubtitles, setShowOnlineSubtitles] = useState(false);

  // Opened from the subtitle menu (React popover on macOS/Linux, native menu
  // on Windows) - see lib/subtitleSearch.ts for why this is an event.
  useEffect(() => {
    const open = () => setShowOnlineSubtitles(true);
    window.addEventListener(FIND_SUBTITLES_EVENT, open);
    return () => window.removeEventListener(FIND_SUBTITLES_EVENT, open);
  }, []);
  const [updateInfo, setUpdateInfo] = useState<UpdateResult | null>(null);
  const [toast, setToast] = useState<{ id: number; kind: "success" | "error"; message: string } | null>(null);
  // Default-player prompt: show once after first launch unless dismissed.
  // Stored in localStorage so it survives reinstalls but not "clear data."
  // Platform branching: a few bits of UX are explicitly different
  // between Windows and macOS. The popup video architecture differs
  // (top-level WS_POPUP on Win vs subview-of-WebView on mac), so React-
  // rendered menus naturally show *above* the popup on mac but *behind*
  // it on Windows. We use this flag to pick between OS-native menus
  // (TrackPopupMenu on Windows) and React popovers (mac/Linux).
  const isWindows = typeof window !== "undefined" && /Win(dows|32|64)/i.test(navigator.userAgent);

  // Default-player banner is Windows-only. The button invokes a
  // Windows-specific Tauri command (open_default_apps_settings) that
  // launches `ms-settings:defaultapps`, which doesn't exist on macOS or
  // Linux. On macOS the equivalent flow is Finder → Right-click → Open
  // With → unflick → Change All, which the user discovers themselves;
  // we don't try to surface a banner about it.
  const [showDefaultPrompt, setShowDefaultPrompt] = useState<boolean>(() => {
    if (typeof window === "undefined") return false;
    if (localStorage.getItem("default-prompt-dismissed")) return false;
    return /Win(dows|32|64)/i.test(navigator.userAgent);
  });
  // Right-click menu state. On Windows we route through the Win32
  // TrackPopupMenu API for z-order reasons (popup is a top-level window
  // above the WebView). On macOS / Linux the popup sits *under* the
  // WebView in z-order, so a React-rendered menu naturally floats above
  // the video — we use the original ContextMenu component there.
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  // Subtitle generation status — persistent banner that stays visible
  // throughout the (multi-minute) whisper run, replacing the toast that
  // auto-dismissed before the user could read it.
  const [genStatus, setGenStatus] = useState<
    | { kind: "running" }
    | { kind: "success" }
    | { kind: "error"; message: string }
    | null
  >(null);
  const t = useStrings();
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const videoRegionRef = useRef<HTMLDivElement | null>(null);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Track fullscreen state so we can hide chrome (TitleBar / PlayerBar)
  // for a real cinema view. The Rust set_fullscreen / exit_fullscreen
  // commands emit `unflick:fullscreen-changed` after toggling — this
  // covers every entry point (F key, double-click, context menu, the
  // PlayerBar button, the native app menu) without each call site
  // having to push state itself. We also seed the initial value at
  // mount in case the window happens to start fullscreen.
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let cancelled = false;
    win.isFullscreen()
      .then((fs) => { if (!cancelled) setIsFullscreen(fs); })
      .catch(() => {});
    const unlisten = listen<boolean>("unflick:fullscreen-changed", (e) => {
      setIsFullscreen(e.payload);
    });
    return () => {
      cancelled = true;
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // Same shape as the fullscreen listener above, and for the same reason:
  // `gui::window` owns the mode and announces it, so the PiP button, the
  // music-mode hotkey, `unflick window mode` and an agent's MCP call all
  // arrive here through one path.
  useEffect(() => {
    const unlisten = listen<string>("unflick:window-mode", (e) => {
      setWindowMode(e.payload as "normal" | "pip" | "music");
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // Open an audio file in music mode without being asked.
  //
  // This is what makes the mode findable: nobody switches to a layout they
  // have not seen. Keyed on the file, so it fires once per track and a user
  // who leaves music mode for this file is not dragged back into it — and
  // never in reverse, because a video that follows an mp3 should not yank
  // the window back while the tags are still being read.
  const autoMusicFor = useRef<string | null>(null);
  useEffect(() => {
    if (!musicModeAuto) return;
    const np = nowPlaying;
    if (!np?.file || np.has_video) return;
    if (autoMusicFor.current === np.file) return;
    autoMusicFor.current = np.file;
    if (windowMode === "normal") {
      invoke("window_mode", { mode: "music" }).catch(console.error);
    }
  }, [nowPlaying, musicModeAuto, windowMode]);

  // Poll mpv for status — position / duration / state. 250 ms gives a smooth
  // playback bar without thrashing CPU. v0.8.x will replace this with mpv
  // observe_property events emitted from Rust, but polling is enough for the
  // initial cutover.
  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        // Typed via the store's own BackendStatus so the poll can't drift
        // from what ingestStatus expects — it grew ab_loop / delay fields
        // in v0.10 and a local copy of the shape would have silently
        // dropped them.
        const s = await invoke<BackendStatus>("player_status");
        if (!cancelled) usePlayerStore.getState().ingestStatus(s);
      } catch {
        // Pre-pipeline-init we get an error; fine, will retry next tick.
      }
    };
    const id = setInterval(poll, 250);
    poll();
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  // Track the position + size of the transparent video region inside the
  // WebView and forward it to the Rust side so the mpv overlay window
  // stays aligned. Uses ResizeObserver for the size and a window resize
  // listener for absolute origin (ResizeObserver doesn't fire on parent-
  // scroll). Coordinates are scaled by devicePixelRatio so the OS-level
  // window math is in physical pixels regardless of system zoom.
  useEffect(() => {
    const el = videoRegionRef.current;
    if (!el) return;

    let last: { x: number; y: number; w: number; h: number } | null = null;
    const sync = () => {
      const rect = el.getBoundingClientRect();
      // We send logical (CSS) pixels — `getBoundingClientRect` reports
      // them — and let each backend convert as needed:
      //   - Windows: SetWindowPos uses physical px on a PerMonitorV2-
      //     aware process, so video/windows.rs scales by GetDpiForWindow
      //     before talking to Win32. Doing the scale there keeps it on
      //     the same monitor as the popup, which matters when the user
      //     drags between monitors with different DPIs.
      //   - macOS: NSView frame is in points (logical), so the values
      //     pass through unchanged.
      //   - Linux: X11 XMoveResizeWindow takes physical px, but GTK's
      //     CSD layout is already physical at our scale, so values
      //     pass through unchanged.
      const x = Math.round(rect.left);
      const y = Math.round(rect.top);
      const w = Math.round(rect.width);
      const h = Math.round(rect.height);
      // Idempotence: ResizeObserver / window-resize listeners fire freely;
      // skip if the rect is unchanged. Each set_geometry call hits Win32
      // SetWindowPos AND signals the render thread to redraw, so spamming
      // it produces visible flicker on the layered popup.
      if (
        last &&
        last.x === x &&
        last.y === y &&
        last.w === w &&
        last.h === h
      ) {
        return;
      }
      last = { x, y, w, h };
      invoke("video_surface_set_geometry", { x, y, w, h }).catch(() => {});
    };

    const ro = new ResizeObserver(sync);
    ro.observe(el);
    window.addEventListener("resize", sync);
    sync();
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", sync);
    };
  }, []);

  // Show the video popup only when (a) a file is loaded, and (b) no
  // panel/dialog/menu is open. The popup is a top-level Win32 window,
  // so it sits *above* the WebView — meaning anything React renders
  // (drop zone when stopped, library/playlist panels, settings/clip/
  // url dialogs, the right-click context menu, popover menus inside
  // the PlayerBar like SubtitleMenu / AudioMenu / VideoFilters /
  // PlaybackControls) is hidden behind it unless we get out of the
  // way. The popover menus close themselves on outside-click, so we
  // proxy them through a custom event the components dispatch.
  const [popoverOpen, setPopoverOpen] = useState(false);
  useEffect(() => {
    const onOpen = () => setPopoverOpen(true);
    const onClose = () => setPopoverOpen(false);
    window.addEventListener("unflick:popover-open", onOpen);
    window.addEventListener("unflick:popover-close", onClose);
    return () => {
      window.removeEventListener("unflick:popover-open", onOpen);
      window.removeEventListener("unflick:popover-close", onClose);
    };
  }, []);
  const desiredVisibleRef = useRef(false);
  const lastVisibleRef = useRef<boolean | null>(null);
  useEffect(() => {
    // Full-screen modals always hide the popup (popup would bleed
    // through the modal otherwise). Popovers (Subtitle/Audio/Filters)
    // and the React context menu only need the popup hidden on
    // **Windows**, where the popup is a top-level WS_POPUP rendered
    // *above* the WebView. On macOS / Linux the popup is a subview
    // *below* the WebView, so React-rendered menus naturally float
    // over the video — no hide needed.
    const blocking =
      showSettings ||
      showClipDialog ||
      showUrlDialog ||
      showOnlineSubtitles ||
      // Music mode has no picture to show, and leaving the surface up
      // would park a black rectangle over the cover art.
      windowMode === "music" ||
      (isWindows && (popoverOpen || contextMenu !== null));
    const visible = state !== "stopped" && !blocking;
    desiredVisibleRef.current = visible;
    if (lastVisibleRef.current === visible) return;
    lastVisibleRef.current = visible;
    invoke("video_surface_set_visible", { visible }).catch(() => {});
  }, [state, showSettings, showClipDialog, showUrlDialog, showOnlineSubtitles, popoverOpen, contextMenu, isWindows, windowMode]);

  // Re-assert popup visibility on a couple of "things may have gone
  // wrong" triggers. The Rust side hides the popup when the main window
  // is minimized; the visibility effect's idempotence cache keeps it
  // from re-pushing the same `visible:true` value, so we need an
  // explicit poke. The `main-restored` event fires from on_window_event
  // when the main window comes back from a minimize / size-zero state.
  useEffect(() => {
    const reassert = () => {
      if (desiredVisibleRef.current) {
        lastVisibleRef.current = null; // bust the cache so next push lands
        invoke("video_surface_set_visible", { visible: true }).catch(() => {});
      }
    };
    const unlisten = listen("main-restored", reassert);
    window.addEventListener("focus", reassert);
    return () => {
      window.removeEventListener("focus", reassert);
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // Subtitle generation lifecycle — listen for events from PlayerBar's
  // generate handler. Keep the banner up the entire time so the user
  // doesn't think the click was a no-op while whisper is grinding.
  useEffect(() => {
    const onStart = () => setGenStatus({ kind: "running" });
    const onSuccess = () => {
      setGenStatus({ kind: "success" });
      setTimeout(() => setGenStatus((s) => (s?.kind === "success" ? null : s)), 4000);
    };
    const onError = (e: Event) => {
      const detail = (e as CustomEvent<{ message: string }>).detail;
      setGenStatus({ kind: "error", message: detail?.message ?? "" });
      setTimeout(() => setGenStatus((s) => (s?.kind === "error" ? null : s)), 8000);
    };
    window.addEventListener("unflick:gen-start", onStart);
    window.addEventListener("unflick:gen-success", onSuccess);
    window.addEventListener("unflick:gen-error", onError as EventListener);
    return () => {
      window.removeEventListener("unflick:gen-start", onStart);
      window.removeEventListener("unflick:gen-success", onSuccess);
      window.removeEventListener("unflick:gen-error", onError as EventListener);
    };
  }, []);

  // Page Visibility API: the WebView's `document.hidden` flips to true
  // whenever the OS considers the window non-visible — minimized,
  // taskbar-hidden, etc. Tauri's WindowEvent::Resized doesn't always
  // fire (0,0) on minimize across Win11 builds, so we use this as the
  // primary minimize/restore signal. Hide popup when hidden, replay
  // desired visibility when visible.
  useEffect(() => {
    const onVisChange = () => {
      if (document.hidden) {
        lastVisibleRef.current = false;
        invoke("video_surface_set_visible", { visible: false }).catch(() => {});
      } else if (desiredVisibleRef.current) {
        lastVisibleRef.current = null;
        invoke("video_surface_set_visible", { visible: true }).catch(() => {});
      }
    };
    document.addEventListener("visibilitychange", onVisChange);
    return () => document.removeEventListener("visibilitychange", onVisChange);
  }, []);

  const handleOpenFile = useCallback(async () => {
    const path = await openFileDialog();
    if (path) play(path);
  }, [play]);

  const handleAddFilesToPlaylist = useCallback(async () => {
    const result = await invoke<{ paths: string[] }>("open_files_dialog");
    if (!result.paths.length) return;
    const { add, togglePlaylist: toggle, showPlaylist: visible } =
      usePlaylistStore.getState();
    for (const p of result.paths) {
      await add(p);
    }
    if (!visible) toggle();
  }, []);

  const captureScreenshot = useCallback(async () => {
    const { state, file: currentFile } = usePlayerStore.getState();
    if (state === "stopped") {
      console.warn("screenshot skipped: nothing playing");
      return;
    }

    // Build a filesystem-friendly default name: <video-stem>-<timestamp>.png
    // when a file is loaded, falling back to "unflick-screenshot-<ts>.png".
    const now = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    const ts = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}-${pad(now.getMinutes())}-${pad(now.getSeconds())}`;
    const stem = currentFile ? currentFile.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "unflick" : "unflick";
    const safeStem = stem.replace(/[<>:"/\\|?*]/g, "_");
    const defaultName = `${safeStem}-${ts}.png`;

    // Silent mode: screenshotDir configured → mpv writes directly, no dialog.
    const screenshotDir = useSettingsStore.getState().screenshotDir;
    if (screenshotDir) {
      const sep = screenshotDir.includes("\\") || /^[A-Za-z]:/.test(screenshotDir) ? "\\" : "/";
      const fullPath = screenshotDir.replace(/[\\/]+$/, "") + sep + defaultName;
      try {
        await invoke("player_screenshot", { output: fullPath });
        window.dispatchEvent(
          new CustomEvent("unflick:toast", {
            detail: { kind: "success", message: `Saved: ${defaultName}` },
          }),
        );
        return;
      } catch (e) {
        console.error("silent screenshot failed, falling back to dialog:", e);
      }
    }

    // Linux: skip the GTK file dialog. Opening a modal save dialog from a
    // Tauri command on a GTK session interrupts our X11 child window's
    // input handling — mpv's vo=x11 sometimes ends up paused after the
    // dialog closes, and the user has reported "screenshot stops
    // playback". Save straight to ~/Pictures/unflick/ with a toast
    // instead. The path is created lazily by `player_screenshot` if
    // missing.
    const isLinuxUA = typeof navigator !== "undefined" && /Linux/.test(navigator.userAgent);
    if (isLinuxUA) {
      try {
        const resp = await invoke<{ path: string }>("player_screenshot", {
          output: defaultName, // Rust resolves to ~/Pictures/unflick/<name>.
        });
        window.dispatchEvent(
          new CustomEvent("unflick:toast", {
            detail: { kind: "success", message: `Saved: ${resp.path}` },
          }),
        );
      } catch (e) {
        console.error("linux screenshot failed:", e);
        window.dispatchEvent(
          new CustomEvent("unflick:toast", {
            detail: { kind: "error", message: `Screenshot failed: ${e}` },
          }),
        );
      }
      return;
    }

    // Dialog mode (Windows/macOS): ask the user where to save, then mpv writes there.
    const result = await invoke<{ path: string | null }>("save_file_dialog", {
      defaultName,
    });
    if (!result.path) return;
    try {
      await invoke("player_screenshot", { output: result.path });
      console.log("screenshot saved:", result.path);
    } catch (e) {
      console.error("save screenshot failed:", e);
    }
  }, []);

  // Auto-hide controls during playback
  const showControls = useCallback(() => {
    setControlsVisible(true);
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => {
      setControlsVisible(false);
    }, 1000);
  }, []);

  const handleMouseMove = useCallback(() => {
    if (state === "playing") {
      showControls();
    }
  }, [state, showControls]);

  // When state changes: if not playing, always show controls and cancel timer
  useEffect(() => {
    if (state !== "playing") {
      if (hideTimer.current) clearTimeout(hideTimer.current);
      setControlsVisible(true);
    } else {
      // Just started playing — start the hide timer
      showControls();
    }
    return () => {
      if (hideTimer.current) clearTimeout(hideTimer.current);
    };
  }, [state, showControls]);

  // Volume before the last mute, so a second press restores it instead of
  // jumping to a default.
  const lastVolume = useRef(100);

  // mpv's own speed ladder. Stepping through fixed rates beats a fixed
  // increment, which turns into either useless nudges or huge jumps
  // depending on where you start.
  const SPEED_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3, 4];

  const adjustSpeed = useCallback(
    (direction: number) => {
      const nearest = SPEED_STEPS.reduce((best, s) =>
        Math.abs(s - speed) < Math.abs(best - speed) ? s : best,
      );
      const next =
        SPEED_STEPS[
          Math.max(0, Math.min(SPEED_STEPS.length - 1, SPEED_STEPS.indexOf(nearest) + direction))
        ];
      setSpeed(next);
      showToast(`${t.hotkeys.speed} ${formatSpeed(next)}`);
    },
    [speed, setSpeed, t],
  );

  // The ladder is for getting somewhere fast; this is for landing on the rate
  // that actually suits the speaker. Same keys, different modifier.
  const FINE_SPEED_STEP = 0.05;

  const nudgeSpeed = useCallback(
    (direction: number) => {
      const base = Math.round(speed / FINE_SPEED_STEP) * FINE_SPEED_STEP;
      const next = Math.min(
        SPEED_STEPS[SPEED_STEPS.length - 1],
        Math.max(SPEED_STEPS[0], base + direction * FINE_SPEED_STEP),
      );
      setSpeed(parseFloat(next.toFixed(2)));
      showToast(`${t.hotkeys.speed} ${formatSpeed(next)}`);
    },
    [speed, setSpeed, t],
  );

  const nudgeSubDelay = useCallback(
    (delta: number) => {
      setSubDelay(delta, true).then((value) =>
        showToast(`${t.hotkeys.subtitleDelay} ${formatDelay(value)}`),
      );
    },
    [setSubDelay, t],
  );

  const nudgeAudioDelay = useCallback(
    (delta: number) => {
      setAudioDelay(delta, true).then((value) =>
        showToast(`${t.hotkeys.audioDelay} ${formatDelay(value)}`),
      );
    },
    [setAudioDelay, t],
  );

  // ── Keyboard shortcuts ──────────────────────────────────────────────
  // Every chord resolves through the binding table rather than a switch
  // on `e.key`, so the same actions are listable (`unflick keybind list`)
  // and rebindable. `core::keybind` owns the catalogue and the defaults;
  // this map only says what each action *does*.
  const actions = useMemo<Record<string, () => void>>(
    () => ({
      play_pause: () => {
        if (state === "playing") pause();
        else if (state === "paused") resume();
      },
      seek_back: () => seek(Math.max(0, position - 5)),
      seek_forward: () => seek(position + 5),
      frame_back: () => { if (state !== "stopped") stepFrame(-1); },
      frame_forward: () => { if (state !== "stopped") stepFrame(1); },
      speed_down: () => adjustSpeed(-1),
      speed_up: () => adjustSpeed(1),
      speed_down_fine: () => nudgeSpeed(-1),
      speed_up_fine: () => nudgeSpeed(1),
      speed_reset: () => {
        setSpeed(1);
        showToast(`${t.hotkeys.speed} ${formatSpeed(1)}`);
      },

      volume_up: () => setVolume(Math.min(150, volume + 5)),
      volume_down: () => setVolume(Math.max(0, volume - 5)),
      mute: () => {
        // No separate mute state to keep in sync: remember the level we
        // came from so the second press restores it.
        if (volume > 0) {
          lastVolume.current = volume;
          setVolume(0);
        } else {
          setVolume(lastVolume.current || 100);
        }
      },

      chapter_next: () => stepChapter(1),
      chapter_prev: () => stepChapter(-1),
      loop_a: () => {
        abLoopAction("a").then((loop) =>
          showToast(`${t.hotkeys.loopStart} ${formatTime(loop.a ?? 0)}`),
        );
      },
      loop_b: () => {
        abLoopAction("b").then((loop) =>
          showToast(
            loop.active
              ? `${t.hotkeys.looping} ${formatTime(loop.a ?? 0)} → ${formatTime(loop.b ?? 0)}`
              : `${t.hotkeys.loopEnd} ${formatTime(loop.b ?? 0)}`,
          ),
        );
      },
      loop_clear: () => {
        abLoopAction("clear").then(() => showToast(t.hotkeys.loopCleared));
      },
      // Saved unnamed: the point of the key is to catch the moment. The
      // bookmark list is where a label gets added afterwards.
      bookmark_add: () => {
        if (state === "stopped") return;
        void addBookmark().then((b) => {
          if (b) showToast(`${t.hotkeys.bookmarked} ${formatTime(b.position)}`);
        });
      },
      toggle_bookmarks: () => {
        window.dispatchEvent(new CustomEvent("unflick:toggle-bookmarks"));
      },

      sub_delay_down: () => nudgeSubDelay(-0.1),
      sub_delay_up: () => nudgeSubDelay(0.1),
      audio_delay_down: () => nudgeAudioDelay(-0.1),
      audio_delay_up: () => nudgeAudioDelay(0.1),

      fullscreen: () => { invoke("set_fullscreen").catch(console.error); },
      pip: () => { invoke("toggle_pip").catch(console.error); },
      music_mode: () => { invoke("toggle_music_mode").catch(console.error); },
      toggle_library: () => toggleLibrary(),
      toggle_playlist: () => togglePlaylist(),

      screenshot: () => { if (state !== "stopped") captureScreenshot(); },
      clip: () => { if (state !== "stopped") setShowClipDialog((v) => !v); },

      open_file: () => handleOpenFile(),
      open_url: () => setShowUrlDialog((v) => !v),
      settings: () => toggleSettings(),
      incognito: () => useIncognitoStore.getState().toggle(),
    }),
    [
      state, pause, resume, seek, position, volume, setVolume, setSpeed,
      stepFrame, stepChapter, abLoopAction, toggleLibrary, togglePlaylist,
      handleOpenFile, toggleSettings, t, adjustSpeed, nudgeSpeed, nudgeSubDelay,
      nudgeAudioDelay, captureScreenshot, addBookmark,
    ],
  );

  // Mouse triggers resolve through the same action map as keys, so a
  // gesture and a shortcut can't drift apart in what they do.
  const runMouseTrigger = useCallback(
    (trigger: string) => {
      const actionId = useMousebindStore.getState().actionFor(trigger);
      if (!actionId) return false;
      const run = actions[actionId];
      if (!run) return false;
      run();
      return true;
    },
    [actions],
  );

  const gesture = useRef(createGestureTracker());
  const wheel = useRef(createWheelAccumulator());
  // Set when a right-drag is recognised, so the context menu that would
  // otherwise fire on the same release is skipped exactly once.
  const suppressMenu = useRef(false);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Never steal a keystroke from a text field.
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement ||
        (e.target instanceof HTMLElement && e.target.isContentEditable)
      ) {
        return;
      }

      // Escape stays hardcoded: it means "dismiss what's open", which is a
      // UI convention rather than a player action. Rebinding it would leave
      // dialogs with no way out.
      if (e.key === "Escape") {
        if (showOnlineSubtitles) setShowOnlineSubtitles(false);
        else if (showClipDialog) setShowClipDialog(false);
        else if (showUrlDialog) setShowUrlDialog(false);
        else invoke("exit_fullscreen").catch(console.error);
        return;
      }

      const chord = eventToKey(e);
      if (!chord) return;

      const actionId = useKeybindStore.getState().actionFor(chord);
      if (!actionId) return;

      const run = actions[actionId];
      if (!run) return;

      e.preventDefault();
      run();
    },
    [actions, showClipDialog, showUrlDialog, showOnlineSubtitles],
  );

  // Initialize mpv player on mount and load persisted settings
  useEffect(() => {
    invoke("player_init").catch(console.error);

    // Load the binding table before anything can be typed at the window.
    // Until it arrives the key handler is inert, which is the right
    // failure mode — better a dead key than the wrong action.
    void useKeybindStore.getState().load();
    void useMousebindStore.getState().load();

    // Re-read it whenever the window comes back to the front. Bindings can
    // also be changed with `unflick keybind set` or by an agent over MCP,
    // and you can only have done that by leaving this window — so focus is
    // exactly the moment the table might be stale.
    // Tauri's own window-focus event, not the DOM `focus` event: the
    // latter only fires when focus moves *within* the page, so activating
    // the window from outside never reached it.
    const focusWatcher = getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void useKeybindStore.getState().load();
        void useMousebindStore.getState().load();
      }
    });

    // If Explorer launched us via a file association, the backend stashed
    // the path on startup. Pull it out and start playback. Single-shot:
    // a second invocation returns null so refreshes don't replay.
    invoke<string | null>("consume_pending_file")
      .then((path) => {
        if (path) play(path);
      })
      .catch(console.error);

    // Quiet background update check. Suppress the banner if the user has
    // already dismissed *this exact version* before — re-show only when a
    // newer version appears.
    checkForUpdate()
      .then((res) => {
        if (!res.hasUpdate || !res.latest) return;
        const dismissed = localStorage.getItem("update-dismissed-version");
        if (dismissed === res.latest) return;
        setUpdateInfo(res);
      })
      .catch(() => {
        /* silent on startup; menu item still lets users retry manually */
      });

    loadSettings().then(async () => {
      // Apply saved volume
      const savedVolume = useSettingsStore.getState().volume;
      if (savedVolume !== undefined && savedVolume !== 100) {
        setVolume(savedVolume);
      }

      // Apply persisted always-on-top preference. The Tauri window
      // doesn't auto-restore this from any config so we tell Rust to
      // re-apply once settings have loaded.
      const aot = useSettingsStore.getState().alwaysOnTop;
      if (aot) {
        invoke("set_always_on_top", { enabled: true }).catch(() => {});
      }

      // Subtitle appearance lives in settings.json but mpv starts at its
      // own defaults every launch, so push the saved values across once
      // settings have loaded.
      const style = useSettingsStore.getState().subtitleStyle;
      for (const [name, value] of Object.entries(style)) {
        invoke("subtitle_style_set", { name, value }).catch(() => {});
      }

      // Auto-detect bundled whisper whenever the local paths are not configured.
      // Previously this only ran when whisperMode === "off", which left users
      // stranded if they had the mode set to "local" but the paths got cleared.
      const s = useSettingsStore.getState();
      if (!s.whisperBinaryPath || !s.whisperModelPath) {
        try {
          const r = await invoke<{
            bundled: boolean;
            whisper_binary?: string;
            model_path?: string;
          }>("check_bundled_whisper");
          if (r.bundled && r.whisper_binary && r.model_path) {
            s.setWhisperMode("local");
            s.setWhisperBinaryPath(r.whisper_binary);
            s.setWhisperModelPath(r.model_path);
            await s.saveSettings();
            console.log("auto-configured bundled whisper");
          }
        } catch (e) {
          console.warn("bundled whisper check failed:", e);
        }
      }
    });

    return () => {
      focusWatcher.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Apply theme data attribute on root
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Listen for screenshot requests from any component (e.g. PlayerBar button)
  useEffect(() => {
    const handler = () => captureScreenshot();
    window.addEventListener("unflick:screenshot", handler);
    return () => window.removeEventListener("unflick:screenshot", handler);
  }, [captureScreenshot]);

  // Toast bus — any component can dispatch `unflick:toast` with a payload
  // and we'll render it bottom-center for ~2.5s. Used for silent screenshot
  // confirmation today; reusable for other quiet-action feedback later.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<{ kind?: "success" | "error"; message: string }>).detail;
      if (!detail?.message) return;
      const id = Date.now();
      setToast({ id, kind: detail.kind ?? "success", message: detail.message });
      setTimeout(() => {
        setToast((cur) => (cur && cur.id === id ? null : cur));
      }, 2500);
    };
    window.addEventListener("unflick:toast", handler);
    return () => window.removeEventListener("unflick:toast", handler);
  }, []);

  // Single-instance plugin: a second launch (e.g. user double-clicks
  // another file in Explorer while we're running) is short-circuited,
  // and Rust forwards the new file path to *us* via this event so it
  // plays in the existing window instead of opening a new one.
  useEffect(() => {
    const unlisten = listen<string>("open-file", (event) => {
      if (event.payload) play(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [play]);

  // Listen for native menu events
  useEffect(() => {
    const unlisten = listen<string>("menu-event", async (event) => {
      const { state: currentState } = usePlayerStore.getState();
      switch (event.payload) {
        case "open":
          handleOpenFile();
          break;
        case "open_url":
          setShowUrlDialog(true);
          break;
        case "play_pause":
          if (currentState === "playing") usePlayerStore.getState().pause();
          else if (currentState === "paused") usePlayerStore.getState().resume();
          break;
        case "stop":
          usePlayerStore.getState().stop();
          break;
        case "fullscreen":
          invoke("set_fullscreen").catch(console.error);
          break;
        case "pip":
          invoke("toggle_pip").catch(console.error);
          break;
        case "library":
          useLibraryStore.getState().toggleLibrary();
          break;
        case "playlist":
          usePlaylistStore.getState().togglePlaylist();
          break;
        case "volume_up":
          usePlayerStore.getState().setVolume(
            Math.min(150, usePlayerStore.getState().volume + 5)
          );
          break;
        case "volume_down":
          usePlayerStore.getState().setVolume(
            Math.max(0, usePlayerStore.getState().volume - 5)
          );
          break;
        case "check_updates": {
          // Manual menu trigger: always tell the user the result, even when
          // they're up to date or the network call failed.
          const result = await checkForUpdate();
          setUpdateInfo(result);
          if (result.error) {
            alert(t.update.checkFailed);
          } else if (result.hasUpdate) {
            const open = confirm(
              t.update.newVersionPrompt
                .replace("{latest}", result.latest ?? "")
                .replace("{current}", result.current),
            );
            if (open) window.open(result.url, "_blank");
          } else {
            alert(t.update.upToDate.replace("{current}", result.current));
          }
          break;
        }
        case "about":
          // Could show an about dialog in the future
          console.log("unflick v0.1.0 — A video player for humans and AI");
          break;
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [handleOpenFile]);

  // Tauri native drag and drop
  useEffect(() => {
    const webview = getCurrentWebviewWindow();
    const unlisten = webview.onDragDropEvent((event) => {
      if (event.payload.type === "over") {
        setIsDragging(true);
      } else if (event.payload.type === "leave") {
        setIsDragging(false);
      } else if (event.payload.type === "drop") {
        setIsDragging(false);
        const paths = event.payload.paths;
        if (paths.length > 0) {
          play(paths[0]);
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [play]);

  // Context menu items
  const contextMenuItems: ContextMenuEntry[] = [
    {
      label: t.context.openFile,
      shortcut: "Ctrl+O",
      onClick: handleOpenFile,
    },
    {
      label: t.context.openUrl,
      shortcut: "Ctrl+U",
      onClick: () => setShowUrlDialog(true),
    },
    {
      label: t.context.addToPlaylist,
      onClick: handleAddFilesToPlaylist,
    },
    { separator: true },
    {
      label: state === "playing" ? t.context.pause : t.context.play,
      shortcut: "Space",
      onClick: () => {
        if (state === "playing") pause();
        else if (state === "paused") resume();
      },
      disabled: state === "stopped",
    },
    {
      label: t.context.stop,
      onClick: () => usePlayerStore.getState().stop(),
      disabled: state === "stopped",
    },
    { separator: true },
    {
      label: t.context.screenshot,
      shortcut: "S",
      onClick: () => captureScreenshot(),
      disabled: state === "stopped",
    },
    {
      label: t.context.clip,
      shortcut: "C",
      onClick: () => setShowClipDialog(true),
      disabled: state === "stopped",
    },
    { separator: true },
    {
      label: t.context.fullscreen,
      shortcut: "F",
      onClick: () => invoke("set_fullscreen").catch(console.error),
    },
    {
      label: t.context.pip,
      shortcut: "P",
      onClick: () => invoke("toggle_pip").catch(console.error),
    },
    {
      label: t.context.openLibrary,
      shortcut: "L",
      onClick: toggleLibrary,
    },
    {
      label: t.context.openPlaylist,
      shortcut: "N",
      onClick: togglePlaylist,
    },
    { separator: true },
    {
      label: incognito ? t.context.incognitoOn : t.context.incognito,
      shortcut: "Ctrl+Shift+P",
      onClick: toggleIncognito,
    },
    {
      label: t.context.clearPlaylist,
      onClick: () => usePlaylistStore.getState().clear(),
    },
    { separator: true },
    {
      label: t.context.settings,
      shortcut: "Ctrl+,",
      onClick: toggleSettings,
    },
  ];

  const handleContextMenu = useCallback(async (e: React.MouseEvent) => {
    // Don't override the native context menu (paste/copy) when the user
    // right-clicks inside an input or contenteditable element
    const t = e.target as HTMLElement;
    if (
      t.tagName === "INPUT" ||
      t.tagName === "TEXTAREA" ||
      t.isContentEditable
    ) {
      return;
    }
    e.preventDefault();

    // Platform branch:
    //  - Windows: Win32 TrackPopupMenu — the video popup is a top-level
    //    window above the WebView, so React menus render *behind* it.
    //    OS menus float above every app window naturally.
    //  - macOS / Linux: video popup is a *subview below the WebView*, so
    //    a React-rendered menu in the WebView naturally floats above
    //    the video. Use the original ContextMenu component.
    if (!isWindows) {
      setContextMenu({ x: e.clientX, y: e.clientY });
      return;
    }

    const items = contextMenuItems.map((item) =>
      "separator" in item && item.separator
        ? { label: "", separator: true, disabled: false }
        : {
            label: (item as { label: string }).label,
            separator: false,
            disabled: (item as { disabled?: boolean }).disabled ?? false,
          }
    );
    try {
      const selected = await invoke<number | null>(
        "show_native_context_menu",
        { items, x: e.screenX, y: e.screenY }
      );
      if (selected != null) {
        const item = contextMenuItems[selected];
        if (item && !("separator" in item && item.separator)) {
          (item as { onClick: () => void }).onClick();
        }
      }
    } catch (err) {
      console.error("[contextmenu] native menu failed:", err);
    }
  }, [contextMenuItems, isWindows]);

  return (
    <div
      className={`flex h-full flex-col ${state === "playing" && !controlsVisible ? "cursor-none" : ""}`}
      style={{ backgroundColor: state === "stopped" ? "var(--bg-primary, #030712)" : "transparent" }}
      onMouseMove={handleMouseMove}
    >
      {/* Custom title bar. Hidden in fullscreen so the video really
          fills the screen. We use conditional rendering (not opacity)
          so the bar is removed from the layout — that makes the
          flex-1 video region expand into the chrome's old space, which
          fires our ResizeObserver and grows the mpv popup to match.
          Opacity:0 would leave a transparent gap and let the OS
          desktop bleed through under the popup. */}
      {(!isFullscreen || controlsVisible) && <TitleBar />}

      {/* Update available banner */}
      <AnimatePresence>
        {updateInfo?.hasUpdate && updateInfo.latest && (
          <motion.div
            initial={{ y: -40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -40, opacity: 0 }}
            transition={{ duration: 0.25 }}
            className="flex items-center gap-3 px-4 py-2 text-sm bg-gradient-to-r from-violet-600/90 to-pink-600/90 text-white"
          >
            <span className="font-medium">unflick v{updateInfo.latest}</span>
            <span className="text-white/80">{t.update.available} — {t.update.currentVersion.replace("{current}", updateInfo.current)}.</span>
            <button
              type="button"
              onClick={() => updateInfo.url && window.open(updateInfo.url, "_blank")}
              className="ml-auto px-3 py-0.5 rounded-md bg-white/15 hover:bg-white/25 transition text-xs font-medium"
            >
              {t.update.download}
            </button>
            <button
              type="button"
              aria-label="Dismiss"
              onClick={() => {
                if (updateInfo.latest) {
                  localStorage.setItem("update-dismissed-version", updateInfo.latest);
                }
                setUpdateInfo(null);
              }}
              className="text-white/70 hover:text-white transition"
            >
              <svg className="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                <path d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z" />
              </svg>
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Subtitle generation status — persistent until success/error
          dismissal. Whisper runs can take minutes; a transient toast
          would auto-hide and leave the user wondering whether the click
          even worked. */}
      <AnimatePresence>
        {genStatus && (
          <motion.div
            initial={{ y: -40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -40, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className={`flex items-center gap-3 px-4 py-2 text-sm text-white ${
              genStatus.kind === "running"
                ? "bg-violet-600/95"
                : genStatus.kind === "success"
                ? "bg-emerald-600/95"
                : "bg-red-600/95"
            }`}
          >
            {genStatus.kind === "running" && (
              <>
                <span className="h-3 w-3 animate-spin rounded-full border-2 border-white border-t-transparent" />
                <span>Generating subtitles… (whisper, may take several minutes)</span>
              </>
            )}
            {genStatus.kind === "success" && (
              <>
                <svg className="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path d="M16.7 5.3a1 1 0 0 1 0 1.4l-7.5 7.5a1 1 0 0 1-1.4 0L3.3 9.7a1 1 0 0 1 1.4-1.4L8.5 12 15.3 5.3a1 1 0 0 1 1.4 0z"/></svg>
                <span>Subtitles generated and loaded</span>
              </>
            )}
            {genStatus.kind === "error" && (
              <>
                <svg className="w-4 h-4 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor"><path d="M10 2a8 8 0 1 1 0 16 8 8 0 0 1 0-16zm-1 4v5h2V6H9zm0 7v2h2v-2H9z"/></svg>
                <span className="truncate">
                  Subtitle generation failed:{" "}
                  {genStatus.message.length > 100
                    ? genStatus.message.slice(0, 100) + "…"
                    : genStatus.message}
                </span>
                <button
                  type="button"
                  aria-label="Dismiss"
                  onClick={() => setGenStatus(null)}
                  className="ml-auto text-white/70 hover:text-white"
                >
                  <svg className="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z"/></svg>
                </button>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* Default-player prompt — first-launch banner offering to make unflick
          the default for video extensions. Tauri registers the file
          associations at install time, but Windows still requires the user
          to confirm via Settings → Default apps before they take effect. */}
      <AnimatePresence>
        {showDefaultPrompt && (
          <motion.div
            initial={{ y: -40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -40, opacity: 0 }}
            transition={{ duration: 0.25 }}
            className="flex items-center gap-3 px-4 py-2 text-sm bg-gradient-to-r from-purple-600/85 to-fuchsia-600/85 text-white"
          >
            <span className="font-medium">{t.settings.fileAssoc.section}</span>
            <span className="text-white/80 truncate">
              {t.settings.fileAssoc.body}
            </span>
            <button
              type="button"
              className="ml-auto whitespace-nowrap px-3 py-0.5 rounded-md bg-white/15 hover:bg-white/25 transition text-xs font-medium"
              onClick={() => {
                invoke("open_default_apps_settings").catch((e) => console.error(e));
                localStorage.setItem("default-prompt-dismissed", "1");
                setShowDefaultPrompt(false);
              }}
            >
              {t.settings.fileAssoc.button}
            </button>
            <button
              type="button"
              aria-label="Dismiss"
              onClick={() => {
                localStorage.setItem("default-prompt-dismissed", "1");
                setShowDefaultPrompt(false);
              }}
              className="text-white/70 hover:text-white transition"
            >
              <svg className="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                <path d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z" />
              </svg>
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Video area / drop zone */}
      <div
        className="relative flex flex-1 items-center justify-center overflow-hidden"
        onContextMenu={(e) => {
          // A recognised right-drag already did something; opening the menu
          // on the same release would be a second, unasked-for action.
          if (suppressMenu.current) {
            suppressMenu.current = false;
            e.preventDefault();
            return;
          }
          void handleContextMenu(e);
        }}
        onWheel={(e) => {
          if (state === "stopped") return;
          const t = e.target as HTMLElement;
          if (t.closest("button, a, input, [data-no-toggle]")) return;
          const { steps, direction } = wheel.current.push(e.deltaY);
          for (let i = 0; i < steps; i++) {
            runMouseTrigger(direction === "up" ? "wheel_up" : "wheel_down");
          }
        }}
        onMouseDown={(e) => {
          if (e.button === 2) {
            gesture.current.begin(e.clientX, e.clientY);
          }
        }}
        onMouseUp={(e) => {
          if (e.button === 1) {
            // Middle click. Handled on mouseup so it can't fight the
            // autoscroll affordance some mice put on mousedown.
            const t = e.target as HTMLElement;
            if (t.closest("button, a, input, [data-no-toggle]")) return;
            e.preventDefault();
            runMouseTrigger("middle_click");
            return;
          }
          if (e.button === 2) {
            const direction = gesture.current.end(e.clientX, e.clientY);
            if (direction && runMouseTrigger(`gesture_${direction}`)) {
              suppressMenu.current = true;
            }
          }
        }}
        onMouseLeave={() => {
          // Releasing outside the video area never reaches our mouseup, so
          // a half-finished stroke would otherwise fire on the next one.
          gesture.current.cancel();
          wheel.current.reset();
        }}
        onClick={(e) => {
          if (state === "stopped") return;
          const t = e.target as HTMLElement;
          if (t.closest("button, a, input, [data-no-toggle]")) return;
          // Defer the click action: if a second click arrives within
          // ~240ms it's a double-click, and we cancel this one.
          if (clickTimer.current) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
          }
          clickTimer.current = setTimeout(() => {
            clickTimer.current = null;
            runMouseTrigger("click");
          }, 240);
        }}
        onDoubleClick={(e) => {
          const t = e.target as HTMLElement;
          if (t.closest("button, a, input, [data-no-toggle]")) return;
          if (clickTimer.current) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
          }
          runMouseTrigger("double_click");
        }}
      >
        {/* Drop overlay */}
        <AnimatePresence>
          {isDragging && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="absolute inset-0 z-10 flex items-center justify-center backdrop-blur-md"
              style={{ backgroundColor: "rgba(0,0,0,0.75)" }}
            >
              <div className="gradient-border rounded-2xl px-12 py-8">
                <p className="idle-title text-xl font-bold">
                  Drop to play
                </p>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Music mode replaces the video area outright — there is no
            picture behind it to leave a gap for. */}
        {windowMode === "music" && (
          <div className="absolute inset-0 z-20">
            <MusicMode />
          </div>
        )}

        {/* Idle state */}
        {state === "stopped" && windowMode !== "music" && (
          <div className="absolute inset-0 flex items-center justify-center overflow-hidden select-none">
            {/* Animated background orbs */}
            <div
              className="pointer-events-none absolute"
              style={{
                width: "600px",
                height: "600px",
                left: "calc(50% - 400px)",
                top: "calc(50% - 350px)",
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(124,58,237,0.12) 0%, transparent 65%)",
                animation: "orb-drift-a 20s ease-in-out infinite",
                filter: "blur(40px)",
              }}
            />
            <div
              className="pointer-events-none absolute"
              style={{
                width: "500px",
                height: "500px",
                left: "calc(50% + 80px)",
                top: "calc(50% - 200px)",
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(219,39,119,0.1) 0%, transparent 65%)",
                animation: "orb-drift-b 24s ease-in-out infinite",
                filter: "blur(40px)",
              }}
            />
            <div
              className="pointer-events-none absolute"
              style={{
                width: "350px",
                height: "350px",
                left: "calc(50% - 50px)",
                top: "calc(50% + 50px)",
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(147,51,234,0.08) 0%, transparent 65%)",
                animation: "orb-drift-c 16s ease-in-out infinite",
                filter: "blur(50px)",
              }}
            />

            {/* Center content */}
            <div className="relative z-10 flex flex-col items-center gap-6">
              <h1 className="idle-title idle-fade-in text-6xl font-extrabold">
                unflick
              </h1>
              <p className="idle-fade-in-delay text-[13px] font-normal tracking-wide text-white/25">
                {t.dropZone.title} · {t.dropZone.subtitle}
              </p>
              <button
                onClick={handleOpenFile}
                className="idle-open-btn idle-fade-in-delay-2 mt-1 rounded-xl px-8 py-2.5 text-[13px] font-semibold text-white transition-all duration-200 active:scale-95"
                style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
              >
                {t.dropZone.openFile}
              </button>

              <RecentFiles />
            </div>
          </div>
        )}

        {/* Punch-through video region.
            mpv renders to a top-level overlay popup; this div's
            getBoundingClientRect drives where Rust positions that
            popup. Side panels (Library / Playlist) shrink the rect
            instead of overlapping it, so video stays visible while
            the user browses. ResizeObserver picks up width changes
            and forwards them to Rust automatically. */}
        <div
          ref={videoRegionRef}
          className="absolute h-full"
          style={{
            top: 0,
            bottom: 0,
            left: showLibrary ? 320 : 0,
            right: showPlaylist ? 288 : 0,
            background: "transparent",
          }}
          aria-label="video region"
        />


      </div>

      {/* Modals — rendered at the App root so they're never clipped by the
          video area's overflow-hidden. They each use fixed inset-0 internally. */}
      {showClipDialog && (
        <ClipDialog onClose={() => setShowClipDialog(false)} />
      )}
      {showOnlineSubtitles && (
        <OnlineSubtitles onClose={() => setShowOnlineSubtitles(false)} />
      )}
      {showUrlDialog && (
        <UrlDialog onClose={() => setShowUrlDialog(false)} />
      )}
      {showSettings && (
        <SettingsPanel onClose={toggleSettings} />
      )}

      {/* Toast — bottom-center, auto-dismiss. Used for silent-action feedback
          (currently silent screenshot confirmation). */}
      <AnimatePresence>
        {toast && (
          <motion.div
            key={toast.id}
            initial={{ y: 32, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 16, opacity: 0 }}
            transition={{ duration: 0.18 }}
            className="pointer-events-none fixed bottom-20 left-1/2 -translate-x-1/2 z-50"
          >
            <div className={`pointer-events-auto rounded-xl border px-4 py-2.5 text-sm font-medium shadow-2xl backdrop-blur-md ${
              toast.kind === "error"
                ? "border-red-500/30 bg-red-950/70 text-red-100"
                : "border-white/10 bg-zinc-900/90 text-white"
            }`}>
              {toast.message}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Right-click menu — Windows uses Win32 TrackPopupMenu inline in
          handleContextMenu; macOS / Linux fall back to this React component
          (which naturally renders above the popup since the popup is a
          subview *below* the WKWebView on those platforms). */}
      {!isWindows && contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Library panel — slides from left */}
      <AnimatePresence>
        {showLibrary && <LibraryPanel />}
      </AnimatePresence>

      {/* Playlist panel — slides from right */}
      <AnimatePresence>
        {showPlaylist && <PlaylistPanel />}
      </AnimatePresence>

      {/* Player bar — visible at all times outside fullscreen. In
          fullscreen we follow the same visibility rule as the title
          bar so the video fills the screen until the user moves the
          mouse. */}
      {(!isFullscreen || controlsVisible) && windowMode !== "music" && <PlayerBar />}
    </div>
  );
}

export default App;

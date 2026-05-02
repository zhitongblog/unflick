import { useEffect, useCallback, useState, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import PlayerBar from "./components/Player/PlayerBar";
import TitleBar from "./components/TitleBar";
import LibraryPanel from "./components/Library/LibraryPanel";
import PlaylistPanel from "./components/Playlist/PlaylistPanel";
import { type ContextMenuEntry } from "./components/ContextMenu";
import ClipDialog from "./components/ClipDialog";
import UrlDialog from "./components/UrlDialog";
import SettingsPanel from "./components/Settings/SettingsPanel";
import { usePlayerStore } from "./stores/playerStore";
import { useLibraryStore } from "./stores/libraryStore";
import { usePlaylistStore } from "./stores/playlistStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useIncognitoStore } from "./stores/incognitoStore";
import { checkForUpdate, type UpdateResult } from "./lib/checkUpdate";
import { useStrings } from "./i18n/utils";

async function openFileDialog() {
  const result = await invoke<{ path: string | null }>("open_file_dialog");
  return result.path;
}

function App() {
  const { state, play, pause, resume, seek, position, volume, setVolume } =
    usePlayerStore();
  // Subtitles are now rendered by mpv natively (ASS/SSA/SRT/PGS), not via
  // <track> in HTML. The store still owns the list for the menu UI.
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const { showPlaylist, togglePlaylist } = usePlaylistStore();
  const { showSettings, toggleSettings, loadSettings, theme } = useSettingsStore();
  const incognito = useIncognitoStore((s) => s.enabled);
  const toggleIncognito = useIncognitoStore((s) => s.toggle);
  const [isDragging, setIsDragging] = useState(false);
  // Right-click menu is rendered via Win32 TrackPopupMenu — no React state needed.
  const [controlsVisible, setControlsVisible] = useState(true);
  const [showClipDialog, setShowClipDialog] = useState(false);
  const [showUrlDialog, setShowUrlDialog] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateResult | null>(null);
  const [toast, setToast] = useState<{ id: number; kind: "success" | "error"; message: string } | null>(null);
  // Default-player prompt: show once after first launch unless dismissed.
  // Stored in localStorage so it survives reinstalls but not "clear data."
  // Default-player banner is Windows-only. The button invokes a
  // Windows-specific Tauri command (open_default_apps_settings) that
  // launches `ms-settings:defaultapps`, which doesn't exist on macOS or
  // Linux. On macOS the equivalent flow is Finder → Right-click → Open
  // With → unflick → Change All, which the user discovers themselves;
  // we don't try to surface a banner about it.
  const [showDefaultPrompt, setShowDefaultPrompt] = useState<boolean>(() => {
    if (typeof window === "undefined") return false;
    if (localStorage.getItem("default-prompt-dismissed")) return false;
    const ua = navigator.userAgent;
    return /Win(dows|32|64)/i.test(ua);
  });
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

  // Poll mpv for status — position / duration / state. 250 ms gives a smooth
  // playback bar without thrashing CPU. v0.8.x will replace this with mpv
  // observe_property events emitted from Rust, but polling is enough for the
  // initial cutover.
  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const s = await invoke<{
          state: "stopped" | "playing" | "paused";
          file: string | null;
          position: number;
          duration: number;
          volume: number;
          speed: number;
        }>("player_status");
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
      // Tauri's main HWND on Windows reports its rect in logical (CSS-
      // matching) pixels because Tao opts into PerMonitorV2 DPI awareness
      // but Tauri's window size config + GetWindowRect both stay in
      // logical units for our purposes. CSS getBoundingClientRect is in
      // logical px too, so they match 1:1 — no DPR scaling needed.
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
    // Hide the popup for full-screen modals + popovers + context-menu
    // states. Right-click menus now go through TrackPopupMenu (an
    // OS-level menu that floats above the popup automatically), so the
    // React contextMenu state path is dead code now — but we still hide
    // for popoverOpen because Subtitle/Audio/Filters menus anchor inside
    // the player bar and don't actually need the popup gone. Revisit if
    // the popoverOpen-hides behaviour annoys anyone.
    const blocking = showSettings || showClipDialog || showUrlDialog || popoverOpen;
    const visible = state !== "stopped" && !blocking;
    desiredVisibleRef.current = visible;
    if (lastVisibleRef.current === visible) return;
    lastVisibleRef.current = visible;
    invoke("video_surface_set_visible", { visible }).catch(() => {});
  }, [state, showSettings, showClipDialog, showUrlDialog, popoverOpen]);

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

    // Dialog mode: ask the user where to save, then mpv writes there.
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
    }, 3000);
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

  // Keyboard shortcuts
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Ignore if user is typing in an input
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement
      ) {
        return;
      }

      // Ctrl+O / Cmd+O to open file
      if ((e.ctrlKey || e.metaKey) && e.key === "o") {
        e.preventDefault();
        handleOpenFile();
        return;
      }

      // Ctrl+U / Cmd+U to open URL dialog
      if ((e.ctrlKey || e.metaKey) && e.key === "u") {
        e.preventDefault();
        setShowUrlDialog((v) => !v);
        return;
      }

      // Ctrl+, / Cmd+, to open settings
      if ((e.ctrlKey || e.metaKey) && e.key === ",") {
        e.preventDefault();
        toggleSettings();
        return;
      }

      // Ctrl+Shift+P / Cmd+Shift+P to toggle incognito mode
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === "P" || e.key === "p")) {
        e.preventDefault();
        useIncognitoStore.getState().toggle();
        return;
      }

      switch (e.key) {
        case " ":
          e.preventDefault();
          if (state === "playing") pause();
          else if (state === "paused") resume();
          break;
        case "ArrowLeft":
          e.preventDefault();
          seek(Math.max(0, position - 5));
          break;
        case "ArrowRight":
          e.preventDefault();
          seek(position + 5);
          break;
        case "ArrowUp":
          e.preventDefault();
          setVolume(Math.min(150, volume + 5));
          break;
        case "ArrowDown":
          e.preventDefault();
          setVolume(Math.max(0, volume - 5));
          break;
        case "l":
        case "L":
          e.preventDefault();
          toggleLibrary();
          break;
        case "n":
        case "N":
          e.preventDefault();
          togglePlaylist();
          break;
        case "s":
        case "S":
          e.preventDefault();
          if (state !== "stopped") captureScreenshot();
          break;
        case "c":
        case "C":
          e.preventDefault();
          if (state !== "stopped") setShowClipDialog((v) => !v);
          break;
        case "p":
        case "P":
          e.preventDefault();
          invoke("toggle_pip").catch(console.error);
          break;
        case "f":
        case "F":
          e.preventDefault();
          invoke("set_fullscreen").catch(console.error);
          break;
        case "Escape":
          if (showClipDialog) {
            setShowClipDialog(false);
          } else if (showUrlDialog) {
            setShowUrlDialog(false);
          } else {
            invoke("exit_fullscreen").catch(console.error);
          }
          break;
      }
    },
    [state, pause, resume, seek, position, volume, setVolume, toggleLibrary, togglePlaylist, handleOpenFile, showClipDialog, showUrlDialog, toggleSettings],
  );

  // Initialize mpv player on mount and load persisted settings
  useEffect(() => {
    invoke("player_init").catch(console.error);

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

    // Right-click goes through Win32 TrackPopupMenu instead of the
    // React ContextMenu component. The video popup is a top-level
    // window above the WebView, so a React menu would render *behind*
    // it; OS-level menus float above every app window automatically,
    // so the video keeps playing and the menu is always visible.
    // Native styling (Win11 default) is the cost.
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
  }, [contextMenuItems]);

  return (
    <div
      className={`flex h-full flex-col ${state === "playing" && !controlsVisible ? "cursor-none" : ""}`}
      style={{ backgroundColor: state === "stopped" ? "var(--bg-primary, #030712)" : "transparent" }}
      onMouseMove={handleMouseMove}
    >
      {/* Custom title bar.
          v0.8: always visible. Auto-hide is unsafe with transparent: true
          + an mpv child window beneath — opacity:0 leaves a transparent
          gap that lets the OS desktop bleed through. The proper "cinema
          mode" (fullscreen + child window expansion to occupy chrome
          space) lands in v0.8.x. */}
      <TitleBar />

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
                ? "bg-gradient-to-r from-violet-600/90 to-pink-600/90"
                : genStatus.kind === "success"
                ? "bg-emerald-600/90"
                : "bg-red-600/90"
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
        onContextMenu={handleContextMenu}
        onClick={(e) => {
          if (state === "stopped") return;
          const t = e.target as HTMLElement;
          if (t.closest("button, a, input, [data-no-toggle]")) return;
          // Defer the play/pause toggle: if a second click arrives within
          // ~250ms it's a double-click for fullscreen, and we cancel.
          if (clickTimer.current) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
          }
          clickTimer.current = setTimeout(() => {
            clickTimer.current = null;
            const cur = usePlayerStore.getState().state;
            if (cur === "playing") pause();
            else if (cur === "paused") resume();
          }, 240);
        }}
        onDoubleClick={(e) => {
          const t = e.target as HTMLElement;
          if (t.closest("button, a, input, [data-no-toggle]")) return;
          // Cancel any pending single-click → play/pause
          if (clickTimer.current) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
          }
          invoke("set_fullscreen").catch(console.error);
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

        {/* Idle state */}
        {state === "stopped" && (
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

      {/* Right-click menu is now rendered via Win32 TrackPopupMenu in
          handleContextMenu — see show_native_context_menu command. */}

      {/* Library panel — slides from left */}
      <AnimatePresence>
        {showLibrary && <LibraryPanel />}
      </AnimatePresence>

      {/* Playlist panel — slides from right */}
      <AnimatePresence>
        {showPlaylist && <PlaylistPanel />}
      </AnimatePresence>

      {/* Player bar — always visible in v0.8 (see TitleBar comment). */}
      <PlayerBar />
    </div>
  );
}

export default App;

import { useEffect, useCallback, useState, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import PlayerBar from "./components/Player/PlayerBar";
import TitleBar from "./components/TitleBar";
import LibraryPanel from "./components/Library/LibraryPanel";
import PlaylistPanel from "./components/Playlist/PlaylistPanel";
import ContextMenu, { type ContextMenuEntry } from "./components/ContextMenu";
import ClipDialog from "./components/ClipDialog";
import UrlDialog from "./components/UrlDialog";
import SettingsPanel from "./components/Settings/SettingsPanel";
import { usePlayerStore } from "./stores/playerStore";
import { useLibraryStore } from "./stores/libraryStore";
import { usePlaylistStore } from "./stores/playlistStore";
import { useSettingsStore } from "./stores/settingsStore";

async function openFileDialog() {
  const result = await invoke<{ path: string | null }>("open_file_dialog");
  return result.path;
}

function App() {
  const { state, play, pause, resume, seek, position, volume, setVolume } =
    usePlayerStore();
  const subtitles = usePlayerStore((s) => s.subtitles);
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const { showPlaylist, togglePlaylist } = usePlaylistStore();
  const { showSettings, toggleSettings, loadSettings, theme } = useSettingsStore();
  const [isDragging, setIsDragging] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [showClipDialog, setShowClipDialog] = useState(false);
  const [showUrlDialog, setShowUrlDialog] = useState(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Register the <video> element with the player store
  useEffect(() => {
    usePlayerStore.getState().setVideoElement(videoRef.current);
    return () => {
      usePlayerStore.getState().setVideoElement(null);
    };
  }, []);

  // Sync subtitle text track modes when subtitles array changes
  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;
    // Wait a tick for React to add/remove <track> elements
    const apply = () => {
      for (let i = 0; i < v.textTracks.length; i++) {
        const tt = v.textTracks[i];
        const target = subtitles[i];
        tt.mode = target?.active ? "showing" : "disabled";
      }
    };
    apply();
    // Some browsers need a small delay until <track> children are mounted
    const t = setTimeout(apply, 50);
    return () => clearTimeout(t);
  }, [subtitles]);

  // Wire video element events to the store
  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;

    const onTimeUpdate = () => usePlayerStore.setState({ position: v.currentTime });
    const onDurationChange = () => usePlayerStore.setState({ duration: isFinite(v.duration) ? v.duration : 0 });
    const onPlay = () => usePlayerStore.setState({ state: "playing" });
    const onPause = () => {
      // Only mark paused if we still have a file loaded
      if (usePlayerStore.getState().file) {
        usePlayerStore.setState({ state: "paused" });
      }
    };
    const onEnded = () => {
      const { file } = usePlayerStore.getState();
      if (file) invoke("clear_position", { path: file }).catch(() => {});
      usePlayerStore.setState({ state: "stopped", position: 0 });
    };
    const onError = () => console.error("video error:", v.error);

    v.addEventListener("timeupdate", onTimeUpdate);
    v.addEventListener("durationchange", onDurationChange);
    v.addEventListener("play", onPlay);
    v.addEventListener("pause", onPause);
    v.addEventListener("ended", onEnded);
    v.addEventListener("error", onError);
    return () => {
      v.removeEventListener("timeupdate", onTimeUpdate);
      v.removeEventListener("durationchange", onDurationChange);
      v.removeEventListener("play", onPlay);
      v.removeEventListener("pause", onPause);
      v.removeEventListener("ended", onEnded);
      v.removeEventListener("error", onError);
    };
  }, []);

  const handleOpenFile = useCallback(async () => {
    const path = await openFileDialog();
    if (path) play(path);
  }, [play]);

  const captureScreenshot = useCallback(async () => {
    const v = videoRef.current;
    if (!v || !v.videoWidth || !v.videoHeight) {
      console.warn("screenshot skipped: no video frame ready");
      return;
    }
    const canvas = document.createElement("canvas");
    canvas.width = v.videoWidth;
    canvas.height = v.videoHeight;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    try {
      ctx.drawImage(v, 0, 0, canvas.width, canvas.height);
    } catch (e) {
      console.error("drawImage failed:", e);
      return;
    }

    let blob: Blob | null = null;
    try {
      blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob((b) => resolve(b), "image/png"),
      );
    } catch (e) {
      console.error("toBlob failed (canvas tainted?):", e);
      return;
    }
    if (!blob) {
      console.error("toBlob returned null (canvas may be tainted by cross-origin video)");
      return;
    }

    const ts = Date.now();
    const result = await invoke<{ path: string | null }>("save_file_dialog", {
      defaultName: `unflick-screenshot-${ts}.png`,
    });
    if (!result.path) return;

    const buf = await blob.arrayBuffer();
    try {
      await invoke("write_file_bytes", {
        path: result.path,
        bytes: Array.from(new Uint8Array(buf)),
      });
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
          setVolume(Math.min(100, volume + 5));
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

    loadSettings().then(async () => {
      // Apply saved volume
      const savedVolume = useSettingsStore.getState().volume;
      if (savedVolume !== undefined && savedVolume !== 100) {
        setVolume(savedVolume);
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
            Math.min(100, usePlayerStore.getState().volume + 5)
          );
          break;
        case "volume_down":
          usePlayerStore.getState().setVolume(
            Math.max(0, usePlayerStore.getState().volume - 5)
          );
          break;
        case "check_updates":
          invoke<{ message: string }>("check_for_updates")
            .then((result) => {
              alert(result.message);
            })
            .catch((err) => {
              alert(`Update check failed: ${err}`);
            });
          break;
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
      label: "Open File...",
      shortcut: "Ctrl+O",
      onClick: handleOpenFile,
    },
    {
      label: "Open URL...",
      shortcut: "Ctrl+U",
      onClick: () => setShowUrlDialog(true),
    },
    { separator: true },
    {
      label: state === "playing" ? "Pause" : "Play",
      shortcut: "Space",
      onClick: () => {
        if (state === "playing") pause();
        else if (state === "paused") resume();
      },
      disabled: state === "stopped",
    },
    {
      label: "Stop",
      onClick: () => usePlayerStore.getState().stop(),
      disabled: state === "stopped",
    },
    { separator: true },
    {
      label: "Screenshot",
      shortcut: "S",
      onClick: () => captureScreenshot(),
      disabled: state === "stopped",
    },
    {
      label: "Extract Clip...",
      shortcut: "C",
      onClick: () => setShowClipDialog(true),
      disabled: state === "stopped",
    },
    { separator: true },
    {
      label: "Toggle Fullscreen",
      shortcut: "F",
      onClick: () => invoke("set_fullscreen").catch(console.error),
    },
    {
      label: "Picture in Picture",
      shortcut: "P",
      onClick: () => invoke("toggle_pip").catch(console.error),
    },
    {
      label: "Toggle Library",
      shortcut: "L",
      onClick: toggleLibrary,
    },
    {
      label: "Toggle Playlist",
      shortcut: "N",
      onClick: togglePlaylist,
    },
    { separator: true },
    {
      label: "Settings",
      shortcut: "Ctrl+,",
      onClick: toggleSettings,
    },
  ];

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
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
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  return (
    <div
      className={`flex h-full flex-col ${state === "playing" && !controlsVisible ? "cursor-none" : ""}`}
      style={{ backgroundColor: state === "stopped" ? "var(--bg-primary, #030712)" : "transparent" }}
      onMouseMove={handleMouseMove}
    >
      {/* Custom title bar */}
      <motion.div
        animate={{ opacity: controlsVisible ? 1 : 0, y: controlsVisible ? 0 : -8 }}
        transition={{ duration: 0.3, ease: "easeInOut" }}
        className={!controlsVisible ? "pointer-events-none" : ""}
      >
        <TitleBar />
      </motion.div>

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
                Drop a video file or click to open
              </p>
              <button
                onClick={handleOpenFile}
                className="idle-open-btn idle-fade-in-delay-2 mt-1 rounded-xl px-8 py-2.5 text-[13px] font-semibold text-white transition-all duration-200 active:scale-95"
                style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
              >
                Open File
              </button>
            </div>
          </div>
        )}

        {/* Video element — fills the area, sits behind UI controls.
            crossOrigin is set dynamically by playerStore based on source. */}
        <video
          ref={videoRef}
          className="absolute inset-0 h-full w-full bg-black"
          style={{
            display: state === "stopped" ? "none" : "block",
            objectFit: "contain",
          }}
          playsInline
        >
          {subtitles.map((track) => (
            <track
              key={track.id}
              kind="subtitles"
              src={track.src}
              label={track.label}
              default={track.active}
            />
          ))}
        </video>


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

      {/* Context menu */}
      {contextMenu && (
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

      {/* Player bar at bottom — auto-hides during playback */}
      <motion.div
        animate={{ opacity: controlsVisible ? 1 : 0, y: controlsVisible ? 0 : 8 }}
        transition={{ duration: 0.3, ease: "easeInOut" }}
        className={!controlsVisible ? "pointer-events-none" : ""}
      >
        <PlayerBar />
      </motion.div>
    </div>
  );
}

export default App;

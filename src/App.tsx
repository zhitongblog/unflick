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
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const { showPlaylist, togglePlaylist } = usePlaylistStore();
  const { showSettings, toggleSettings, loadSettings } = useSettingsStore();
  const [isDragging, setIsDragging] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [showClipDialog, setShowClipDialog] = useState(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleOpenFile = useCallback(async () => {
    const path = await openFileDialog();
    if (path) play(path);
  }, [play]);

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
          if (state !== "stopped") {
            invoke("player_screenshot")
              .then((result: unknown) => {
                console.log("Screenshot saved:", result);
              })
              .catch(console.error);
          }
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
          } else {
            invoke("exit_fullscreen").catch(console.error);
          }
          break;
      }
    },
    [state, pause, resume, seek, position, volume, setVolume, toggleLibrary, togglePlaylist, handleOpenFile, showClipDialog, toggleSettings],
  );

  // Initialize mpv player on mount and load persisted settings
  useEffect(() => {
    invoke("player_init").catch(console.error);
    loadSettings();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Toggle background transparency when video is playing so mpv shows through
  useEffect(() => {
    const els = [document.documentElement, document.body, document.getElementById("root")];
    if (state !== "stopped") {
      els.forEach((el) => el?.style.setProperty("background-color", "transparent", "important"));
    } else {
      els.forEach((el) => el?.style.setProperty("background-color", "#030712", "important"));
    }
  }, [state]);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Listen for native menu events
  useEffect(() => {
    const unlisten = listen<string>("menu-event", async (event) => {
      const { state: currentState } = usePlayerStore.getState();
      switch (event.payload) {
        case "open":
          handleOpenFile();
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
      onClick: () => invoke("player_screenshot").catch(console.error),
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
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  return (
    <div
      className={`flex h-full flex-col ${state === "stopped" ? "bg-gray-950" : ""} ${state === "playing" && !controlsVisible ? "cursor-none" : ""}`}
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
        onDoubleClick={() => {
          if (state === "playing") pause();
          else if (state === "paused") resume();
        }}
      >
        {/* Drop overlay */}
        <AnimatePresence>
          {isDragging && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="absolute inset-0 z-10 flex items-center justify-center bg-gray-950/80 backdrop-blur-sm"
            >
              <div className="rounded-2xl border-2 border-dashed border-brand-purple/60 px-12 py-8">
                <p className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-xl font-medium text-transparent">
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
                width: "520px",
                height: "520px",
                left: "calc(50% - 340px)",
                top: "calc(50% - 320px)",
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(124,58,237,0.09) 0%, transparent 70%)",
                animation: "orb-drift-a 18s ease-in-out infinite",
              }}
            />
            <div
              className="pointer-events-none absolute"
              style={{
                width: "480px",
                height: "480px",
                left: "calc(50% + 60px)",
                top: "calc(50% - 180px)",
                borderRadius: "50%",
                background: "radial-gradient(circle, rgba(219,39,119,0.07) 0%, transparent 70%)",
                animation: "orb-drift-b 22s ease-in-out infinite",
              }}
            />

            {/* Center content */}
            <div className="relative z-10 flex flex-col items-center gap-5">
              <h1 className="idle-title text-5xl font-bold tracking-tight">
                unflick
              </h1>
              <p className="text-sm font-light tracking-wide text-white/35">
                Drop a video file or click to open
              </p>
              <button
                onClick={handleOpenFile}
                className="idle-open-btn mt-1 rounded-xl bg-gradient-to-r from-brand-purple to-brand-pink px-8 py-2.5 text-sm font-semibold text-white transition-all duration-200 hover:opacity-90 active:scale-95"
              >
                Open File
              </button>
            </div>
          </div>
        )}

        {/* mpv renders behind the webview via --wid */}

        {/* Clip dialog */}
        {showClipDialog && (
          <ClipDialog onClose={() => setShowClipDialog(false)} />
        )}

        {/* Settings panel */}
        {showSettings && (
          <SettingsPanel onClose={toggleSettings} />
        )}
      </div>

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

import { useEffect, useCallback, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import PlayerBar from "./components/Player/PlayerBar";
import LibraryPanel from "./components/Library/LibraryPanel";
import ContextMenu, { type ContextMenuEntry } from "./components/ContextMenu";
import { usePlayerStore } from "./stores/playerStore";
import { useLibraryStore } from "./stores/libraryStore";

async function openFileDialog() {
  const result = await invoke<{ path: string | null }>("open_file_dialog");
  return result.path;
}

function App() {
  const { state, play, pause, resume, seek, position, volume, setVolume } =
    usePlayerStore();
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const [isDragging, setIsDragging] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const handleOpenFile = useCallback(async () => {
    const path = await openFileDialog();
    if (path) play(path);
  }, [play]);

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
          invoke("exit_fullscreen").catch(console.error);
          break;
      }
    },
    [state, pause, resume, seek, position, volume, setVolume, toggleLibrary, handleOpenFile],
  );

  // Initialize mpv player with the window handle on mount
  useEffect(() => {
    invoke("player_init").catch(console.error);
  }, []);

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
  ];

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  return (
    <div className={`flex h-full flex-col ${state === "stopped" ? "bg-gray-950" : "bg-transparent"}`}>
      {/* Video area / drop zone */}
      <div
        className="relative flex flex-1 items-center justify-center overflow-hidden"
        onContextMenu={handleContextMenu}
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
          <div className="flex flex-col items-center gap-4 select-none">
            <h1 className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-4xl font-bold text-transparent">
              unflick
            </h1>
            <p className="text-sm text-gray-500">
              Drop a video file to play
            </p>
            <button
              onClick={handleOpenFile}
              className="mt-2 rounded-lg bg-gradient-to-r from-brand-purple to-brand-pink px-6 py-2 text-sm font-medium text-white transition-opacity hover:opacity-80"
            >
              Open File
            </button>
          </div>
        )}

        {/* Playing/paused state - mpv renders video directly via --wid into the native window */}
        {state !== "stopped" && (
          <div className="absolute inset-0 bg-transparent pointer-events-none">
            {/* mpv video surface renders behind the webview via native window embedding */}
          </div>
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

      {/* Library panel */}
      <AnimatePresence>
        {showLibrary && <LibraryPanel />}
      </AnimatePresence>

      {/* Player bar at bottom */}
      <PlayerBar />
    </div>
  );
}

export default App;

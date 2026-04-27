import { useEffect, useCallback, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import PlayerBar from "./components/Player/PlayerBar";
import LibraryPanel from "./components/Library/LibraryPanel";
import { usePlayerStore } from "./stores/playerStore";
import { useLibraryStore } from "./stores/libraryStore";

function App() {
  const { state, play, pause, resume, seek, position, volume, setVolume } =
    usePlayerStore();
  const { showLibrary, toggleLibrary } = useLibraryStore();
  const [isDragging, setIsDragging] = useState(false);

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
    [state, pause, resume, seek, position, volume, setVolume, toggleLibrary],
  );

  // Initialize mpv player with the window handle on mount
  useEffect(() => {
    invoke("player_init").catch(console.error);
  }, []);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

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

  return (
    <div className={`flex h-full flex-col ${state === "stopped" ? "bg-gray-950" : "bg-transparent"}`}>
      {/* Video area / drop zone */}
      <div
        className="relative flex flex-1 items-center justify-center overflow-hidden"
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
          </div>
        )}

        {/* Playing/paused state - mpv renders video directly via --wid into the native window */}
        {state !== "stopped" && (
          <div className="absolute inset-0 bg-transparent pointer-events-none">
            {/* mpv video surface renders behind the webview via native window embedding */}
          </div>
        )}
      </div>

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

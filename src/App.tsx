import { useEffect, useCallback, useState, DragEvent } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
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
      }
    },
    [state, pause, resume, seek, position, volume, setVolume, toggleLibrary],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Drag and drop
  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const file = files[0];
      // Use the file path if available (Tauri provides it), otherwise use name
      const path = (file as any).path || file.name;
      play(path);
    }
  };

  return (
    <div className="flex h-full flex-col bg-gray-950">
      {/* Video area / drop zone */}
      <div
        className="relative flex flex-1 items-center justify-center overflow-hidden"
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
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

        {/* Playing/paused state placeholder - video will render here via mpv --wid */}
        {state !== "stopped" && (
          <div className="flex items-center justify-center text-gray-600">
            {/* mpv video surface will be embedded here */}
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

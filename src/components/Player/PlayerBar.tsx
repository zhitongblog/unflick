import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";
import { useLibraryStore } from "../../stores/libraryStore";
import ProgressBar from "./ProgressBar";
import PlaybackControls from "./PlaybackControls";
import VolumeControl from "./VolumeControl";

function LibraryIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="7" height="7" />
      <rect x="14" y="3" width="7" height="7" />
      <rect x="3" y="14" width="7" height="7" />
      <rect x="14" y="14" width="7" height="7" />
    </svg>
  );
}

function PipIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <rect x="12" y="9" width="8" height="6" rx="1" />
    </svg>
  );
}

function FullscreenIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="15 3 21 3 21 9" />
      <polyline points="9 21 3 21 3 15" />
      <line x1="21" y1="3" x2="14" y2="10" />
      <line x1="3" y1="21" x2="10" y2="14" />
    </svg>
  );
}

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

export default function PlayerBar() {
  const { file, state } = usePlayerStore();
  const toggleLibrary = useLibraryStore((s) => s.toggleLibrary);

  return (
    <motion.div
      className="flex flex-col border-t border-gray-800 bg-gray-900/95 backdrop-blur-sm"
      initial={{ y: 20, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      transition={{ duration: 0.3 }}
    >
      {/* Progress bar */}
      <ProgressBar />

      {/* Controls row */}
      <div className="flex items-center justify-between px-4 pb-3 pt-1">
        {/* Left: library button + file name */}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <motion.button
            className="flex-shrink-0 rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={toggleLibrary}
            whileTap={{ scale: 0.9 }}
            title="Media Library (L)"
          >
            <LibraryIcon />
          </motion.button>
          {file && state !== "stopped" ? (
            <p className="truncate text-sm text-gray-300" title={file}>
              {extractFileName(file)}
            </p>
          ) : (
            <p className="text-sm text-gray-600">No file loaded</p>
          )}
        </div>

        {/* Center: playback controls */}
        <div className="flex-shrink-0 px-4">
          <PlaybackControls />
        </div>

        {/* Right: volume + window controls */}
        <div className="flex flex-1 items-center justify-end gap-1">
          <VolumeControl />
          <motion.button
            className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={() => invoke("toggle_pip").catch(console.error)}
            whileTap={{ scale: 0.9 }}
            title="Picture-in-Picture (P)"
          >
            <PipIcon />
          </motion.button>
          <motion.button
            className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={() => invoke("set_fullscreen").catch(console.error)}
            whileTap={{ scale: 0.9 }}
            title="Fullscreen (F)"
          >
            <FullscreenIcon />
          </motion.button>
        </div>
      </div>
    </motion.div>
  );
}

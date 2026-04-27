import { motion } from "framer-motion";
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

        {/* Right: volume */}
        <div className="flex flex-1 items-center justify-end">
          <VolumeControl />
        </div>
      </div>
    </motion.div>
  );
}

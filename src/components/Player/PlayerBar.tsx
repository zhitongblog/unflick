import { motion } from "framer-motion";
import { usePlayerStore } from "../../stores/playerStore";
import ProgressBar from "./ProgressBar";
import PlaybackControls from "./PlaybackControls";
import VolumeControl from "./VolumeControl";

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

export default function PlayerBar() {
  const { file, state } = usePlayerStore();

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
        {/* Left: file name */}
        <div className="min-w-0 flex-1">
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

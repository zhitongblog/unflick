import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";

const SPEED_OPTIONS = [0.5, 0.75, 1, 1.25, 1.5, 2];

function PrevIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
    </svg>
  );
}

function NextIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M16 6h2v12h-2zm-10 6l8.5 6V6z" transform="scale(-1,1) translate(-24,0)" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg width="28" height="28" viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg width="28" height="28" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 4h4v16H6zM14 4h4v16h-4z" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="6" width="12" height="12" rx="1" />
    </svg>
  );
}

function ScreenshotIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M23 19a2 2 0 01-2 2H3a2 2 0 01-2-2V8a2 2 0 012-2h4l2-3h6l2 3h4a2 2 0 012 2z" />
      <circle cx="12" cy="13" r="4" />
    </svg>
  );
}

export default function PlaybackControls() {
  const { state, speed, pause, resume, stop, play, file, setSpeed } = usePlayerStore();
  const [showSpeedMenu, setShowSpeedMenu] = useState(false);

  const handlePlayPause = () => {
    if (state === "playing") {
      pause();
    } else if (state === "paused") {
      resume();
    } else if (file) {
      play(file);
    }
  };

  const handleSpeedSelect = (rate: number) => {
    setSpeed(rate);
    setShowSpeedMenu(false);
  };

  return (
    <div className="flex items-center gap-1">
      {/* Previous track */}
      <motion.button
        className="rounded-full p-2 text-gray-500 cursor-not-allowed"
        disabled
        whileTap={{ scale: 0.9 }}
        title="Previous (playlist not available)"
      >
        <PrevIcon />
      </motion.button>

      {/* Stop */}
      <motion.button
        className="rounded-full p-2 text-gray-300 transition-colors hover:text-white disabled:text-gray-600 disabled:cursor-not-allowed"
        onClick={stop}
        disabled={state === "stopped"}
        whileTap={{ scale: 0.9 }}
        title="Stop"
      >
        <StopIcon />
      </motion.button>

      {/* Play/Pause - main button */}
      <motion.button
        className="mx-1 flex h-10 w-10 items-center justify-center rounded-full bg-gradient-to-r from-brand-purple to-brand-pink text-white shadow-lg transition-shadow hover:shadow-brand-purple/30"
        onClick={handlePlayPause}
        whileTap={{ scale: 0.9 }}
        whileHover={{ scale: 1.05 }}
        title={state === "playing" ? "Pause" : "Play"}
      >
        {state === "playing" ? <PauseIcon /> : <PlayIcon />}
      </motion.button>

      {/* Next track */}
      <motion.button
        className="rounded-full p-2 text-gray-500 cursor-not-allowed"
        disabled
        whileTap={{ scale: 0.9 }}
        title="Next (playlist not available)"
      >
        <NextIcon />
      </motion.button>

      {/* Speed selector */}
      <div className="relative ml-2">
        <motion.button
          className="rounded px-2 py-1 text-xs tabular-nums text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
          onClick={() => setShowSpeedMenu(!showSpeedMenu)}
          whileTap={{ scale: 0.95 }}
          title="Playback speed"
        >
          {speed}x
        </motion.button>

        <AnimatePresence>
          {showSpeedMenu && (
            <motion.div
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 8 }}
              transition={{ duration: 0.12 }}
              className="absolute bottom-full left-1/2 mb-2 -translate-x-1/2 rounded-lg border border-gray-700 bg-gray-900 py-1 shadow-xl"
            >
              {SPEED_OPTIONS.map((rate) => (
                <button
                  key={rate}
                  className={`block w-full whitespace-nowrap px-4 py-1.5 text-left text-xs transition-colors hover:bg-gray-800 ${
                    rate === speed ? "text-brand-purple font-medium" : "text-gray-300"
                  }`}
                  onClick={() => handleSpeedSelect(rate)}
                >
                  {rate}x
                </button>
              ))}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Screenshot */}
      <motion.button
        className="ml-1 rounded-lg p-2 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200 disabled:cursor-not-allowed disabled:text-gray-600"
        onClick={() => {
          invoke("player_screenshot")
            .then((result: unknown) => {
              console.log("Screenshot saved:", result);
            })
            .catch(console.error);
        }}
        disabled={state === "stopped"}
        whileTap={{ scale: 0.9 }}
        title="Screenshot (S)"
      >
        <ScreenshotIcon />
      </motion.button>
    </div>
  );
}

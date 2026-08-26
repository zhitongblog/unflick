import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { usePlayerStore } from "../../stores/playerStore";
import { usePlaylistStore } from "../../stores/playlistStore";
import { formatSpeed } from "../../lib/format";

const SPEED_OPTIONS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];

/**
 * Fine-tune bounds and step. The presets cover the rates people ask for by
 * name; this covers the ones they arrive at by ear — 1.35× for a slow
 * lecturer. Narrower than mpv's own 0.01–100 on purpose: the popover is not
 * where anyone sets 40× playback, and a slider spanning four orders of
 * magnitude is useless for the adjustment this control exists to make.
 */
const SPEED_MIN = 0.25;
const SPEED_MAX = 4;
const SPEED_STEP = 0.05;

function PrevIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
    </svg>
  );
}

function NextIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
      <path d="M16 6h2v12h-2zm-10 6l8.5 6V6z" transform="scale(-1,1) translate(-24,0)" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 4h4v16H6zM14 4h4v16h-4z" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );
}

function ScreenshotIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M23 19a2 2 0 01-2 2H3a2 2 0 01-2-2V8a2 2 0 012-2h4l2-3h6l2 3h4a2 2 0 012 2z" />
      <circle cx="12" cy="13" r="4" />
    </svg>
  );
}

export default function PlaybackControls() {
  const { state, speed, pause, resume, stop, play, file, setSpeed } = usePlayerStore();
  const { items: playlistItems, next: playlistNext, prev: playlistPrev } = usePlaylistStore();
  const [showSpeedMenu, setShowSpeedMenu] = useState(false);

  // Pull the mpv overlay out of the way when the speed picker is open
  // so it isn't hidden behind the video region.
  useEffect(() => {
    if (!showSpeedMenu) return;
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, [showSpeedMenu]);

  const hasPlaylist = playlistItems.length > 1;

  const handlePlayPause = () => {
    if (state === "playing") pause();
    else if (state === "paused") resume();
    else if (file) play(file);
  };

  /**
   * Nudge by whole steps off a rounded base, so arriving here from a preset
   * like 1.25 keeps the readout on the same grid instead of drifting to
   * 1.2999999.
   */
  const nudgeSpeed = (direction: number) => {
    const base = Math.round(speed / SPEED_STEP) * SPEED_STEP;
    const next = Math.min(SPEED_MAX, Math.max(SPEED_MIN, base + direction * SPEED_STEP));
    setSpeed(parseFloat(next.toFixed(2)));
  };

  const nudgeBtnClass = (enabled: boolean) =>
    `flex h-6 w-6 items-center justify-center rounded-md text-[13px] leading-none transition-colors ${
      enabled
        ? "text-white/60 hover:bg-white/10 hover:text-white"
        : "cursor-not-allowed text-white/20"
    }`;

  const iconBtnClass = (enabled: boolean) =>
    `rounded-full p-2 transition-all duration-150 ${
      enabled
        ? "text-white/60 hover:text-white hover:bg-white/8 active:scale-90"
        : "text-white/15 cursor-not-allowed"
    }`;

  return (
    <div className="flex items-center gap-0.5">
      {/* Stop */}
      <button
        className={iconBtnClass(state !== "stopped")}
        onClick={stop}
        disabled={state === "stopped"}
        title="Stop"
      >
        <StopIcon />
      </button>

      {/* Previous */}
      <button
        className={iconBtnClass(hasPlaylist)}
        disabled={!hasPlaylist}
        onClick={() => hasPlaylist && playlistPrev()}
        title={hasPlaylist ? "Previous" : "No playlist"}
      >
        <PrevIcon />
      </button>

      {/* Play/Pause — hero button */}
      <motion.button
        className="mx-1.5 flex h-11 w-11 items-center justify-center rounded-full text-white shadow-lg"
        style={{
          background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)",
        }}
        onClick={handlePlayPause}
        whileTap={{ scale: 0.88 }}
        whileHover={{ scale: 1.06 }}
        title={state === "playing" ? "Pause" : "Play"}
      >
        {state === "playing" ? <PauseIcon /> : <PlayIcon />}
      </motion.button>

      {/* Next */}
      <button
        className={iconBtnClass(hasPlaylist)}
        disabled={!hasPlaylist}
        onClick={() => hasPlaylist && playlistNext()}
        title={hasPlaylist ? "Next" : "No playlist"}
      >
        <NextIcon />
      </button>

      {/* Speed */}
      <div className="relative ml-1">
        <button
          className={`rounded-md px-1.5 py-1 text-[11px] tabular-nums font-medium transition-all duration-150 ${
            speed !== 1
              ? "text-brand-purple bg-brand-purple/10"
              : "text-white/35 hover:text-white/60 hover:bg-white/5"
          }`}
          onClick={() => setShowSpeedMenu(!showSpeedMenu)}
          title="Playback speed"
        >
          {formatSpeed(speed)}
        </button>

        <AnimatePresence>
          {showSpeedMenu && (
            <motion.div
              initial={{ opacity: 0, y: 8, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 8, scale: 0.95 }}
              transition={{ duration: 0.12 }}
              className="glass-elevated absolute bottom-full left-1/2 mb-2 -translate-x-1/2 rounded-xl py-1 shadow-2xl"
            >
              {SPEED_OPTIONS.map((rate) => (
                <button
                  key={rate}
                  className={`block w-full whitespace-nowrap px-5 py-1.5 text-left text-[11px] font-medium transition-colors hover:bg-white/8 ${
                    rate === speed ? "text-brand-purple" : "text-white/60"
                  }`}
                  onClick={() => { setSpeed(rate); setShowSpeedMenu(false); }}
                >
                  {formatSpeed(rate)}
                </button>
              ))}

              {/* Fine tune. Stays open while you nudge — the whole point is
                  landing on a rate the presets don't have. */}
              <div className="mt-1 border-t border-white/10 px-3 pb-1 pt-2">
                <div className="flex items-center gap-2">
                  <button
                    className={nudgeBtnClass(speed > SPEED_MIN)}
                    disabled={speed <= SPEED_MIN}
                    onClick={() => nudgeSpeed(-1)}
                    title={`-${SPEED_STEP}`}
                  >
                    &#8722;
                  </button>
                  <span className="min-w-[3.25rem] text-center text-[11px] font-medium tabular-nums text-white/80">
                    {formatSpeed(speed)}
                  </span>
                  <button
                    className={nudgeBtnClass(speed < SPEED_MAX)}
                    disabled={speed >= SPEED_MAX}
                    onClick={() => nudgeSpeed(1)}
                    title={`+${SPEED_STEP}`}
                  >
                    +
                  </button>
                </div>
                <input
                  type="range"
                  min={SPEED_MIN}
                  max={SPEED_MAX}
                  step={SPEED_STEP}
                  value={Math.min(SPEED_MAX, Math.max(SPEED_MIN, speed))}
                  onChange={(e) => setSpeed(Number(e.target.value))}
                  className="mt-2 w-full accent-brand-purple"
                />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Screenshot */}
      <button
        className={`ml-0.5 ${iconBtnClass(state !== "stopped")}`}
        onClick={() => window.dispatchEvent(new CustomEvent("unflick:screenshot"))}
        disabled={state === "stopped"}
        title="Screenshot (S)"
      >
        <ScreenshotIcon />
      </button>
    </div>
  );
}

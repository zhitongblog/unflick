import { useState, useRef, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { usePlayerStore } from "../../stores/playerStore";

function VolumeIcon({ level }: { level: number }) {
  if (level === 0) {
    return (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 5L6 9H2v6h4l5 4V5z" />
        <line x1="23" y1="9" x2="17" y2="15" />
        <line x1="17" y1="9" x2="23" y2="15" />
      </svg>
    );
  }
  if (level < 50) {
    return (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 5L6 9H2v6h4l5 4V5z" />
        <path d="M15.54 8.46a5 5 0 010 7.07" />
      </svg>
    );
  }
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5L6 9H2v6h4l5 4V5z" />
      <path d="M15.54 8.46a5 5 0 010 7.07" />
      <path d="M19.07 4.93a10 10 0 010 14.14" />
    </svg>
  );
}

export default function VolumeControl() {
  const { volume, setVolume } = usePlayerStore();
  const [showSlider, setShowSlider] = useState(false);
  const [prevVolume, setPrevVolume] = useState(100);
  const sliderRef = useRef<HTMLDivElement>(null);

  const toggleMute = () => {
    if (volume > 0) {
      setPrevVolume(volume);
      setVolume(0);
    } else {
      setVolume(prevVolume || 100);
    }
  };

  const handleSliderClick = useCallback(
    (e: React.MouseEvent) => {
      if (!sliderRef.current) return;
      const rect = sliderRef.current.getBoundingClientRect();
      setVolume(Math.round(Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)) * 100));
    },
    [setVolume],
  );

  const handleSliderDrag = useCallback(
    (e: React.MouseEvent) => {
      if (e.buttons !== 1 || !sliderRef.current) return;
      const rect = sliderRef.current.getBoundingClientRect();
      setVolume(Math.round(Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)) * 100));
    },
    [setVolume],
  );

  return (
    <div
      className="relative flex items-center gap-1.5"
      onMouseEnter={() => setShowSlider(true)}
      onMouseLeave={() => setShowSlider(false)}
    >
      <button
        className="rounded-full p-1.5 text-white/50 transition-all duration-150 hover:text-white/80 hover:bg-white/8 active:scale-90"
        onClick={toggleMute}
        title={volume === 0 ? "Unmute" : "Mute"}
      >
        <VolumeIcon level={volume} />
      </button>

      <AnimatePresence>
        {showSlider && (
          <motion.div
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 80, opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ duration: 0.15, ease: "easeOut" }}
            className="overflow-hidden"
          >
            <div
              ref={sliderRef}
              className="group flex h-6 w-20 cursor-pointer items-center"
              onClick={handleSliderClick}
              onMouseMove={handleSliderDrag}
            >
              <div className="relative h-[3px] w-full rounded-full bg-white/10 transition-all group-hover:h-1">
                <div
                  className="absolute inset-y-0 left-0 rounded-full"
                  style={{
                    width: `${volume}%`,
                    background: "linear-gradient(90deg, #7C3AED, #DB2777)",
                  }}
                />
                {/* Volume knob */}
                <div
                  className="absolute top-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 transition-opacity group-hover:opacity-100"
                  style={{
                    left: `${volume}%`,
                    transform: `translate(-50%, -50%)`,
                    width: "10px",
                    height: "10px",
                    boxShadow: "0 0 4px rgba(124,58,237,0.4)",
                  }}
                />
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <span className="w-7 text-[11px] tabular-nums text-white/25 font-medium">
        {volume}
      </span>
    </div>
  );
}

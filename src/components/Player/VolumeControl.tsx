import { useState, useRef, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { usePlayerStore } from "../../stores/playerStore";

function VolumeIcon({ level }: { level: number }) {
  if (level === 0) {
    return (
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 5L6 9H2v6h4l5 4V5z" />
        <line x1="23" y1="9" x2="17" y2="15" />
        <line x1="17" y1="9" x2="23" y2="15" />
      </svg>
    );
  }
  if (level < 50) {
    return (
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 5L6 9H2v6h4l5 4V5z" />
        <path d="M15.54 8.46a5 5 0 010 7.07" />
      </svg>
    );
  }
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
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
      const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      setVolume(Math.round(ratio * 100));
    },
    [setVolume],
  );

  const handleSliderDrag = useCallback(
    (e: React.MouseEvent) => {
      if (e.buttons !== 1 || !sliderRef.current) return;
      const rect = sliderRef.current.getBoundingClientRect();
      const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      setVolume(Math.round(ratio * 100));
    },
    [setVolume],
  );

  return (
    <div
      className="relative flex items-center gap-2"
      onMouseEnter={() => setShowSlider(true)}
      onMouseLeave={() => setShowSlider(false)}
    >
      <motion.button
        className="text-gray-300 transition-colors hover:text-white"
        onClick={toggleMute}
        whileTap={{ scale: 0.9 }}
        title={volume === 0 ? "Unmute" : "Mute"}
      >
        <VolumeIcon level={volume} />
      </motion.button>

      <AnimatePresence>
        {showSlider && (
          <motion.div
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 80, opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden"
          >
            <div
              ref={sliderRef}
              className="group flex h-6 w-20 cursor-pointer items-center"
              onClick={handleSliderClick}
              onMouseMove={handleSliderDrag}
            >
              <div className="h-1 w-full rounded-full bg-gray-700 transition-all group-hover:h-1.5">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-brand-purple to-brand-pink transition-all"
                  style={{ width: `${volume}%` }}
                />
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <span className="w-8 text-xs tabular-nums text-gray-500">
        {volume}
      </span>
    </div>
  );
}

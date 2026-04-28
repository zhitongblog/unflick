import { useRef, useState, useCallback } from "react";
import { motion } from "framer-motion";
import { usePlayerStore } from "../../stores/playerStore";

function formatTime(seconds: number): string {
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  if (h > 0) {
    return `${h}:${pad(m)}:${pad(sec)}`;
  }
  return `${pad(m)}:${pad(sec)}`;
}

export default function ProgressBar() {
  const { position, duration, seek, state } = usePlayerStore();
  const barRef = useRef<HTMLDivElement>(null);
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [hoverTime, setHoverTime] = useState<number>(0);
  const [isHovering, setIsHovering] = useState(false);

  const progress = duration > 0 ? (position / duration) * 100 : 0;

  const getTimeFromX = useCallback(
    (clientX: number) => {
      if (!barRef.current || duration <= 0) return 0;
      const rect = barRef.current.getBoundingClientRect();
      const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
      return ratio * duration;
    },
    [duration],
  );

  const handleClick = (e: React.MouseEvent) => {
    if (state === "stopped") return;
    const time = getTimeFromX(e.clientX);
    seek(time);
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!barRef.current) return;
    const rect = barRef.current.getBoundingClientRect();
    setHoverX(e.clientX - rect.left);
    setHoverTime(getTimeFromX(e.clientX));
  };

  const handleMouseEnter = () => setIsHovering(true);

  const handleMouseLeave = () => {
    setHoverX(null);
    setIsHovering(false);
  };

  return (
    <div className="flex items-center gap-3 px-4">
      <span className="w-14 text-right text-xs tabular-nums text-white/40">
        {formatTime(position)}
      </span>

      <div
        ref={barRef}
        className="group relative flex h-6 flex-1 cursor-pointer items-center"
        onClick={handleClick}
        onMouseMove={handleMouseMove}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={handleMouseLeave}
      >
        {/* Track background */}
        <div
          className="w-full rounded-full transition-all duration-150"
          style={{ height: isHovering ? "6px" : "4px", background: "rgba(255,255,255,0.12)" }}
        >
          {/* Progress fill — brand gradient */}
          <motion.div
            className="h-full rounded-full bg-gradient-to-r from-brand-purple to-brand-pink"
            style={{ width: `${progress}%` }}
            transition={{ duration: 0.1 }}
          />
        </div>

        {/* Hover time tooltip — pill with arrow */}
        {hoverX !== null && duration > 0 && (
          <div
            className="pointer-events-none absolute z-10 flex flex-col items-center"
            style={{ left: hoverX, bottom: "calc(100% + 4px)", transform: "translateX(-50%)" }}
          >
            <div className="rounded-full bg-black/80 px-2 py-1 text-xs font-medium text-white backdrop-blur-sm whitespace-nowrap shadow-lg">
              {formatTime(hoverTime)}
            </div>
            {/* Arrow triangle */}
            <div
              style={{
                width: 0,
                height: 0,
                borderLeft: "4px solid transparent",
                borderRight: "4px solid transparent",
                borderTop: "4px solid rgba(0,0,0,0.8)",
              }}
            />
          </div>
        )}

        {/* Playhead dot — 12px brand-purple circle with glow, appears on hover */}
        {state !== "stopped" && (
          <div
            className="pointer-events-none absolute top-1/2 -translate-y-1/2 -translate-x-1/2 rounded-full transition-all duration-150"
            style={{
              left: `${progress}%`,
              width: isHovering ? "12px" : "0px",
              height: isHovering ? "12px" : "0px",
              opacity: isHovering ? 1 : 0,
              background: "#7C3AED",
              boxShadow: isHovering ? "0 0 8px 2px rgba(124,58,237,0.55), 0 0 16px 4px rgba(124,58,237,0.25)" : "none",
            }}
          />
        )}
      </div>

      <span className="w-14 text-xs tabular-nums text-white/40">
        {formatTime(duration)}
      </span>
    </div>
  );
}

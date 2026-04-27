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

  const handleMouseLeave = () => {
    setHoverX(null);
  };

  return (
    <div className="flex items-center gap-3 px-4">
      <span className="w-14 text-right text-xs text-gray-400 tabular-nums">
        {formatTime(position)}
      </span>

      <div
        ref={barRef}
        className="group relative flex h-6 flex-1 cursor-pointer items-center"
        onClick={handleClick}
        onMouseMove={handleMouseMove}
        onMouseLeave={handleMouseLeave}
      >
        {/* Track background */}
        <div className="h-1 w-full rounded-full bg-gray-700 transition-all group-hover:h-1.5">
          {/* Progress fill */}
          <motion.div
            className="h-full rounded-full bg-gradient-to-r from-brand-purple to-brand-pink"
            style={{ width: `${progress}%` }}
            transition={{ duration: 0.1 }}
          />
        </div>

        {/* Hover tooltip */}
        {hoverX !== null && duration > 0 && (
          <div
            className="pointer-events-none absolute -top-8 -translate-x-1/2 rounded bg-gray-800 px-2 py-0.5 text-xs text-gray-200 shadow"
            style={{ left: hoverX }}
          >
            {formatTime(hoverTime)}
          </div>
        )}

        {/* Thumb */}
        {state !== "stopped" && (
          <motion.div
            className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 shadow transition-all group-hover:opacity-100 group-hover:shadow-[0_0_8px_rgba(124,58,237,0.6)]"
            style={{ left: `${progress}%` }}
          />
        )}
      </div>

      <span className="w-14 text-xs text-gray-400 tabular-nums">
        {formatTime(duration)}
      </span>
    </div>
  );
}

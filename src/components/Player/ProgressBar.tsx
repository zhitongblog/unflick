import { useRef, useState, useCallback } from "react";
import { usePlayerStore } from "../../stores/playerStore";

function formatTime(seconds: number): string {
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  if (h > 0) return `${h}:${pad(m)}:${pad(sec)}`;
  return `${pad(m)}:${pad(sec)}`;
}

export default function ProgressBar() {
  const { position, duration, seek, state } = usePlayerStore();
  const barRef = useRef<HTMLDivElement>(null);
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [hoverTime, setHoverTime] = useState<number>(0);
  const [isHovering, setIsHovering] = useState(false);
  const [isDragging, setIsDragging] = useState(false);

  const progress = duration > 0 ? (position / duration) * 100 : 0;

  const getTimeFromX = useCallback(
    (clientX: number) => {
      if (!barRef.current || duration <= 0) return 0;
      const rect = barRef.current.getBoundingClientRect();
      return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width)) * duration;
    },
    [duration],
  );

  const handleClick = (e: React.MouseEvent) => {
    if (state === "stopped") return;
    seek(getTimeFromX(e.clientX));
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!barRef.current) return;
    const rect = barRef.current.getBoundingClientRect();
    setHoverX(e.clientX - rect.left);
    setHoverTime(getTimeFromX(e.clientX));
    if (isDragging && state !== "stopped") {
      seek(getTimeFromX(e.clientX));
    }
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    if (state === "stopped") return;
    setIsDragging(true);
    seek(getTimeFromX(e.clientX));

    const onMouseUp = () => {
      setIsDragging(false);
      window.removeEventListener("mouseup", onMouseUp);
    };
    window.addEventListener("mouseup", onMouseUp);
  };

  const active = isHovering || isDragging;
  const trackHeight = active ? 6 : 3;

  return (
    <div className="flex items-center gap-3 px-4 py-1">
      <span className="w-[52px] text-right text-[11px] tabular-nums text-white/30 font-medium">
        {formatTime(position)}
      </span>

      <div
        ref={barRef}
        className="group relative flex flex-1 cursor-pointer items-center py-2"
        onClick={handleClick}
        onMouseMove={handleMouseMove}
        onMouseDown={handleMouseDown}
        onMouseEnter={() => setIsHovering(true)}
        onMouseLeave={() => { setIsHovering(false); setHoverX(null); }}
      >
        {/* Track background */}
        <div
          className="relative w-full overflow-hidden rounded-full transition-all duration-200 ease-out"
          style={{ height: `${trackHeight}px`, background: "rgba(255,255,255,0.08)" }}
        >
          {/* Hover fill preview */}
          {hoverX !== null && barRef.current && (
            <div
              className="absolute inset-y-0 left-0 rounded-full"
              style={{
                width: `${(hoverX / barRef.current.getBoundingClientRect().width) * 100}%`,
                background: "rgba(255,255,255,0.06)",
              }}
            />
          )}

          {/* Progress fill */}
          <div
            className="absolute inset-y-0 left-0 rounded-full"
            style={{
              width: `${progress}%`,
              background: "linear-gradient(90deg, #7C3AED, #DB2777)",
              transition: isDragging ? "none" : "width 0.15s linear",
            }}
          />
        </div>

        {/* Playhead */}
        {state !== "stopped" && (
          <div
            className="pointer-events-none absolute top-1/2"
            style={{
              left: `${progress}%`,
              transform: "translate(-50%, -50%)",
              width: active ? "14px" : "0px",
              height: active ? "14px" : "0px",
              borderRadius: "50%",
              background: "#fff",
              boxShadow: active
                ? "0 0 0 3px rgba(124,58,237,0.5), 0 0 12px rgba(124,58,237,0.4)"
                : "none",
              opacity: active ? 1 : 0,
              transition: "all 0.15s ease-out",
            }}
          />
        )}

        {/* Hover time tooltip */}
        {hoverX !== null && duration > 0 && (
          <div
            className="pointer-events-none absolute z-10 flex flex-col items-center"
            style={{ left: hoverX, bottom: "calc(100% + 6px)", transform: "translateX(-50%)" }}
          >
            <div className="glass-elevated rounded-md px-2 py-0.5 text-[11px] font-semibold text-white tabular-nums whitespace-nowrap shadow-lg">
              {formatTime(hoverTime)}
            </div>
            <div
              style={{
                width: 0, height: 0,
                borderLeft: "4px solid transparent",
                borderRight: "4px solid transparent",
                borderTop: "4px solid var(--bg-elevated, rgba(17,24,39,0.8))",
              }}
            />
          </div>
        )}
      </div>

      <span className="w-[52px] text-[11px] tabular-nums text-white/30 font-medium">
        {formatTime(duration)}
      </span>
    </div>
  );
}

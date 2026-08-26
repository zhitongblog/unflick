import { useRef, useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";
import { formatTime } from "../../lib/format";

/** Preview width in CSS pixels. Matches what the backend renders. */
const THUMB_WIDTH = 160;

/**
 * How long the pointer must settle before we ask for a preview. Scrubbing
 * across a timeline fires mousemove per pixel; without this, a single sweep
 * would queue hundreds of extractions for frames nobody looks at.
 */
const HOVER_DEBOUNCE_MS = 90;

/**
 * Closest already-fetched preview to `seconds`, if one is near enough to be
 * honest about what's there. The window is generous because it only fills
 * the gap until the real one arrives, and a roughly-right frame reads far
 * better than a tooltip that flickers between image and no image.
 */
function nearestCached(cache: Map<number, string>, seconds: number): string | null {
  let best: string | null = null;
  let bestGap = Infinity;
  for (const [bucket, url] of cache) {
    const gap = Math.abs(bucket - seconds);
    if (gap < bestGap) {
      bestGap = gap;
      best = url;
    }
  }
  return bestGap <= 30 ? best : null;
}

export default function ProgressBar() {
  const { position, duration, seek, state, chapters, abLoop, bookmarks, file } =
    usePlayerStore();
  const barRef = useRef<HTMLDivElement>(null);
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [hoverTime, setHoverTime] = useState<number>(0);
  const [isHovering, setIsHovering] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [thumb, setThumb] = useState<string | null>(null);

  // Previews already come from a disk cache, but a second in-memory cache
  // keyed by the backend's bucket avoids an IPC round trip while sweeping
  // back and forth over the same stretch of timeline.
  const thumbCache = useRef<Map<number, string>>(new Map());
  const thumbTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Monotonic id so a slow response for an old position can't overwrite the
  // preview for where the pointer is now.
  const requestId = useRef(0);
  // Streams have no previews. One failure is enough to stop trying for this
  // file, rather than spawning a doomed ffmpeg call on every hover.
  const unavailable = useRef(false);

  // A new file invalidates everything: different frames, different buckets.
  useEffect(() => {
    thumbCache.current.clear();
    unavailable.current = false;
    setThumb(null);
  }, [file]);

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

  /// Ask for the preview at `seconds`, debounced, newest-wins.
  const requestThumb = useCallback(
    (seconds: number) => {
      if (unavailable.current || state === "stopped" || duration <= 0) return;

      if (thumbTimer.current) clearTimeout(thumbTimer.current);
      thumbTimer.current = setTimeout(() => {
        const id = ++requestId.current;
        invoke<{ position: number; dataUrl: string }>("thumbnail_at", {
          position: seconds,
          width: THUMB_WIDTH,
        })
          .then((res) => {
            thumbCache.current.set(res.position, res.dataUrl);
            // Drop the answer if the pointer has moved on since we asked.
            if (id === requestId.current) setThumb(res.dataUrl);
          })
          .catch(() => {
            // Expected for streams and for files ffmpeg can't seek. Not
            // worth a toast — the tooltip simply shows the time alone.
            unavailable.current = true;
            if (id === requestId.current) setThumb(null);
          });
      }, HOVER_DEBOUNCE_MS);
    },
    [state, duration],
  );

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!barRef.current) return;
    const rect = barRef.current.getBoundingClientRect();
    const time = getTimeFromX(e.clientX);
    setHoverX(e.clientX - rect.left);
    setHoverTime(time);

    // Show a cached neighbour instantly while the debounced request for
    // the exact bucket is still in flight — scrubbing then previews
    // continuously instead of blinking empty between fetches.
    const cached = nearestCached(thumbCache.current, time);
    if (cached) setThumb(cached);

    requestThumb(time);

    if (isDragging && state !== "stopped") {
      seek(time);
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

  /** Percent along the track for a time in seconds. */
  const pct = (seconds: number) =>
    duration > 0 ? Math.max(0, Math.min(100, (seconds / duration) * 100)) : 0;

  // The chapter whose range contains the hovered time — shown in the
  // tooltip so scrubbing a long file tells you where you're landing, not
  // just when.
  const hoveredChapter =
    chapters.length > 0 && hoverX !== null
      ? [...chapters].reverse().find((c) => hoverTime >= c.time)
      : undefined;

  // A-B loop shading. Drawn only once both bounds exist; a lone A point
  // gets a marker instead (below), since a region needs two edges.
  const loopStart = abLoop.a !== null ? pct(abLoop.a) : null;
  const loopEnd = abLoop.b !== null ? pct(abLoop.b) : null;

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
        onMouseLeave={() => {
          setIsHovering(false);
          setHoverX(null);
          setThumb(null);
          if (thumbTimer.current) clearTimeout(thumbTimer.current);
          // Invalidate anything still in flight so it can't repaint a
          // preview after the tooltip is gone.
          requestId.current++;
        }}
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

          {/* A-B loop region, painted over the fill so the looped span
              reads as "this part repeats" at a glance. */}
          {loopStart !== null && loopEnd !== null && (
            <div
              className="pointer-events-none absolute inset-y-0"
              style={{
                left: `${Math.min(loopStart, loopEnd)}%`,
                width: `${Math.abs(loopEnd - loopStart)}%`,
                background: "rgba(255,255,255,0.32)",
              }}
            />
          )}

          {/* Chapter dividers. Skipped for the chapter starting at 0 —
              a tick flush against the left edge just looks like noise. */}
          {chapters.map((c) =>
            c.time <= 0 ? null : (
              <div
                key={c.index}
                className="pointer-events-none absolute inset-y-0"
                style={{
                  left: `${pct(c.time)}%`,
                  width: "2px",
                  transform: "translateX(-1px)",
                  background: "rgba(0,0,0,0.55)",
                }}
              />
            ),
          )}
        </div>

        {/* Bookmark pins. Above the bar rather than in it: chapter ticks
            already divide the track, and a second set of marks inside it
            would be read as more of the same. Clicking one jumps there —
            the point of putting them on the timeline at all. */}
        {bookmarks.map((b) => (
          <button
            key={b.id}
            className="absolute top-1/2 h-2 w-2 rounded-sm transition-transform hover:scale-125"
            style={{
              left: `${pct(b.position)}%`,
              transform: "translate(-50%, -160%)",
              background: "linear-gradient(135deg, #7C3AED, #DB2777)",
            }}
            title={b.name ?? formatTime(b.position)}
            // The track underneath turns a press into "seek to where you
            // clicked". A pin means "exactly the bookmark", so it has to
            // swallow the press as well as the click — otherwise the bar
            // seeks to the approximate spot first and the jump lands twice.
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              void usePlayerStore.getState().gotoBookmark(b);
            }}
          />
        ))}

        {/* Loop bound markers. Visible even when only A is set, so the
            user can see the pending point before pressing ]. */}
        {[
          { value: abLoop.a, label: "A" },
          { value: abLoop.b, label: "B" },
        ].map(({ value, label }) =>
          value === null ? null : (
            <div
              key={label}
              className="pointer-events-none absolute top-1/2 flex flex-col items-center"
              style={{ left: `${pct(value)}%`, transform: "translate(-50%, -50%)" }}
            >
              <span
                className="rounded-sm px-1 text-[9px] font-bold leading-[13px] text-black"
                style={{ background: "rgba(255,255,255,0.9)" }}
              >
                {label}
              </span>
            </div>
          ),
        )}

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
            <div
              className={`glass-elevated flex flex-col items-center overflow-hidden shadow-2xl ${
                thumb ? "gap-0 rounded-lg p-1" : "gap-0.5 rounded-md px-2 py-0.5"
              }`}
            >
              {thumb && (
                <img
                  src={thumb}
                  alt=""
                  width={THUMB_WIDTH}
                  className="block rounded-md"
                  style={{ width: THUMB_WIDTH, height: "auto" }}
                  draggable={false}
                />
              )}
              <div
                className={`flex flex-col items-center whitespace-nowrap text-[11px] font-semibold tabular-nums text-white ${
                  thumb ? "px-1 pb-0.5 pt-1" : ""
                }`}
              >
                <span>{formatTime(hoverTime)}</span>
                {hoveredChapter && (
                  <span className="max-w-[220px] truncate text-[10px] font-medium text-white/60">
                    {hoveredChapter.title ?? `#${hoveredChapter.index + 1}`}
                  </span>
                )}
              </div>
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

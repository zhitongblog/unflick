import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { useCastStore } from "../stores/castStore";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";
import {
  castFileLabel,
  castProgress,
  castView,
  isPaused,
  seekTarget,
  type Renderer,
} from "../lib/cast";

/**
 * Casting popover: find a television, hand it the film, drive it there,
 * and come back.
 *
 * The panel owns no casting logic — every button is one `cast` invoke into
 * the same `core::daemon` arm `unflick cast …` drives, and what is drawn
 * here is whatever `cast status` last said. So this file is about the two
 * things a window has to get right that a command line does not:
 *
 * **Time.** Discovery takes seconds — SSDP replies are spread out on
 * purpose so a house full of devices does not answer at once — and a panel
 * that looks frozen for eight seconds is a panel people click twice. The
 * search says it is running and counts the seconds while it does.
 *
 * **Nothing found.** The likeliest outcome on a developer's desk, and the
 * one an empty box explains worst. "No televisions found" is not an answer;
 * what to check is.
 */
export default function CastMenu({ onClose }: { onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const t = useStrings();
  const file = usePlayerStore((s) => s.file);
  const {
    renderers,
    discovering,
    searched,
    session,
    connecting,
    busy,
    error,
    discover,
    castTo,
    pause,
    resume,
    stop,
    seek,
  } = useCastStore();

  /** Seconds the running search has been going, so it visibly is. */
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  // Slide the mpv overlay window out of the way while this is mounted,
  // otherwise the popover renders behind the video on Windows.
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, []);

  // Where the "every open starts from nothing known" rule is *not*: see
  // `castStore.open`, which the player bar calls on the click that opens
  // this. A mount effect would be the obvious place and the wrong one —
  // reopening a popover before its exit animation has finished makes
  // `AnimatePresence` reverse the exit instead of remounting, so the
  // effect would run once and the panel would go on showing the
  // televisions that answered the first time.

  // Follow the television while it plays. Only while something is casting:
  // `cast status` with no session touches no network at all, but with one
  // it is two SOAP round trips, and there is no reason to spend them on a
  // panel showing a renderer list.
  useEffect(() => {
    if (!session) return;
    const timer = window.setInterval(() => {
      void useCastStore.getState().refresh();
    }, 2000);
    return () => window.clearInterval(timer);
  }, [session?.renderer.id]);

  useEffect(() => {
    if (!discovering) {
      setElapsed(0);
      return;
    }
    setElapsed(0);
    const started = Date.now();
    const timer = window.setInterval(() => {
      setElapsed(Math.floor((Date.now() - started) / 1000));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [discovering]);

  const view = castView({ discovering, searched, renderers, session });

  const onStop = async () => {
    await stop();
    if (!useCastStore.getState().error) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: { kind: "info", message: t.cast.returned },
        }),
      );
      // Back to the list rather than an empty panel: whoever just stopped
      // is one click from sending it to a different television.
      void discover();
    }
  };

  const onSeek = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!session || session.duration <= 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    void seek(seekTarget(ratio, session.duration));
  };

  return (
    <motion.div
      ref={menuRef}
      data-cast-panel
      initial={{ opacity: 0, y: 8, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.95 }}
      transition={{ duration: 0.12 }}
      className="glass-elevated absolute bottom-full right-0 mb-2 max-h-96 w-80 overflow-y-auto rounded-xl py-1.5 shadow-2xl"
    >
      <p className="px-3 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-widest text-white/25">
        {t.cast.title}
      </p>

      {/* ── The search, visibly running ─────────────────────────────── */}
      {view === "searching" && (
        <div data-cast-searching className="px-3 py-2">
          <div className="flex items-center gap-2">
            <span className="h-3 w-3 flex-shrink-0 animate-spin rounded-full border-[1.5px] border-white/15 border-t-brand-purple" />
            <p className="flex-1 text-[11px] text-white/60">{t.cast.searching}</p>
            <span className="flex-shrink-0 text-[10px] tabular-nums text-white/25">
              {t.cast.elapsed.replace("{seconds}", String(elapsed))}
            </span>
          </div>
          <p className="mt-1.5 text-[10px] leading-relaxed text-white/30">
            {t.cast.searchingHint}
          </p>
        </div>
      )}

      {/* ── Nothing answered ────────────────────────────────────────── */}
      {view === "empty" && (
        <div data-cast-empty className="px-3 py-2">
          <p className="text-[11px] font-medium text-white/70">{t.cast.noneTitle}</p>
          <p className="mt-1 text-[10px] leading-relaxed text-white/35">{t.cast.noneBody}</p>
          <button
            className="mt-2 rounded-lg border border-white/10 px-2 py-1 text-[11px] text-white/60 transition-colors hover:bg-white/6 hover:text-white/90"
            onClick={() => void discover()}
          >
            {t.cast.searchAgain}
          </button>
        </div>
      )}

      {/* ── Televisions that answered ───────────────────────────────── */}
      {view === "renderers" && (
        <div data-cast-list>
          <p className="px-3 pb-1 text-[10px] text-white/30">{t.cast.pick}</p>
          {!file && (
            <p className="px-3 pb-1 text-[10px] leading-relaxed text-white/35">
              {t.cast.nothingToCast}
            </p>
          )}
          {renderers.map((r: Renderer) => (
            <button
              key={r.id}
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-white/6 disabled:opacity-30"
              disabled={!file || connecting !== null}
              title={r.name}
              onClick={() => void castTo(r)}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="flex-shrink-0 text-white/30">
                <rect x="2" y="4" width="20" height="13" rx="2" />
                <line x1="8" y1="21" x2="16" y2="21" />
              </svg>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[11px] text-white/70">{r.name}</span>
                <span className="block truncate text-[9px] tabular-nums text-white/25">
                  {r.address}
                </span>
              </span>
            </button>
          ))}
          {connecting && (
            <p className="px-3 py-1.5 text-[10px] text-white/40">
              {t.cast.connecting.replace("{name}", connecting.name)}
            </p>
          )}
          <div className="my-1 h-px bg-white/8" />
          <button
            className="w-full px-3 py-1.5 text-left text-[11px] text-white/45 transition-colors hover:bg-white/6 hover:text-white/80"
            onClick={() => void discover()}
          >
            {t.cast.searchAgain}
          </button>
        </div>
      )}

      {/* ── Driving a television ────────────────────────────────────── */}
      {view === "casting" && session && (
        <div data-cast-active className="px-3 py-1.5">
          <div className="flex items-center gap-2">
            <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-brand-purple" />
            <p className="min-w-0 flex-1 truncate text-[11px] text-white/80">
              {t.cast.playingOn.replace("{name}", session.renderer.name)}
            </p>
          </div>
          <p className="mt-0.5 truncate text-[10px] text-white/35" title={session.file}>
            {castFileLabel(session.file)}
          </p>

          <div
            data-cast-seek
            className="mt-2 h-1.5 w-full cursor-pointer rounded-full bg-white/10"
            onClick={onSeek}
          >
            <div
              className="h-full rounded-full bg-brand-purple"
              style={{ width: `${castProgress(session) * 100}%` }}
            />
          </div>
          <div className="mt-1 flex items-center justify-between text-[10px] tabular-nums text-white/30">
            <span>{formatTime(session.position)}</span>
            <span>{formatTime(session.duration)}</span>
          </div>

          <div className="mt-2 flex items-center gap-1.5">
            <button
              className="rounded-lg border border-white/10 px-2 py-1 text-[11px] text-white/70 transition-colors hover:bg-white/6 hover:text-white disabled:opacity-30"
              disabled={busy}
              onClick={() => void (isPaused(session.state) ? resume() : pause())}
            >
              {isPaused(session.state) ? t.cast.resume : t.cast.pause}
            </button>
            <button
              className="rounded-lg border border-white/10 px-2 py-1 text-[11px] text-white/70 transition-colors hover:bg-white/6 hover:text-white disabled:opacity-30"
              disabled={busy}
              title={t.cast.stopHint}
              onClick={() => void onStop()}
            >
              {t.cast.stop}
            </button>
          </div>
          <p className="mt-1.5 text-[10px] leading-relaxed text-white/25">{t.cast.stopHint}</p>
        </div>
      )}

      {/* The backend's own words. English, like every other message Rust
          composes — better verbatim than paraphrased into a guess. */}
      {error && (
        <p data-cast-error className="mt-1 px-3 py-1 text-[10px] leading-relaxed text-red-300/70">
          {t.cast.failed}: {error}
        </p>
      )}
    </motion.div>
  );
}

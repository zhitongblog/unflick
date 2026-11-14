import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";

/**
 * Chapter list popover with quick actions to generate chapters from the
 * transcript or from visual scene detection.
 */
export default function ChapterMenu({ onClose }: { onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const { chapters, seekChapter, refreshChapters } = usePlayerStore();
  const [detecting, setDetecting] = useState(false);
  const t = useStrings();

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

  const handleSelect = (index: number) => {
    void seekChapter(index);
    onClose();
  };

  const handleGenerateFromTranscript = async () => {
    try {
      const list = await invoke<unknown[]>("chapters_generate", { count: 8 });
      await refreshChapters();
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "success",
            message: t.chapters.generated.replace("{count}", String(list.length)),
          },
        }),
      );
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "error",
            message: typeof e === "string" ? e : t.chapters.generateFailed,
          },
        }),
      );
    }
  };

  const handleDetectScenes = async () => {
    if (detecting) return;
    setDetecting(true);
    try {
      const res = await invoke<{ scenes: unknown[]; applied_as_chapters: boolean }>(
        "scene_detect",
        { toChapters: true },
      );
      await refreshChapters();
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "success",
            message: t.chapters.generated.replace("{count}", String(res.scenes.length)),
          },
        }),
      );
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "error",
            message: typeof e === "string" ? e : t.chapters.generateFailed,
          },
        }),
      );
    } finally {
      setDetecting(false);
    }
  };

  const handleClearGenerated = async () => {
    try {
      await invoke("chapters_clear");
      await refreshChapters();
    } catch {
      /* ignore */
    }
  };

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, y: 8, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.95 }}
      transition={{ duration: 0.12 }}
      className="glass-elevated absolute bottom-full right-0 mb-2 max-h-80 w-64 overflow-y-auto rounded-xl py-1.5 shadow-2xl"
    >
      <p className="px-3 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-widest text-white/25">
        {t.chapters.title}
      </p>

      {chapters.length === 0 && (
        <p className="px-3 py-2 text-[11px] text-white/25">{t.chapters.none}</p>
      )}

      {chapters.map((c) => (
        <button
          key={c.index}
          className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-white/6 ${
            c.current ? "text-brand-purple font-medium" : "text-white/60"
          }`}
          onClick={() => handleSelect(c.index)}
        >
          {c.current ? (
            <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7z" />
            </svg>
          ) : (
            <span className="w-[10px]" />
          )}
          <span className="flex-1 truncate" title={c.title ?? undefined}>
            {c.title ?? `${t.chapters.title} ${c.index + 1}`}
          </span>
          <span className="flex-shrink-0 text-[10px] tabular-nums text-white/25">
            {formatTime(c.time)}
          </span>
        </button>
      ))}

      <div
        className="mt-1 pt-1"
        style={{ borderTop: "1px solid var(--border-subtle)" }}
      >
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/60 transition-colors hover:bg-white/6 hover:text-white/85"
          onClick={handleGenerateFromTranscript}
        >
          <span className="truncate">{t.chapters.fromTranscript}</span>
        </button>
        <button
          type="button"
          disabled={detecting}
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/60 transition-colors hover:bg-white/6 hover:text-white/85 disabled:opacity-40"
          onClick={handleDetectScenes}
        >
          <span className="truncate">
            {detecting ? t.chapters.detecting : t.chapters.detectScenes}
          </span>
        </button>
        {chapters.length > 0 && (
          <button
            type="button"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/40 transition-colors hover:bg-white/6 hover:text-white/70"
            onClick={handleClearGenerated}
          >
            <span className="truncate">{t.chapters.clearGenerated}</span>
          </button>
        )}
      </div>
    </motion.div>
  );
}

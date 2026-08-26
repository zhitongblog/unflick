import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useSettingsStore } from "../stores/settingsStore";
import { useStrings } from "../i18n/utils";
import { formatDelay } from "../lib/format";
import { findSubtitlesOnline } from "../lib/subtitleSearch";

export default function SubtitleMenu({ onClose }: { onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const { file, subtitles, loadSubtitle, selectSubtitle, subDelay, setSubDelay } =
    usePlayerStore();
  const { whisperMode, whisperBinaryPath, whisperModelPath } = useSettingsStore();
  const t = useStrings();

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  // Tell the App-level popup hider that this popover is mounted so the
  // mpv overlay window slides out of the way; otherwise the menu would
  // render behind the video.
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, []);

  const handleSelect = (id: number | null) => {
    selectSubtitle(id);
    onClose();
  };

  const handleLoadExternal = async () => {
    try {
      const result = await invoke<{ path: string | null }>("open_subtitle_dialog");
      if (result.path) {
        await loadSubtitle(result.path);
      }
    } catch (e) {
      window.dispatchEvent(new CustomEvent("unflick:toast", {
        detail: {
          kind: "error",
          message: typeof e === "string" ? e : "Failed to load subtitle",
        },
      }));
      return;
    }
    onClose();
  };

  const handleGenerateAi = async () => {
    if (whisperMode !== "local" || !file) return;
    // Snapshot the args before unmounting; the menu closes immediately
    // so the user can keep watching while whisper runs in the background.
    const args = {
      videoPath: file,
      mode: "local" as const,
      whisperBinary: whisperBinaryPath ?? undefined,
      modelPath: whisperModelPath ?? undefined,
    };
    onClose();
    window.dispatchEvent(new CustomEvent("unflick:toast", {
      detail: { kind: "success", message: "Generating subtitles…" },
    }));
    try {
      const result = await invoke<{ srt_path: string }>("generate_subtitles", args);
      // Use the store directly: by the time this resolves, the SubtitleMenu
      // component may have been unmounted long ago, so the captured
      // loadSubtitle closure is fine but going through getState() makes
      // the lifecycle explicit.
      await usePlayerStore.getState().loadSubtitle(result.srt_path);
      window.dispatchEvent(new CustomEvent("unflick:toast", {
        detail: { kind: "success", message: "Subtitles ready" },
      }));
    } catch (err) {
      const msg = String(err);
      window.dispatchEvent(new CustomEvent("unflick:toast", {
        detail: {
          kind: "error",
          message: `Subtitle generation failed: ${msg.length > 100 ? msg.slice(0, 100) + "…" : msg}`,
        },
      }));
    }
  };

  const hasActive = subtitles.some((t) => t.active);

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, y: 8, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.95 }}
      transition={{ duration: 0.12 }}
      className="glass-elevated absolute bottom-full right-0 mb-2 w-56 rounded-xl py-1.5 shadow-2xl"
    >
      <p className="px-3 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-widest text-white/25">
        Subtitles
      </p>

      {subtitles.length === 0 && (
        <p className="px-3 py-2 text-[11px] text-white/25">No subtitle tracks</p>
      )}

      {subtitles.length > 0 && (
        <button
          className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-white/6 ${
            !hasActive ? "text-brand-purple font-medium" : "text-white/60"
          }`}
          onClick={() => handleSelect(null)}
        >
          {!hasActive ? (
            <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" /></svg>
          ) : <span className="w-[10px]" />}
          <span className="flex-1">Off</span>
        </button>
      )}

      {subtitles.map((track) => (
        <button
          key={track.id}
          className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-white/6 ${
            track.active ? "text-brand-purple font-medium" : "text-white/60"
          }`}
          onClick={() => handleSelect(track.id)}
        >
          {track.active ? (
            <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" /></svg>
          ) : <span className="w-[10px]" />}
          <span className="flex-1 truncate" title={track.label}>{track.label}</span>
        </button>
      ))}

      {/* Timing. AI-generated tracks in particular tend to land a few
          hundred ms off, and until now there was no way to correct them. */}
      <div className="mx-2 my-1 border-t border-white/6" />
      <div className="flex items-center gap-1 px-3 py-1.5">
        <span className="flex-1 text-[11px] text-white/60">{t.subtitle.delay}</span>
        <button
          className="flex h-5 w-5 items-center justify-center rounded text-[13px] leading-none text-white/50 transition-colors hover:bg-white/10 hover:text-white/90"
          title="z"
          onClick={() => setSubDelay(-0.1, true)}
        >
          −
        </button>
        <button
          className="min-w-[52px] rounded px-1 text-[11px] tabular-nums text-white/80 transition-colors hover:bg-white/10"
          title={t.subtitle.reset}
          onClick={() => setSubDelay(0, false)}
        >
          {formatDelay(subDelay)}
        </button>
        <button
          className="flex h-5 w-5 items-center justify-center rounded text-[13px] leading-none text-white/50 transition-colors hover:bg-white/10 hover:text-white/90"
          title="Shift+Z"
          onClick={() => setSubDelay(0.1, true)}
        >
          +
        </button>
      </div>

      <div className="mx-2 my-1 border-t border-white/6" />

      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/40 transition-colors hover:bg-white/6 hover:text-white/70"
        onClick={handleLoadExternal}
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" />
          <polyline points="17 8 12 3 7 8" />
          <line x1="12" y1="3" x2="12" y2="15" />
        </svg>
        Load subtitle file...
      </button>

      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/40 transition-colors hover:bg-white/6 hover:text-white/70"
        onClick={() => {
          onClose();
          findSubtitlesOnline();
        }}
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="7" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        Find subtitles online...
      </button>

      {whisperMode === "local" && file && (
        <>
          <div className="mx-2 my-1 border-t border-white/6" />
          <button
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-brand-purple transition-colors hover:bg-white/6"
            onClick={handleGenerateAi}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 2l2.4 7.4H22l-6.2 4.5 2.4 7.4L12 17l-6.2 4.3 2.4-7.4L2 9.4h7.6z" /></svg>
            Generate AI Subtitles
          </button>
        </>
      )}
    </motion.div>
  );
}

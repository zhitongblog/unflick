import { useState, useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useSettingsStore } from "../stores/settingsStore";

export default function SubtitleMenu({ onClose }: { onClose: () => void }) {
  const [isGenerating, setIsGenerating] = useState(false);
  const [generateError, setGenerateError] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const { file, subtitles, loadSubtitle, selectSubtitle } = usePlayerStore();
  const { whisperMode, whisperBinaryPath, whisperModelPath } = useSettingsStore();

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      // Don't close while a long-running transcription is in flight —
      // user needs to see the spinner and final result.
      if (isGenerating) return;
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose, isGenerating]);

  const handleSelect = (id: string | null) => {
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
      setGenerateError(typeof e === "string" ? e : "Failed to load subtitle");
      return;
    }
    onClose();
  };

  const handleGenerateAi = async () => {
    if (whisperMode !== "local") return;
    if (!file) return;
    setIsGenerating(true);
    setGenerateError(null);
    try {
      const result = await invoke<{ srt_path: string }>("generate_subtitles", {
        videoPath: file,
        mode: "local",
        whisperBinary: whisperBinaryPath ?? undefined,
        modelPath: whisperModelPath ?? undefined,
      });
      await loadSubtitle(result.srt_path);
      onClose();
    } catch (err) {
      setGenerateError(String(err));
    } finally {
      setIsGenerating(false);
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

      {whisperMode === "local" && file && (
        <>
          <div className="mx-2 my-1 border-t border-white/6" />
          <button
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-brand-purple transition-colors hover:bg-white/6"
            onClick={handleGenerateAi}
            disabled={isGenerating}
          >
            {isGenerating ? (
              <span className="h-3 w-3 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            ) : (
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 2l2.4 7.4H22l-6.2 4.5 2.4 7.4L12 17l-6.2 4.3 2.4-7.4L2 9.4h7.6z" /></svg>
            )}
            {isGenerating ? "Transcribing... (may take several minutes)" : "Generate AI Subtitles"}
          </button>
          {isGenerating && (
            <p className="px-3 py-1 text-[10px] leading-snug text-white/30">
              Long videos take longer. The window can stay open in the background — feel free to keep watching.
            </p>
          )}
        </>
      )}

      {generateError && (
        <p className="mx-3 mb-1 text-[10px] leading-snug text-red-400/80">
          {generateError.length > 120 ? generateError.slice(0, 120) + "..." : generateError}
        </p>
      )}
    </motion.div>
  );
}

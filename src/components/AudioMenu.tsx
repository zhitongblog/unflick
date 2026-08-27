import { useState, useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import { formatDelay } from "../lib/format";
import { audioTrackLabel, type AudioTrack } from "../lib/tracks";

export default function AudioMenu({
  onClose,
  onOpenEqualizer,
}: {
  onClose: () => void;
  onOpenEqualizer: () => void;
}) {
  const [tracks, setTracks] = useState<AudioTrack[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const menuRef = useRef<HTMLDivElement>(null);
  const { audioDelay, setAudioDelay } = usePlayerStore();
  const t = useStrings();

  useEffect(() => {
    invoke<AudioTrack[]>("audio_list")
      .then((t) => { setTracks(t); setIsLoading(false); })
      .catch(() => setIsLoading(false));
  }, []);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  // Move the mpv popup out of the way while this menu is mounted.
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, []);

  const handleSelect = (id: number) => {
    invoke("audio_select", { id }).catch(console.error);
    onClose();
  };

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
        Audio Tracks
      </p>

      {isLoading && (
        <div className="flex items-center justify-center py-4">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
        </div>
      )}

      {!isLoading && tracks.length === 0 && (
        <p className="px-3 py-2 text-[11px] text-white/25">No audio tracks</p>
      )}

      {!isLoading && tracks.map((track) => {
        const label = audioTrackLabel(track);
        return (
          <button
            key={track.id}
            className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-white/6 ${
              track.selected ? "text-brand-purple font-medium" : "text-white/60"
            }`}
            onClick={() => handleSelect(track.id)}
          >
            {track.selected ? (
              <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" /></svg>
            ) : <span className="w-[10px]" />}
            <span className="flex-1 truncate">{label}</span>
            {track.codec && <span className="text-[9px] text-white/20">{track.codec}</span>}
          </button>
        );
      })}

      {/* Lip-sync correction. Lives here rather than in settings because
          it's per-file and you only reach for it while watching. */}
      <div className="mx-2 my-1 border-t border-white/6" />
      <div className="flex items-center gap-1 px-3 py-1.5">
        <span className="flex-1 text-[11px] text-white/60">{t.audio.delay}</span>
        <button
          className="flex h-5 w-5 items-center justify-center rounded text-[13px] leading-none text-white/50 transition-colors hover:bg-white/10 hover:text-white/90"
          title="Ctrl+-"
          onClick={() => setAudioDelay(-0.1, true)}
        >
          −
        </button>
        <button
          className="min-w-[52px] rounded px-1 text-[11px] tabular-nums text-white/80 transition-colors hover:bg-white/10"
          title={t.audio.reset}
          onClick={() => setAudioDelay(0, false)}
        >
          {formatDelay(audioDelay)}
        </button>
        <button
          className="flex h-5 w-5 items-center justify-center rounded text-[13px] leading-none text-white/50 transition-colors hover:bg-white/10 hover:text-white/90"
          title="Ctrl+="
          onClick={() => setAudioDelay(0.1, true)}
        >
          +
        </button>
      </div>

      <div className="mx-2 my-1 border-t border-white/6" />

      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/40 transition-colors hover:bg-white/6 hover:text-white/70"
        onClick={() => {
          onClose();
          onOpenEqualizer();
        }}
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <line x1="4" y1="21" x2="4" y2="14" />
          <line x1="4" y1="10" x2="4" y2="3" />
          <line x1="12" y1="21" x2="12" y2="12" />
          <line x1="12" y1="8" x2="12" y2="3" />
          <line x1="20" y1="21" x2="20" y2="16" />
          <line x1="20" y1="12" x2="20" y2="3" />
          <line x1="1" y1="14" x2="7" y2="14" />
          <line x1="9" y1="8" x2="15" y2="8" />
          <line x1="17" y1="16" x2="23" y2="16" />
        </svg>
        Equalizer...
      </button>
    </motion.div>
  );
}

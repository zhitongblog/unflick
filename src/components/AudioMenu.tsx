import { useState, useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

interface AudioTrack {
  id: number;
  title: string | null;
  lang: string | null;
  codec: string | null;
  selected: boolean;
}

export default function AudioMenu({ onClose }: { onClose: () => void }) {
  const [tracks, setTracks] = useState<AudioTrack[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const menuRef = useRef<HTMLDivElement>(null);

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
        const label = track.title || (track.lang ? `Track ${track.id} (${track.lang})` : `Track ${track.id}`);
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
    </motion.div>
  );
}

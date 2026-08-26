import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { usePlayerStore } from "../stores/playerStore";
import { usePlaylistStore } from "../stores/playlistStore";
import { useStrings } from "../i18n/utils";
import ProgressBar from "./Player/ProgressBar";
import PlaybackControls from "./Player/PlaybackControls";

/**
 * The compact layout for a file with no picture in it.
 *
 * A video player with an mp3 loaded shows a black rectangle and a timeline,
 * which is the shape of an apology. This is the shape people already expect
 * from a music player: the cover, who it is, and the transport — nothing
 * competing for the space a video would have taken.
 *
 * Deliberately reuses `ProgressBar` and `PlaybackControls` rather than
 * growing compact copies. Two timelines would drift on the first bug fix,
 * and the A-B loop markers and bookmark pins belong here as much as they do
 * over a video.
 */
export default function MusicMode() {
  const { nowPlaying, file } = usePlayerStore();
  const items = usePlaylistStore((s) => s.items);
  const t = useStrings();

  const cover = (nowPlaying as { cover_data_url?: string } | null)?.cover_data_url;
  // mpv's media-title falls back to the file name, so this is only empty
  // when nothing is loaded at all.
  const title = nowPlaying?.title ?? t.music.noTrack;
  const artist = nowPlaying?.artist ?? t.music.unknownArtist;

  return (
    <div className="flex h-full flex-col items-center justify-between px-6 pb-3 pt-8">
      {/* The way back. The PlayerBar is hidden here — its controls do not
          fit a 380px window — so without this the only exits are a hotkey
          and a CLI command. */}
      <button
        onClick={() => invoke("toggle_music_mode").catch(console.error)}
        title={t.music.toggle}
        className="absolute right-3 top-3 rounded-md p-1.5 text-white/30 transition-colors hover:bg-white/8 hover:text-white/70"
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="2" y="4" width="20" height="14" rx="2" />
          <path d="M8 21h8" />
        </svg>
      </button>

      <motion.div
        key={cover ?? file ?? "empty"}
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.25 }}
        className="aspect-square w-full max-w-[260px] overflow-hidden rounded-2xl shadow-2xl shadow-black/50"
      >
        {cover ? (
          <img src={cover} alt="" className="h-full w-full object-cover" />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-gradient-to-br from-brand-purple/30 to-brand-pink/30">
            <svg
              width="64"
              height="64"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="text-white/25"
            >
              <path d="M9 18V5l12-2v13" />
              <circle cx="6" cy="18" r="3" />
              <circle cx="18" cy="16" r="3" />
            </svg>
          </div>
        )}
      </motion.div>

      <div className="mt-5 w-full text-center">
        <p className="truncate text-[15px] font-semibold text-white/90" title={title}>
          {title}
        </p>
        <p className="mt-1 truncate text-[12px] text-white/45" title={artist}>
          {artist}
        </p>
        {nowPlaying?.album && (
          <p className="mt-0.5 truncate text-[11px] text-white/25" title={nowPlaying.album}>
            {nowPlaying.album}
          </p>
        )}
      </div>

      {/* ProgressBar carries its own elapsed/remaining labels — a second
          pair here read as a bug, not as information. */}
      <div className="mt-auto w-full pt-6">
        <ProgressBar />

        <div className="mt-3 flex items-center justify-center">
          <PlaybackControls />
        </div>

        {items.length > 1 && (
          <p className="mt-2 text-center text-[10.5px] text-white/25">
            {t.playlist.title} · {items.length}
          </p>
        )}
      </div>
    </div>
  );
}

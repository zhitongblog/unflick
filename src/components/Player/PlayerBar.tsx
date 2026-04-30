import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";
import { useLibraryStore } from "../../stores/libraryStore";
import { usePlaylistStore } from "../../stores/playlistStore";
import ProgressBar from "./ProgressBar";
import PlaybackControls from "./PlaybackControls";
import VolumeControl from "./VolumeControl";
import VideoFilters from "../VideoFilters";
import SubtitleMenu from "../SubtitleMenu";
import AudioMenu from "../AudioMenu";

function LibraryIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </svg>
  );
}

function PipIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <rect x="12" y="9" width="8" height="6" rx="1" />
    </svg>
  );
}

function FullscreenIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="15 3 21 3 21 9" />
      <polyline points="9 21 3 21 3 15" />
      <line x1="21" y1="3" x2="14" y2="10" />
      <line x1="3" y1="21" x2="10" y2="14" />
    </svg>
  );
}

function SubtitleIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="4" width="20" height="16" rx="2" />
      <line x1="6" y1="12" x2="12" y2="12" />
      <line x1="14" y1="12" x2="18" y2="12" />
      <line x1="6" y1="16" x2="9" y2="16" />
      <line x1="11" y1="16" x2="18" y2="16" />
    </svg>
  );
}

function PlaylistIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <line x1="8" y1="6" x2="21" y2="6" />
      <line x1="8" y1="12" x2="21" y2="12" />
      <line x1="8" y1="18" x2="21" y2="18" />
      <line x1="3" y1="6" x2="3.01" y2="6" />
      <line x1="3" y1="12" x2="3.01" y2="12" />
      <line x1="3" y1="18" x2="3.01" y2="18" />
    </svg>
  );
}

function AudioIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
    </svg>
  );
}

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  const name = parts[parts.length - 1] || path;
  // Remove extension for cleaner display
  return name.replace(/\.[^/.]+$/, "");
}

const barBtnClass = (active?: boolean) =>
  `rounded-lg p-1.5 transition-all duration-150 ${
    active
      ? "text-brand-purple"
      : "text-white/35 hover:text-white/70 hover:bg-white/6"
  } active:scale-90`;

export default function PlayerBar() {
  const { file, state } = usePlayerStore();
  const toggleLibrary = useLibraryStore((s) => s.toggleLibrary);
  const { togglePlaylist, items: playlistItems } = usePlaylistStore();
  const [showSubtitleMenu, setShowSubtitleMenu] = useState(false);
  const [showAudioMenu, setShowAudioMenu] = useState(false);

  return (
    <div
      data-player-bar
      className="flex flex-col"
      style={{
        background: "linear-gradient(to top, rgba(0,0,0,0.65), rgba(0,0,0,0.3))",
        backdropFilter: "blur(24px) saturate(1.3)",
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      <ProgressBar />

      {/* Controls row */}
      <div className="flex items-center justify-between px-3 pb-2.5 pt-0.5">
        {/* Left: library + file name */}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <button
            className={barBtnClass()}
            onClick={toggleLibrary}
            title="Library (L)"
          >
            <LibraryIcon />
          </button>
          {file && state !== "stopped" ? (
            <p className="truncate text-[12px] font-medium text-white/50" title={file}>
              {extractFileName(file)}
            </p>
          ) : (
            <p className="text-[12px] text-white/20">No file loaded</p>
          )}
        </div>

        {/* Center: playback controls */}
        <div className="flex-shrink-0 px-2">
          <PlaybackControls />
        </div>

        {/* Right: feature buttons */}
        <div className="flex flex-1 items-center justify-end gap-0.5">
          <VolumeControl />

          {/* Audio */}
          <div className="relative">
            <button
              className={barBtnClass(showAudioMenu)}
              onClick={() => { setShowAudioMenu((v) => !v); setShowSubtitleMenu(false); }}
              title="Audio Tracks"
              disabled={state === "stopped"}
            >
              <AudioIcon />
            </button>
            <AnimatePresence>
              {showAudioMenu && <AudioMenu onClose={() => setShowAudioMenu(false)} />}
            </AnimatePresence>
          </div>

          {/* Subtitles */}
          <div className="relative">
            <button
              className={barBtnClass(showSubtitleMenu)}
              onClick={() => { setShowSubtitleMenu((v) => !v); setShowAudioMenu(false); }}
              title="Subtitles"
              disabled={state === "stopped"}
            >
              <SubtitleIcon />
            </button>
            <AnimatePresence>
              {showSubtitleMenu && <SubtitleMenu onClose={() => setShowSubtitleMenu(false)} />}
            </AnimatePresence>
          </div>

          {/* Playlist */}
          <button
            className={`relative ${barBtnClass()}`}
            onClick={togglePlaylist}
            title="Playlist (N)"
          >
            <PlaylistIcon />
            {playlistItems.length > 0 && (
              <span className="absolute -right-0.5 -top-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-full text-[8px] font-bold text-white leading-none"
                style={{ background: "linear-gradient(135deg, #7C3AED, #DB2777)" }}
              >
                {playlistItems.length > 9 ? "+" : playlistItems.length}
              </span>
            )}
          </button>

          {/* Video Filters */}
          <VideoFilters disabled={state === "stopped"} />

          {/* PiP */}
          <button
            className={barBtnClass()}
            onClick={() => invoke("toggle_pip").catch(console.error)}
            title="Picture-in-Picture (P)"
          >
            <PipIcon />
          </button>

          {/* Fullscreen */}
          <button
            className={barBtnClass()}
            onClick={() => invoke("set_fullscreen").catch(console.error)}
            title="Fullscreen (F)"
          >
            <FullscreenIcon />
          </button>
        </div>
      </div>
    </div>
  );
}

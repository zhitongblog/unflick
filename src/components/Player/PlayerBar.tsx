import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";
import { useLibraryStore } from "../../stores/libraryStore";
import { usePlaylistStore } from "../../stores/playlistStore";
import { useSettingsStore } from "../../stores/settingsStore";
import ProgressBar from "./ProgressBar";
import PlaybackControls from "./PlaybackControls";
import VolumeControl from "./VolumeControl";

function LibraryIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="7" height="7" />
      <rect x="14" y="3" width="7" height="7" />
      <rect x="3" y="14" width="7" height="7" />
      <rect x="14" y="14" width="7" height="7" />
    </svg>
  );
}

function PipIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <rect x="12" y="9" width="8" height="6" rx="1" />
    </svg>
  );
}

function FullscreenIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="15 3 21 3 21 9" />
      <polyline points="9 21 3 21 3 15" />
      <line x1="21" y1="3" x2="14" y2="10" />
      <line x1="3" y1="21" x2="10" y2="14" />
    </svg>
  );
}

function SubtitleIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
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
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
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
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
    </svg>
  );
}

interface SubtitleTrack {
  id: number;
  title: string | null;
  lang: string | null;
  external: boolean;
  selected: boolean;
}

interface AudioTrack {
  id: number;
  title: string | null;
  lang: string | null;
  codec: string | null;
  selected: boolean;
}

function AiSparkleIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 2l2.4 7.4H22l-6.2 4.5 2.4 7.4L12 17l-6.2 4.3 2.4-7.4L2 9.4h7.6z" />
    </svg>
  );
}

function SubtitleMenu({ onClose }: { onClose: () => void }) {
  const [tracks, setTracks] = useState<SubtitleTrack[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isGenerating, setIsGenerating] = useState(false);
  const [generateError, setGenerateError] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const { file } = usePlayerStore();
  const { whisperMode, whisperBinaryPath, whisperModelPath, openaiApiKey, toggleSettings } =
    useSettingsStore();

  useEffect(() => {
    invoke<SubtitleTrack[]>("subtitle_list")
      .then((t) => {
        setTracks(t);
        setIsLoading(false);
      })
      .catch(() => setIsLoading(false));
  }, []);

  // Close on outside click
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  const handleSelect = (id: number) => {
    invoke("subtitle_select", { id }).catch(console.error);
    onClose();
  };

  const handleLoadExternal = async () => {
    try {
      const result = await invoke<{ path: string | null }>("open_subtitle_dialog");
      if (result.path) {
        await invoke("subtitle_load", { path: result.path });
      }
    } catch {
      // open_subtitle_dialog or subtitle_load not yet available
    }
    onClose();
  };

  const handleGenerateAi = async () => {
    if (whisperMode === "off") {
      toggleSettings();
      onClose();
      return;
    }
    if (!file) return;

    setIsGenerating(true);
    setGenerateError(null);
    try {
      const result = await invoke<{ srt_path: string }>("generate_subtitles", {
        videoPath: file,
        mode: whisperMode,
        whisperBinary: whisperBinaryPath ?? undefined,
        modelPath: whisperModelPath ?? undefined,
        apiKey: openaiApiKey ?? undefined,
      });
      await invoke("subtitle_load", { path: result.srt_path });
      onClose();
    } catch (err) {
      setGenerateError(String(err));
    } finally {
      setIsGenerating(false);
    }
  };

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 8 }}
      transition={{ duration: 0.12 }}
      className="absolute bottom-full right-0 mb-2 w-52 rounded-lg border border-gray-700 bg-gray-900 py-1 shadow-xl"
    >
      <p className="px-3 pb-1 pt-1.5 text-[10px] font-medium uppercase tracking-wider text-gray-600">
        Subtitles
      </p>

      {isLoading && (
        <div className="flex items-center justify-center py-4">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
        </div>
      )}

      {!isLoading && tracks.length === 0 && (
        <p className="px-3 py-2 text-xs text-gray-500">No subtitle tracks</p>
      )}

      {!isLoading &&
        tracks.map((track) => {
          const label =
            track.title ||
            (track.lang ? `Track ${track.id} (${track.lang})` : `Track ${track.id}`);
          return (
            <button
              key={track.id}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-gray-800 ${
                track.selected ? "text-brand-purple font-medium" : "text-gray-300"
              }`}
              onClick={() => handleSelect(track.id)}
            >
              {track.selected && (
                <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                </svg>
              )}
              <span className={track.selected ? "" : "ml-[14px]"}>{label}</span>
              {track.external && (
                <span className="ml-auto text-[10px] text-gray-600">ext</span>
              )}
            </button>
          );
        })}

      <div className="mx-2 my-1 border-t border-gray-800" />
      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
        onClick={handleLoadExternal}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" />
          <polyline points="17 8 12 3 7 8" />
          <line x1="12" y1="3" x2="12" y2="15" />
        </svg>
        Load subtitle file...
      </button>

      <div className="mx-2 my-1 border-t border-gray-800" />
      <button
        className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-gray-800 ${
          whisperMode === "off"
            ? "text-gray-600 hover:text-gray-400"
            : "text-brand-purple hover:text-brand-purple"
        }`}
        onClick={handleGenerateAi}
        disabled={isGenerating}
      >
        {isGenerating ? (
          <span className="h-3 w-3 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
        ) : (
          <AiSparkleIcon />
        )}
        {isGenerating
          ? "Generating..."
          : whisperMode === "off"
          ? "Generate AI Subtitles (configure in Settings...)"
          : "Generate AI Subtitles"}
      </button>

      {generateError && (
        <p className="mx-3 mb-1.5 text-[10px] leading-snug text-red-400">
          {generateError.length > 120 ? generateError.slice(0, 120) + "…" : generateError}
        </p>
      )}
    </motion.div>
  );
}

function AudioMenu({ onClose }: { onClose: () => void }) {
  const [tracks, setTracks] = useState<AudioTrack[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<AudioTrack[]>("audio_list")
      .then((t) => {
        setTracks(t);
        setIsLoading(false);
      })
      .catch(() => setIsLoading(false));
  }, []);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
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
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 8 }}
      transition={{ duration: 0.12 }}
      className="absolute bottom-full right-0 mb-2 w-52 rounded-lg border border-gray-700 bg-gray-900 py-1 shadow-xl"
    >
      <p className="px-3 pb-1 pt-1.5 text-[10px] font-medium uppercase tracking-wider text-gray-600">
        Audio Tracks
      </p>

      {isLoading && (
        <div className="flex items-center justify-center py-4">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
        </div>
      )}

      {!isLoading && tracks.length === 0 && (
        <p className="px-3 py-2 text-xs text-gray-500">No audio tracks</p>
      )}

      {!isLoading &&
        tracks.map((track) => {
          const label =
            track.title ||
            (track.lang ? `Track ${track.id} (${track.lang})` : `Track ${track.id}`);
          return (
            <button
              key={track.id}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-gray-800 ${
                track.selected ? "text-brand-purple font-medium" : "text-gray-300"
              }`}
              onClick={() => handleSelect(track.id)}
            >
              {track.selected && (
                <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                </svg>
              )}
              <span className={track.selected ? "" : "ml-[14px]"}>{label}</span>
              {track.codec && (
                <span className="ml-auto text-[10px] text-gray-600">{track.codec}</span>
              )}
            </button>
          );
        })}
    </motion.div>
  );
}

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

export default function PlayerBar() {
  const { file, state } = usePlayerStore();
  const toggleLibrary = useLibraryStore((s) => s.toggleLibrary);
  const { togglePlaylist, items: playlistItems } = usePlaylistStore();
  const [showSubtitleMenu, setShowSubtitleMenu] = useState(false);
  const [showAudioMenu, setShowAudioMenu] = useState(false);

  return (
    <motion.div
      data-player-bar
      className="flex flex-col border-t border-white/5 bg-black/60 backdrop-blur-xl"
      initial={{ y: 20, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      transition={{ duration: 0.3 }}
    >
      {/* Progress bar */}
      <ProgressBar />

      {/* Controls row */}
      <div className="flex items-center justify-between px-4 pb-3 pt-1">
        {/* Left: library button + file name */}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <motion.button
            className="flex-shrink-0 rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={toggleLibrary}
            whileTap={{ scale: 0.9 }}
            title="Media Library (L)"
          >
            <LibraryIcon />
          </motion.button>
          {file && state !== "stopped" ? (
            <p className="truncate text-sm text-gray-300" title={file}>
              {extractFileName(file)}
            </p>
          ) : (
            <p className="text-sm text-gray-600">No file loaded</p>
          )}
        </div>

        {/* Center: playback controls */}
        <div className="flex-shrink-0 px-4">
          <PlaybackControls />
        </div>

        {/* Right: volume + window controls */}
        <div className="flex flex-1 items-center justify-end gap-1">
          <VolumeControl />

          {/* Audio track button */}
          <div className="relative">
            <motion.button
              className={`rounded-lg p-1.5 transition-colors hover:bg-gray-800 ${
                showAudioMenu ? "text-brand-purple" : "text-gray-400 hover:text-gray-200"
              }`}
              onClick={() => {
                setShowAudioMenu((v) => !v);
                setShowSubtitleMenu(false);
              }}
              whileTap={{ scale: 0.9 }}
              title="Audio Tracks (A)"
              disabled={state === "stopped"}
            >
              <AudioIcon />
            </motion.button>
            <AnimatePresence>
              {showAudioMenu && (
                <AudioMenu onClose={() => setShowAudioMenu(false)} />
              )}
            </AnimatePresence>
          </div>

          {/* Subtitle button */}
          <div className="relative">
            <motion.button
              className={`rounded-lg p-1.5 transition-colors hover:bg-gray-800 ${
                showSubtitleMenu ? "text-brand-purple" : "text-gray-400 hover:text-gray-200"
              }`}
              onClick={() => {
                setShowSubtitleMenu((v) => !v);
                setShowAudioMenu(false);
              }}
              whileTap={{ scale: 0.9 }}
              title="Subtitles"
              disabled={state === "stopped"}
            >
              <SubtitleIcon />
            </motion.button>
            <AnimatePresence>
              {showSubtitleMenu && (
                <SubtitleMenu onClose={() => setShowSubtitleMenu(false)} />
              )}
            </AnimatePresence>
          </div>

          {/* Playlist button */}
          <motion.button
            className={`relative rounded-lg p-1.5 transition-colors hover:bg-gray-800 hover:text-gray-200 ${
              playlistItems.length > 0 ? "text-gray-300" : "text-gray-400"
            }`}
            onClick={togglePlaylist}
            whileTap={{ scale: 0.9 }}
            title="Playlist (N)"
          >
            <PlaylistIcon />
            {playlistItems.length > 0 && (
              <span className="absolute -right-0.5 -top-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-brand-purple text-[9px] font-bold text-white leading-none">
                {playlistItems.length > 9 ? "9+" : playlistItems.length}
              </span>
            )}
          </motion.button>

          <motion.button
            className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={() => invoke("toggle_pip").catch(console.error)}
            whileTap={{ scale: 0.9 }}
            title="Picture-in-Picture (P)"
          >
            <PipIcon />
          </motion.button>
          <motion.button
            className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
            onClick={() => invoke("set_fullscreen").catch(console.error)}
            whileTap={{ scale: 0.9 }}
            title="Fullscreen (F)"
          >
            <FullscreenIcon />
          </motion.button>
        </div>
      </div>
    </motion.div>
  );
}

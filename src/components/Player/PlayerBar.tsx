import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../../stores/playerStore";
import { useLibraryStore } from "../../stores/libraryStore";
import { usePlaylistStore } from "../../stores/playlistStore";
import { useSettingsStore } from "../../stores/settingsStore";
import ProgressBar from "./ProgressBar";
import PlaybackControls from "./PlaybackControls";
import VolumeControl from "./VolumeControl";
import VideoFilters from "../VideoFilters";
import SubtitleMenu from "../SubtitleMenu";
import AudioMenu from "../AudioMenu";

// Same platform branch as App.tsx: Windows uses Win32 native menus
// (show_native_context_menu), macOS / Linux use the React popover
// components that float above the subview-rendered popup naturally.
const IS_WINDOWS =
  typeof window !== "undefined" && /Win(dows|32|64)/i.test(navigator.userAgent);

type NativeItem = { label: string; separator: boolean; disabled: boolean };
type NativeAction = (() => void | Promise<void>) | null;

async function showNativeMenuAt(
  btn: HTMLElement,
  items: NativeItem[],
  actions: NativeAction[],
) {
  const rect = btn.getBoundingClientRect();
  const x = Math.round(window.screenX + rect.left);
  const y = Math.round(window.screenY + rect.top); // top of button; menu opens above
  try {
    const selected = await invoke<number | null>("show_native_context_menu", {
      items,
      x,
      y,
      above: true,
    });
    if (selected != null) {
      const a = actions[selected];
      if (a) await a();
    }
  } catch (err) {
    console.error("[native menu] failed:", err);
  }
}

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
  // React-popover state used by macOS / Linux. On Windows these stay
  // false because the button onClicks short-circuit to native menus.
  const [showSubtitleMenu, setShowSubtitleMenu] = useState(false);
  const [showAudioMenu, setShowAudioMenu] = useState(false);

  // Native menu helpers: build the items + actions for subtitle/audio buttons.
  // Done at click time so the lists reflect current state.
  const handleSubtitleButton = async (e: React.MouseEvent<HTMLButtonElement>) => {
    if (!IS_WINDOWS) {
      setShowSubtitleMenu((v) => !v);
      setShowAudioMenu(false);
      return;
    }
    const btn = e.currentTarget;
    const ps = usePlayerStore.getState();
    const ss = useSettingsStore.getState();
    const subs = ps.subtitles;
    const hasActive = subs.some((t) => t.active);

    const items: NativeItem[] = [];
    const actions: NativeAction[] = [];

    items.push({ label: hasActive ? "Off" : "✓ Off", separator: false, disabled: false });
    actions.push(() => ps.selectSubtitle(null));

    for (const t of subs) {
      items.push({
        label: (t.active ? "✓ " : "") + t.label,
        separator: false,
        disabled: false,
      });
      actions.push(() => ps.selectSubtitle(t.id));
    }

    items.push({ label: "", separator: true, disabled: false });
    actions.push(null);

    items.push({ label: "Load subtitle file…", separator: false, disabled: false });
    actions.push(async () => {
      try {
        const r = await invoke<{ path: string | null }>("open_subtitle_dialog");
        if (r.path) await ps.loadSubtitle(r.path);
      } catch (err) {
        window.dispatchEvent(new CustomEvent("unflick:toast", {
          detail: { kind: "error", message: `Load failed: ${String(err).slice(0, 100)}` },
        }));
      }
    });

    if (ss.whisperMode === "local" && ps.file) {
      items.push({ label: "Generate AI Subtitles", separator: false, disabled: false });
      const args = {
        videoPath: ps.file,
        mode: "local" as const,
        whisperBinary: ss.whisperBinaryPath ?? undefined,
        modelPath: ss.whisperModelPath ?? undefined,
      };
      actions.push(async () => {
        // Dispatch lifecycle events to App.tsx's persistent banner.
        // Toasts auto-dismiss too fast for a multi-minute whisper run.
        console.log("[unflick] subtitle generation invoked", args);
        window.dispatchEvent(new CustomEvent("unflick:gen-start"));
        try {
          const result = await invoke<{ srt_path: string }>("generate_subtitles", args);
          console.log("[unflick] generate_subtitles returned", result);
          await usePlayerStore.getState().loadSubtitle(result.srt_path);
          window.dispatchEvent(new CustomEvent("unflick:gen-success"));
        } catch (err) {
          console.error("[unflick] generate_subtitles failed:", err);
          window.dispatchEvent(new CustomEvent("unflick:gen-error", {
            detail: { message: String(err) },
          }));
        }
      });
    }

    await showNativeMenuAt(btn, items, actions);
  };

  const handleAudioButton = async (e: React.MouseEvent<HTMLButtonElement>) => {
    if (!IS_WINDOWS) {
      setShowAudioMenu((v) => !v);
      setShowSubtitleMenu(false);
      return;
    }
    const btn = e.currentTarget;
    type AudioTrack = { id: number; label: string; active: boolean };
    let tracks: AudioTrack[] = [];
    try {
      tracks = await invoke<AudioTrack[]>("audio_list");
    } catch {
      tracks = [];
    }

    const items: NativeItem[] = [];
    const actions: NativeAction[] = [];

    if (tracks.length === 0) {
      items.push({ label: "No audio tracks", separator: false, disabled: true });
      actions.push(null);
    } else {
      for (const t of tracks) {
        items.push({
          label: (t.active ? "✓ " : "") + t.label,
          separator: false,
          disabled: false,
        });
        actions.push(() => {
          invoke("audio_select", { id: t.id }).catch(console.error);
        });
      }
    }

    await showNativeMenuAt(btn, items, actions);
  };

  return (
    <div
      data-player-bar
      className="flex flex-col"
      style={{
        // v0.8: opaque chrome. mpv renders directly to a child window
        // beneath the WebView, and Win32 doesn't blend WebView's α with
        // a sibling DX surface. Anything not fully opaque on top of the
        // video region would let the GL clear color bleed through. The
        // gradient goes from full-black at the bottom to a slightly
        // lighter solid at the top so the bar still has visual weight
        // and a clean edge against the video region above it.
        background: "linear-gradient(to top, #0a0a0f, #1a1a26)",
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

          {/* Audio — Win32 native menu on Windows; React popover on
              macOS / Linux (popup is below the WebView there, so the
              popover floats above the video naturally). */}
          <div className="relative">
            <button
              className={barBtnClass(showAudioMenu)}
              onClick={handleAudioButton}
              title="Audio Tracks"
              disabled={state === "stopped"}
            >
              <AudioIcon />
            </button>
            {!IS_WINDOWS && (
              <AnimatePresence>
                {showAudioMenu && <AudioMenu onClose={() => setShowAudioMenu(false)} />}
              </AnimatePresence>
            )}
          </div>

          {/* Subtitles — same platform branch. */}
          <div className="relative">
            <button
              className={barBtnClass(showSubtitleMenu)}
              onClick={handleSubtitleButton}
              title="Subtitles"
              disabled={state === "stopped"}
            >
              <SubtitleIcon />
            </button>
            {!IS_WINDOWS && (
              <AnimatePresence>
                {showSubtitleMenu && <SubtitleMenu onClose={() => setShowSubtitleMenu(false)} />}
              </AnimatePresence>
            )}
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

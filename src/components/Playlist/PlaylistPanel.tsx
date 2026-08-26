import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlaylistStore, PlaylistItem } from "../../stores/playlistStore";
import { useStrings } from "../../i18n/utils";
import { usePlayerStore } from "../../stores/playerStore";

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  const name = parts[parts.length - 1] || path;
  return name.replace(/\.[^/.]+$/, "");
}

function PlaylistEntry({
  item,
  onPlay,
  onRemove,
}: {
  item: PlaylistItem;
  onPlay: (index: number) => void;
  onRemove: (index: number) => void;
}) {
  const displayTitle = item.title || extractFileName(item.path);

  return (
    <div
      className={`group flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left transition-all duration-150 ${
        item.current
          ? "bg-brand-purple/10 border border-brand-purple/15"
          : "hover:bg-white/4"
      }`}
    >
      {/* Play indicator */}
      <div className="flex w-5 flex-shrink-0 items-center justify-center">
        {item.current ? (
          <span className="text-brand-purple">
            <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
          </span>
        ) : (
          <span className="text-[10px] tabular-nums text-white/15 group-hover:hidden font-medium">
            {item.index + 1}
          </span>
        )}
      </div>

      {/* Title */}
      <button
        className="min-w-0 flex-1 text-left"
        onClick={() => onPlay(item.index)}
        title={item.path}
      >
        <p className={`truncate text-[12px] ${item.current ? "font-medium text-white/80" : "text-white/50 group-hover:text-white/70"}`}>
          {displayTitle}
        </p>
      </button>

      {/* Remove */}
      <button
        className="flex-shrink-0 rounded p-1 text-white/15 opacity-0 transition-all hover:bg-white/6 hover:text-white/40 group-hover:opacity-100"
        onClick={() => onRemove(item.index)}
        title="Remove"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
      </button>
    </div>
  );
}

export default function PlaylistPanel() {
  const {
    items,
    isLoading,
    fetchPlaylist,
    add,
    remove,
    clear,
    playAt,
    togglePlaylist,
    repeat,
    shuffle,
    fetchModes,
    cycleRepeat,
    toggleShuffle,
  } = usePlaylistStore();
  const t = useStrings();
  const play = usePlayerStore((s) => s.play);
  const hasFetchedRef = useRef(false);

  useEffect(() => {
    if (!hasFetchedRef.current) {
      hasFetchedRef.current = true;
      fetchPlaylist();
      fetchModes();
    }
  }, [fetchPlaylist, fetchModes]);

  const repeatLabel =
    repeat === "one"
      ? t.playlist.repeatOne
      : repeat === "all"
        ? t.playlist.repeatAll
        : t.playlist.repeatOff;

  const handleAddFile = async () => {
    try {
      // Multi-select — Shift/Ctrl-click in the dialog to pick a batch.
      const result = await invoke<{ paths: string[] }>("open_files_dialog");
      for (const p of result.paths) {
        await add(p);
      }
    } catch (e) {
      console.error("Failed to open file dialog:", e);
    }
  };

  const handlePlayAt = async (index: number) => {
    await playAt(index);
    const item = items.find((i) => i.index === index);
    if (item) play(item.path);
  };

  return (
    <>
      {/* Panel — slides in from the right and shares space with the
          video region instead of overlapping it. No backdrop. */}
      <motion.div
        className="absolute bottom-0 right-0 top-0 z-30 flex w-72 flex-col"
        style={{
          background: "var(--bg-primary)",
          borderLeft: "1px solid var(--border-subtle)",
        }}
        initial={{ x: 288 }}
        animate={{ x: 0 }}
        exit={{ x: 288 }}
        transition={{ type: "spring", damping: 30, stiffness: 300 }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
          <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">
            {t.playlist.title}
          </h2>
          <div className="flex items-center gap-1">
            {items.length > 0 && (
              <button
                className="rounded-lg px-2 py-1 text-[10px] font-medium text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
                onClick={clear}
                title={t.playlist.clear}
              >
                {t.common.clear}
              </button>
            )}
            <button
              className="rounded-lg p-1.5 text-white/30 transition-colors hover:bg-white/6 hover:text-white/60"
              onClick={handleAddFile}
              title={t.playlist.addFile}
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
            </button>
            <button
              className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={togglePlaylist}
              title="Close (N)"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>
        </div>

        {/* Items */}
        <div className="flex-1 overflow-y-auto px-2 py-1.5">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            </div>
          )}

          {!isLoading && items.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-16 text-center">
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" className="text-white/10" strokeLinecap="round">
                <line x1="8" y1="6" x2="21" y2="6" /><line x1="8" y1="12" x2="21" y2="12" /><line x1="8" y1="18" x2="21" y2="18" />
                <line x1="3" y1="6" x2="3.01" y2="6" /><line x1="3" y1="12" x2="3.01" y2="12" /><line x1="3" y1="18" x2="3.01" y2="18" />
              </svg>
              <p className="text-[12px] text-white/25">{t.playlist.empty}</p>
              <button
                onClick={handleAddFile}
                className="rounded-lg bg-white/5 px-4 py-2 text-[11px] font-medium text-white/40 transition-colors hover:bg-white/8 hover:text-white/60"
              >
                {t.playlist.addFile}
              </button>
            </div>
          )}

          {!isLoading && items.map((item) => (
            <PlaylistEntry key={item.index} item={item} onPlay={handlePlayAt} onRemove={remove} />
          ))}
        </div>

        {/* Footer: item count plus the two playback-order toggles. Both
            report backend state — the Rust playlist owns repeat/shuffle so
            CLI and MCP see the same setting. */}
        {!isLoading && items.length > 0 && (
          <div
            className="flex items-center justify-between px-4 py-2"
            style={{ borderTop: "1px solid var(--border-subtle)" }}
          >
            <p className="text-[10px] text-white/15 font-medium">
              {items.length} item{items.length !== 1 ? "s" : ""}
            </p>
            <div className="flex items-center gap-1">
              <button
                className={`flex items-center gap-1 rounded-lg px-1.5 py-1 text-[10px] font-medium transition-colors hover:bg-white/6 ${
                  shuffle ? "text-brand-purple" : "text-white/25 hover:text-white/50"
                }`}
                onClick={toggleShuffle}
                title={t.playlist.shuffle}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="16 3 21 3 21 8" />
                  <line x1="4" y1="20" x2="21" y2="3" />
                  <polyline points="21 16 21 21 16 21" />
                  <line x1="15" y1="15" x2="21" y2="21" />
                  <line x1="4" y1="4" x2="9" y2="9" />
                </svg>
              </button>
              <button
                className={`flex items-center gap-1 rounded-lg px-1.5 py-1 text-[10px] font-medium transition-colors hover:bg-white/6 ${
                  repeat === "off" ? "text-white/25 hover:text-white/50" : "text-brand-purple"
                }`}
                onClick={cycleRepeat}
                title={`${t.playlist.repeat}: ${repeatLabel}`}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="17 1 21 5 17 9" />
                  <path d="M3 11V9a4 4 0 014-4h14" />
                  <polyline points="7 23 3 19 7 15" />
                  <path d="M21 13v2a4 4 0 01-4 4H3" />
                </svg>
                {/* "1" badge distinguishes repeat-one from repeat-all,
                    which otherwise share an icon. */}
                {repeat === "one" && <span className="text-[9px] leading-none">1</span>}
              </button>
            </div>
          </div>
        )}
      </motion.div>
    </>
  );
}

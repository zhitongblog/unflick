import { useEffect, useRef } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlaylistStore, PlaylistItem } from "../../stores/playlistStore";
import { usePlayerStore } from "../../stores/playerStore";

function CloseIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function AddIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14H6L5 6" />
      <path d="M10 11v6M14 11v6" />
      <path d="M9 6V4h6v2" />
    </svg>
  );
}

function PlayingIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function ListIcon() {
  return (
    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="text-gray-600">
      <line x1="8" y1="6" x2="21" y2="6" />
      <line x1="8" y1="12" x2="21" y2="12" />
      <line x1="8" y1="18" x2="21" y2="18" />
      <line x1="3" y1="6" x2="3.01" y2="6" />
      <line x1="3" y1="12" x2="3.01" y2="12" />
      <line x1="3" y1="18" x2="3.01" y2="18" />
    </svg>
  );
}

function extractFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
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
    <motion.div
      className={`group flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-left transition-colors ${
        item.current
          ? "bg-brand-purple/15 border border-brand-purple/20"
          : "hover:bg-gray-800/60"
      }`}
      whileTap={{ scale: 0.98 }}
    >
      {/* Play indicator / index */}
      <div className="flex w-5 flex-shrink-0 items-center justify-center">
        {item.current ? (
          <span className="text-brand-purple">
            <PlayingIcon />
          </span>
        ) : (
          <span className="text-xs tabular-nums text-gray-600 group-hover:hidden">
            {item.index + 1}
          </span>
        )}
      </div>

      {/* Title — clickable */}
      <button
        className="min-w-0 flex-1 text-left"
        onClick={() => onPlay(item.index)}
        title={item.path}
      >
        <p
          className={`truncate text-sm ${
            item.current ? "font-medium text-gray-100" : "text-gray-300"
          }`}
        >
          {displayTitle}
        </p>
      </button>

      {/* Remove button */}
      <motion.button
        className="flex-shrink-0 rounded p-1 text-gray-600 opacity-0 transition-colors hover:bg-gray-700 hover:text-gray-300 group-hover:opacity-100"
        onClick={() => onRemove(item.index)}
        whileTap={{ scale: 0.9 }}
        title="Remove from playlist"
      >
        <TrashIcon />
      </motion.button>
    </motion.div>
  );
}

export default function PlaylistPanel() {
  const { items, isLoading, fetchPlaylist, add, remove, clear, playAt, togglePlaylist } =
    usePlaylistStore();
  const play = usePlayerStore((s) => s.play);
  const hasFetchedRef = useRef(false);

  useEffect(() => {
    if (!hasFetchedRef.current) {
      hasFetchedRef.current = true;
      fetchPlaylist();
    }
  }, [fetchPlaylist]);

  const handleAddFile = async () => {
    try {
      const result = await invoke<{ path: string | null }>("open_file_dialog");
      if (result.path) {
        await add(result.path);
      }
    } catch (e) {
      console.error("Failed to open file dialog:", e);
    }
  };

  const handlePlayAt = async (index: number) => {
    await playAt(index);
    // Also trigger playerStore play so UI state updates
    const item = items.find((i) => i.index === index);
    if (item) {
      play(item.path);
    }
  };

  return (
    <>
      {/* Backdrop */}
      <motion.div
        className="absolute inset-0 z-20 bg-black/50"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.2 }}
        onClick={togglePlaylist}
      />

      {/* Panel — slides in from right */}
      <motion.div
        className="absolute bottom-0 right-0 top-0 z-30 flex w-72 flex-col border-l border-gray-800 bg-gray-950/95 backdrop-blur-md"
        initial={{ x: 288 }}
        animate={{ x: 0 }}
        exit={{ x: 288 }}
        transition={{ type: "spring", damping: 30, stiffness: 300 }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-800 px-4 py-3">
          <h2 className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-sm font-semibold text-transparent">
            Playlist
          </h2>
          <div className="flex items-center gap-1">
            {items.length > 0 && (
              <motion.button
                className="rounded-lg px-2 py-1 text-xs text-gray-500 transition-colors hover:bg-gray-800 hover:text-gray-300"
                onClick={clear}
                whileTap={{ scale: 0.9 }}
                title="Clear playlist"
              >
                Clear
              </motion.button>
            )}
            <motion.button
              className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
              onClick={handleAddFile}
              whileTap={{ scale: 0.9 }}
              title="Add file to playlist"
            >
              <AddIcon />
            </motion.button>
            <motion.button
              className="rounded-lg p-1 text-gray-500 transition-colors hover:bg-gray-800 hover:text-gray-300"
              onClick={togglePlaylist}
              whileTap={{ scale: 0.9 }}
              title="Close playlist (N)"
            >
              <CloseIcon />
            </motion.button>
          </div>
        </div>

        {/* Items list */}
        <div className="flex-1 overflow-y-auto px-2 py-2">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            </div>
          )}

          {!isLoading && items.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-12 text-center">
              <ListIcon />
              <p className="text-sm text-gray-500">Playlist is empty</p>
              <button
                onClick={handleAddFile}
                className="rounded-lg bg-gray-800 px-4 py-2 text-xs text-gray-300 transition-colors hover:bg-gray-700 hover:text-gray-100"
              >
                Add a file
              </button>
            </div>
          )}

          {!isLoading &&
            items.map((item) => (
              <PlaylistEntry
                key={item.index}
                item={item}
                onPlay={handlePlayAt}
                onRemove={remove}
              />
            ))}
        </div>

        {/* Footer with count */}
        {!isLoading && items.length > 0 && (
          <div className="border-t border-gray-800/50 px-4 py-2">
            <p className="text-xs text-gray-600">
              {items.length} item{items.length !== 1 ? "s" : ""}
            </p>
          </div>
        )}
      </motion.div>
    </>
  );
}

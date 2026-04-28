import { useEffect, useRef, useCallback, useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { useLibraryStore, MediaEntry } from "../../stores/libraryStore";
import { usePlayerStore } from "../../stores/playerStore";

function formatDuration(seconds: number | null): string {
  if (seconds == null) return "--:--";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
}

function formatFileSize(bytes: number | null): string {
  if (bytes == null) return "";
  if (bytes >= 1_073_741_824) {
    return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  }
  if (bytes >= 1_048_576) {
    return `${(bytes / 1_048_576).toFixed(1)} MB`;
  }
  return `${(bytes / 1024).toFixed(0)} KB`;
}

function CloseIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function FilmIcon() {
  return (
    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="text-gray-600">
      <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
      <line x1="7" y1="2" x2="7" y2="22" />
      <line x1="17" y1="2" x2="17" y2="22" />
      <line x1="2" y1="12" x2="22" y2="12" />
      <line x1="2" y1="7" x2="7" y2="7" />
      <line x1="2" y1="17" x2="7" y2="17" />
      <line x1="17" y1="7" x2="22" y2="7" />
      <line x1="17" y1="17" x2="22" y2="17" />
    </svg>
  );
}

function LibraryEntry({ entry, onPlay }: { entry: MediaEntry; onPlay: (path: string) => void }) {
  const resolution = entry.width && entry.height ? `${entry.width}\u00D7${entry.height}` : null;

  return (
    <motion.button
      className="flex w-full items-start gap-3 rounded-lg px-3 py-3 text-left transition-colors hover:bg-gray-800/60"
      onClick={() => onPlay(entry.path)}
      whileTap={{ scale: 0.98 }}
    >
      <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg bg-gray-800">
        <FilmIcon />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-gray-100" title={entry.title}>
          {entry.title}
        </p>
        <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-gray-500">
          <span>{formatDuration(entry.duration)}</span>
          {resolution && (
            <>
              <span className="text-gray-700">&middot;</span>
              <span>{resolution}</span>
            </>
          )}
          {entry.file_size != null && (
            <>
              <span className="text-gray-700">&middot;</span>
              <span>{formatFileSize(entry.file_size)}</span>
            </>
          )}
          {entry.play_count > 0 && (
            <span className="ml-auto rounded-full bg-brand-purple/20 px-2 py-0.5 text-[10px] font-medium text-brand-purple">
              {entry.play_count} play{entry.play_count !== 1 ? "s" : ""}
            </span>
          )}
        </div>
      </div>
    </motion.button>
  );
}

export default function LibraryPanel() {
  const { entries, searchQuery, isLoading, setSearchQuery, toggleLibrary, fetchLibrary, search, scanDirectory } =
    useLibraryStore();
  const play = usePlayerStore((s) => s.play);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const hasFetchedRef = useRef(false);
  const [isScanning, setIsScanning] = useState(false);

  const handleScanFolder = useCallback(async () => {
    const result = await invoke<{ path: string | null }>("open_folder_dialog");
    if (!result.path) return;
    setIsScanning(true);
    try {
      await scanDirectory(result.path);
    } finally {
      setIsScanning(false);
    }
  }, [scanDirectory]);

  // Fetch library on first mount
  useEffect(() => {
    if (!hasFetchedRef.current) {
      hasFetchedRef.current = true;
      fetchLibrary();
    }
  }, [fetchLibrary]);

  // Focus search input on mount
  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  const handleSearch = useCallback(
    (value: string) => {
      setSearchQuery(value);
      if (value.trim() === "") {
        fetchLibrary();
      } else {
        search(value);
      }
    },
    [setSearchQuery, fetchLibrary, search],
  );

  const handlePlay = useCallback(
    (path: string) => {
      play(path);
      toggleLibrary();
    },
    [play, toggleLibrary],
  );

  return (
    <>
      {/* Backdrop */}
      <motion.div
        className="absolute inset-0 z-20 bg-black/50"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.2 }}
        onClick={toggleLibrary}
      />

      {/* Panel */}
      <motion.div
        className="absolute bottom-0 left-0 top-0 z-30 flex w-80 flex-col border-r border-gray-800 bg-gray-950/95 backdrop-blur-md"
        initial={{ x: -320 }}
        animate={{ x: 0 }}
        exit={{ x: -320 }}
        transition={{ type: "spring", damping: 30, stiffness: 300 }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-800 px-4 py-3">
          <h2 className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-sm font-semibold text-transparent">
            Media Library
          </h2>
          <div className="flex items-center gap-1">
            <motion.button
              className="rounded-lg px-2 py-1 text-xs font-medium bg-gradient-to-r from-brand-purple to-brand-pink text-white transition-opacity hover:opacity-80 disabled:opacity-40"
              onClick={handleScanFolder}
              whileTap={{ scale: 0.95 }}
              disabled={isScanning}
              title="Scan a folder for media"
            >
              {isScanning ? "Scanning…" : "Scan"}
            </motion.button>
            <motion.button
              className="rounded-lg p-1 text-gray-500 transition-colors hover:bg-gray-800 hover:text-gray-300"
              onClick={toggleLibrary}
              whileTap={{ scale: 0.9 }}
              title="Close library (L)"
            >
              <CloseIcon />
            </motion.button>
          </div>
        </div>

        {/* Search bar */}
        <div className="border-b border-gray-800/50 px-4 py-3">
          <div className="flex items-center gap-2 rounded-lg bg-gray-900 px-3 py-2">
            <span className="text-gray-500">
              <SearchIcon />
            </span>
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              placeholder="Search library..."
              className="w-full bg-transparent text-sm text-gray-200 placeholder-gray-600 outline-none focus:ring-0"
            />
          </div>
        </div>

        {/* Entries list */}
        <div className="flex-1 overflow-y-auto px-2 py-2">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            </div>
          )}

          {!isLoading && entries.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-12 text-center">
              <p className="text-sm text-gray-500">No media found</p>
              <motion.button
                className="rounded-lg bg-gradient-to-r from-brand-purple to-brand-pink px-4 py-2 text-sm font-medium text-white transition-opacity hover:opacity-80 disabled:opacity-40"
                onClick={handleScanFolder}
                whileTap={{ scale: 0.97 }}
                disabled={isScanning}
              >
                {isScanning ? "Scanning…" : "Scan Folder"}
              </motion.button>
            </div>
          )}

          {!isLoading &&
            entries.map((entry) => (
              <LibraryEntry key={entry.id} entry={entry} onPlay={handlePlay} />
            ))}
        </div>

        {/* Footer with count */}
        {!isLoading && entries.length > 0 && (
          <div className="border-t border-gray-800/50 px-4 py-2">
            <p className="text-xs text-gray-600">
              {entries.length} item{entries.length !== 1 ? "s" : ""}
            </p>
          </div>
        )}
      </motion.div>
    </>
  );
}

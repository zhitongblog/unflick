import { useEffect, useRef, useCallback, useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { useLibraryStore, MediaEntry } from "../../stores/libraryStore";
import { useStrings } from "../../i18n/utils";
import { usePlayerStore } from "../../stores/playerStore";

function formatDuration(seconds: number | null): string {
  if (seconds == null) return "--:--";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function formatFileSize(bytes: number | null): string {
  if (bytes == null) return "";
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

function LibraryEntry({ entry, onPlay }: { entry: MediaEntry; onPlay: (path: string) => void }) {
  const resolution = entry.width && entry.height ? `${entry.width}\u00D7${entry.height}` : null;

  return (
    <button
      className="group flex w-full items-start gap-3 rounded-xl px-3 py-2.5 text-left transition-all duration-150 hover:bg-white/5 active:scale-[0.98]"
      onClick={() => onPlay(entry.path)}
    >
      <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg bg-white/5">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-white/20">
          <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
          <line x1="7" y1="2" x2="7" y2="22" />
          <line x1="17" y1="2" x2="17" y2="22" />
          <line x1="2" y1="12" x2="22" y2="12" />
        </svg>
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12px] font-medium text-white/70 group-hover:text-white/90" title={entry.title}>
          {entry.title}
        </p>
        <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[10px] text-white/25">
          <span>{formatDuration(entry.duration)}</span>
          {resolution && (
            <>
              <span className="text-white/10">&middot;</span>
              <span>{resolution}</span>
            </>
          )}
          {entry.file_size != null && (
            <>
              <span className="text-white/10">&middot;</span>
              <span>{formatFileSize(entry.file_size)}</span>
            </>
          )}
          {entry.play_count > 0 && (
            <span className="ml-auto rounded-full px-1.5 py-0.5 text-[9px] font-semibold text-brand-purple bg-brand-purple/10">
              {entry.play_count}x
            </span>
          )}
        </div>
      </div>
    </button>
  );
}

export default function LibraryPanel() {
  const { entries, searchQuery, isLoading, setSearchQuery, toggleLibrary, fetchLibrary, search, scanDirectory, clearLibrary } =
    useLibraryStore();
  const play = usePlayerStore((s) => s.play);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const hasFetchedRef = useRef(false);
  const [isScanning, setIsScanning] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const t = useStrings();

  const handleScanFolder = useCallback(async () => {
    const result = await invoke<{ path: string | null }>("open_folder_dialog");
    if (!result.path) return;
    setIsScanning(true);
    try { await scanDirectory(result.path); } finally { setIsScanning(false); }
  }, [scanDirectory]);

  const handleClear = useCallback(async () => {
    if (!confirmClear) {
      setConfirmClear(true);
      setTimeout(() => setConfirmClear(false), 3000);
      return;
    }
    setConfirmClear(false);
    await clearLibrary();
  }, [confirmClear, clearLibrary]);

  useEffect(() => {
    if (!hasFetchedRef.current) {
      hasFetchedRef.current = true;
      fetchLibrary();
    }
  }, [fetchLibrary]);

  useEffect(() => { searchInputRef.current?.focus(); }, []);

  const handleSearch = useCallback(
    (value: string) => {
      setSearchQuery(value);
      if (value.trim() === "") fetchLibrary();
      else search(value);
    },
    [setSearchQuery, fetchLibrary, search],
  );

  const handlePlay = useCallback(
    (path: string) => { play(path); toggleLibrary(); },
    [play, toggleLibrary],
  );

  return (
    <>
      {/* Panel — slides in from left and *displaces* the video region
          so they share screen space side-by-side. No backdrop because
          the panel doesn't overlap the video any more. */}
      <motion.div
        className="absolute bottom-0 left-0 top-0 z-30 flex w-80 flex-col"
        style={{
          background: "var(--bg-primary)",
          borderRight: "1px solid var(--border-subtle)",
        }}
        initial={{ x: -320 }}
        animate={{ x: 0 }}
        exit={{ x: -320 }}
        transition={{ type: "spring", damping: 30, stiffness: 300 }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
          <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">
            {t.library.title}
          </h2>
          <div className="flex items-center gap-1">
            {entries.length > 0 && (
              <button
                className={`rounded-lg px-2 py-1 text-[10px] font-medium transition-all duration-150 active:scale-95 ${
                  confirmClear
                    ? "bg-red-500/20 text-red-300 border border-red-500/40"
                    : "text-white/35 hover:bg-white/6 hover:text-white/60"
                }`}
                onClick={handleClear}
                title={confirmClear ? "Click again to confirm" : t.library.clearAll}
              >
                {confirmClear ? "Confirm?" : t.common.clear}
              </button>
            )}
            <button
              className="rounded-lg px-2.5 py-1 text-[11px] font-semibold text-white transition-all duration-150 hover:opacity-80 disabled:opacity-40 active:scale-95"
              style={{ background: "linear-gradient(135deg, #7C3AED, #DB2777)" }}
              onClick={handleScanFolder}
              disabled={isScanning}
            >
              {isScanning ? t.common.loading : t.library.scan}
            </button>
            <button
              className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={toggleLibrary}
              title="Close (L)"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>
        </div>

        {/* Search */}
        <div className="px-4 py-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
          <div className="flex items-center gap-2 rounded-lg bg-white/4 px-3 py-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-white/20 flex-shrink-0" strokeLinecap="round"><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /></svg>
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              placeholder={t.library.search}
              className="w-full bg-transparent text-[12px] text-white/70 placeholder-white/20 outline-none"
            />
          </div>
        </div>

        {/* Entries */}
        <div className="flex-1 overflow-y-auto px-2 py-1.5">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            </div>
          )}

          {!isLoading && entries.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-16 text-center">
              <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" className="text-white/10" strokeLinecap="round">
                <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
                <line x1="7" y1="2" x2="7" y2="22" />
                <line x1="17" y1="2" x2="17" y2="22" />
                <line x1="2" y1="12" x2="22" y2="12" />
              </svg>
              <p className="text-[12px] text-white/25">{t.library.empty}</p>
              <button
                className="rounded-lg px-4 py-2 text-[11px] font-semibold text-white transition-all hover:opacity-80 active:scale-95 disabled:opacity-40"
                style={{ background: "linear-gradient(135deg, #7C3AED, #DB2777)" }}
                onClick={handleScanFolder}
                disabled={isScanning}
              >
                {isScanning ? t.common.loading : t.library.scan}
              </button>
            </div>
          )}

          {!isLoading && entries.map((entry) => (
            <LibraryEntry key={entry.id} entry={entry} onPlay={handlePlay} />
          ))}
        </div>

        {/* Footer */}
        {!isLoading && entries.length > 0 && (
          <div className="px-4 py-2" style={{ borderTop: "1px solid var(--border-subtle)" }}>
            <p className="text-[10px] text-white/15 font-medium">
              {entries.length} item{entries.length !== 1 ? "s" : ""}
            </p>
          </div>
        )}
      </motion.div>
    </>
  );
}

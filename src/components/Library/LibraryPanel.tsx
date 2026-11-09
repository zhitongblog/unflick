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

interface TranscriptHit {
  media_id: number;
  title: string;
  path: string;
  cue: {
    start: number;
    end: number;
    text: string;
  };
}

export default function LibraryPanel() {
  const { entries, searchQuery, isLoading, error, clearError, setSearchQuery, toggleLibrary, fetchLibrary, search, scanDirectory, clearLibrary } =
    useLibraryStore();
  const play = usePlayerStore((s) => s.play);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const hasFetchedRef = useRef(false);
  const [isScanning, setIsScanning] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [searchMode, setSearchMode] = useState<"titles" | "transcript">("titles");
  const [transcriptHits, setTranscriptHits] = useState<TranscriptHit[]>([]);
  const [isSearchingTranscript, setIsSearchingTranscript] = useState(false);
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

  const runTranscriptSearch = useCallback(async (q: string) => {
    const trimmed = q.trim();
    if (!trimmed) {
      setTranscriptHits([]);
      return;
    }
    setIsSearchingTranscript(true);
    try {
      const hits = await invoke<TranscriptHit[]>("library_transcript_search", {
        query: trimmed,
        limit: 40,
      });
      setTranscriptHits(hits);
    } catch {
      setTranscriptHits([]);
    } finally {
      setIsSearchingTranscript(false);
    }
  }, []);

  const handleSearch = useCallback(
    (value: string) => {
      setSearchQuery(value);
      if (searchMode === "transcript") {
        void runTranscriptSearch(value);
      } else if (value.trim() === "") {
        fetchLibrary();
      } else {
        search(value);
      }
    },
    [setSearchQuery, searchMode, runTranscriptSearch, fetchLibrary, search],
  );

  const handleModeChange = useCallback(
    (mode: "titles" | "transcript") => {
      setSearchMode(mode);
      if (mode === "transcript") {
        void runTranscriptSearch(searchQuery);
      } else if (searchQuery.trim() === "") {
        fetchLibrary();
      } else {
        search(searchQuery);
      }
    },
    [searchQuery, runTranscriptSearch, fetchLibrary, search],
  );

  const handlePlay = useCallback(
    (path: string, seek?: number) => {
      void play(path, null, seek ?? null);
      toggleLibrary();
    },
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
                title={confirmClear ? t.library.confirmHint : t.library.clearAll}
              >
                {confirmClear ? t.library.confirm : t.common.clear}
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
              title={t.library.close}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>
        </div>

        {/* Search */}
        <div className="space-y-2 px-4 py-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
          <div className="flex items-center gap-2 rounded-lg bg-white/4 px-3 py-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-white/20 flex-shrink-0" strokeLinecap="round"><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /></svg>
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              placeholder={searchMode === "transcript" ? t.library.searchTranscript : t.library.search}
              className="w-full bg-transparent text-[12px] text-white/70 placeholder-white/20 outline-none"
            />
          </div>
          <div className="flex gap-1">
            {(["titles", "transcript"] as const).map((m) => {
              const active = searchMode === m;
              return (
                <button
                  key={m}
                  onClick={() => handleModeChange(m)}
                  className={`flex-1 rounded-md px-2 py-1 text-[10px] font-medium transition-colors ${
                    active
                      ? "bg-brand-purple/20 text-brand-purple"
                      : "bg-white/4 text-white/35 hover:text-white/60"
                  }`}
                >
                  {m === "titles" ? t.library.modeTitles : t.library.modeTranscript}
                </button>
              );
            })}
          </div>
        </div>

        {/* Entries */}
        <div className="flex-1 overflow-y-auto px-2 py-1.5">
          {(isLoading || isSearchingTranscript) && (
            <div className="flex items-center justify-center py-12">
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
            </div>
          )}

          {searchMode === "transcript" && !isSearchingTranscript && (
            <>
              {searchQuery.trim() === "" && (
                <p className="px-4 py-12 text-center text-[11px] leading-relaxed text-white/25">
                  {t.library.transcriptHint}
                </p>
              )}
              {searchQuery.trim() !== "" && transcriptHits.length === 0 && (
                <p className="px-4 py-12 text-center text-[11px] leading-relaxed text-white/25">
                  {t.library.noTranscriptMatch.replace("{query}", searchQuery.trim())}
                </p>
              )}
              {transcriptHits.map((hit, idx) => (
                <button
                  key={`${hit.media_id}-${hit.cue.start}-${idx}`}
                  onClick={() => handlePlay(hit.path, hit.cue.start)}
                  className="group flex w-full flex-col gap-1 rounded-xl px-3 py-2 text-left transition-all duration-150 hover:bg-white/5 active:scale-[0.98]"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-[11px] font-medium text-white/60 group-hover:text-white/85">
                      {hit.title}
                    </span>
                    <span className="flex-shrink-0 rounded bg-brand-purple/15 px-1.5 py-0.5 font-mono text-[10px] text-brand-purple">
                      {formatDuration(hit.cue.start)}
                    </span>
                  </div>
                  <p className="line-clamp-2 text-[11px] leading-snug text-white/40 group-hover:text-white/65">
                    {hit.cue.text}
                  </p>
                </button>
              ))}
            </>
          )}

          {/* A failed scan used to land on the empty state below, which
              claims we looked and found nothing. Say what happened instead,
              and keep the backend's own words for a bug report. */}
          {searchMode === "titles" && error && (
            <div className="mx-1 mb-2 rounded-lg border border-red-500/20 bg-red-500/6 px-3 py-2">
              <div className="flex items-start gap-2">
                <p className="min-w-0 flex-1 text-[11px] font-medium text-red-200/90">
                  {t.library.errorTitle}
                </p>
                <button
                  onClick={clearError}
                  aria-label={t.openError.dismiss}
                  className="flex-shrink-0 rounded px-1 text-[11px] leading-none text-white/25 transition-colors hover:text-white/60"
                >
                  ✕
                </button>
              </div>
              <details className="mt-1 group">
                <summary className="cursor-pointer select-none list-none text-[10px] text-white/25 transition-colors hover:text-white/50">
                  {t.openError.details}
                </summary>
                <p className="mt-1 select-text break-words font-mono text-[10px] leading-relaxed text-white/40">
                  {error}
                </p>
              </details>
            </div>
          )}

          {/* Searching for something the library does not have is not the
              same as having an empty library. Until now both rendered "your
              library is empty — scan a folder", which told someone with 400
              films that they had none. */}
          {searchMode === "titles" && !isLoading && !error && entries.length === 0 && searchQuery.trim() !== "" && (
            <div className="flex flex-col items-center gap-3 py-16 text-center">
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" className="text-white/10" strokeLinecap="round">
                <circle cx="11" cy="11" r="8" />
                <line x1="21" y1="21" x2="16.65" y2="16.65" />
              </svg>
              <p className="px-4 text-[12px] leading-relaxed text-white/25">
                {t.library.noMatch.replace("{query}", searchQuery.trim())}
              </p>
              <button
                className="rounded-lg bg-white/5 px-4 py-2 text-[11px] font-medium text-white/50 transition-colors hover:bg-white/8 hover:text-white/80"
                onClick={() => handleSearch("")}
              >
                {t.library.clearSearch}
              </button>
              <button
                className="text-[10.5px] text-white/25 underline decoration-white/15 underline-offset-2 transition-colors hover:text-white/50 disabled:opacity-40"
                onClick={handleScanFolder}
                disabled={isScanning}
              >
                {isScanning ? t.common.loading : t.library.scan}
              </button>
            </div>
          )}

          {searchMode === "titles" && !isLoading && !error && entries.length === 0 && searchQuery.trim() === "" && (
            <div className="flex flex-col items-center gap-3 py-16 text-center">
              <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" className="text-white/10" strokeLinecap="round">
                <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
                <line x1="7" y1="2" x2="7" y2="22" />
                <line x1="17" y1="2" x2="17" y2="22" />
                <line x1="2" y1="12" x2="22" y2="12" />
              </svg>
              <p className="px-4 text-[12px] text-white/25">{t.library.empty}</p>
              <p className="px-5 text-[10.5px] leading-relaxed text-white/20">
                {t.library.emptyHint}
              </p>
              <button
                className="rounded-lg px-4 py-2 text-[11px] font-semibold text-white transition-all hover:opacity-80 active:scale-95 disabled:opacity-40"
                style={{ background: "linear-gradient(135deg, #7C3AED, #DB2777)" }}
                onClick={handleScanFolder}
                disabled={isScanning}
              >
                {isScanning ? t.common.loading : t.library.scan}
              </button>
              {/* The same thing from a terminal. Not translated: a command
                  is not a sentence, and a translation can only break it. */}
              <code className="select-text font-mono text-[10px] text-white/15">
                unflick library scan &lt;folder&gt;
              </code>
            </div>
          )}

          {searchMode === "titles" && !isLoading && entries.map((entry) => (
            <LibraryEntry key={entry.id} entry={entry} onPlay={handlePlay} />
          ))}
        </div>

        {/* Footer */}
        {!isLoading && entries.length > 0 && (
          <div className="px-4 py-2" style={{ borderTop: "1px solid var(--border-subtle)" }}>
            <p className="text-[10px] text-white/15 font-medium">
              {(entries.length === 1 ? t.library.itemsOne : t.library.items).replace(
                "{count}",
                String(entries.length),
              )}
            </p>
          </div>
        )}
      </motion.div>
    </>
  );
}

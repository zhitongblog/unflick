import { useState, useEffect, useRef, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";

/** One flattened search hit, matching `core::opensubtitles::SubtitleResult`. */
type Result = {
  file_id: number;
  language: string;
  release: string;
  file_name: string;
  downloads: number;
  hearing_impaired: boolean;
  from_trusted: boolean;
  moviehash_match: boolean;
  uploader: string;
  url: string;
};

type Outcome = {
  results: Result[];
  query: string | null;
  moviehash: string | null;
  moviehash_matches: number;
  languages: string[];
  file: string | null;
  hash_error: string | null;
};

type Config = { configured: boolean; languages: string[] };

const CONSUMERS_URL = "https://www.opensubtitles.com/consumers";

function toast(kind: "success" | "error", message: string) {
  window.dispatchEvent(
    new CustomEvent("unflick:toast", { detail: { kind, message } }),
  );
}

export default function OnlineSubtitles({ onClose }: { onClose: () => void }) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const file = usePlayerStore((s) => s.file);
  const t = useStrings();

  const [config, setConfig] = useState<Config | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [savingKey, setSavingKey] = useState(false);

  const [query, setQuery] = useState("");
  const [languages, setLanguages] = useState("");
  const [searching, setSearching] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Which row is mid-download. Only one at a time: each download spends a
  // unit of the user's daily quota, so letting them fire several by
  // double-clicking would quietly burn it.
  const [downloading, setDownloading] = useState<number | null>(null);

  useEffect(() => {
    invoke<Config>("opensubtitles_configured")
      .then((c) => {
        setConfig(c);
        setLanguages(c.languages.join(","));
      })
      .catch(() => setConfig({ configured: false, languages: ["en"] }));
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const runSearch = useCallback(
    async (text?: string) => {
      setSearching(true);
      setError(null);
      try {
        const res = await invoke<Outcome>("subtitle_search_online", {
          query: (text ?? query).trim() || null,
          languages: languages.trim() || null,
        });
        setOutcome(res);
        // The backend derives a query from the filename when we send none;
        // showing it back is the difference between "no results" and
        // "no results *for this*", which is usually the fixable part.
        if (res.query && !query) setQuery(res.query);
      } catch (e) {
        setError(String(e));
        setOutcome(null);
      } finally {
        setSearching(false);
      }
    },
    [query, languages],
  );

  // Search on open once we know a key exists and something is playing.
  // Nothing is spent by searching, so doing it unprompted is free and
  // saves the common case a click.
  const configured = config?.configured ?? false;
  const [autoRan, setAutoRan] = useState(false);
  useEffect(() => {
    if (configured && file && !autoRan) {
      setAutoRan(true);
      runSearch();
    }
  }, [configured, file, autoRan, runSearch]);

  useEffect(() => {
    if (configured) setTimeout(() => inputRef.current?.focus(), 50);
  }, [configured]);

  const saveKey = async () => {
    const key = apiKey.trim();
    if (!key) return;
    setSavingKey(true);
    try {
      await invoke("settings_set_key", { key: "opensubtitles_api_key", value: key });
      const c = await invoke<Config>("opensubtitles_configured");
      setConfig(c);
      setLanguages(c.languages.join(","));
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingKey(false);
    }
  };

  const download = async (r: Result) => {
    setDownloading(r.file_id);
    setError(null);
    try {
      const res = await invoke<{ file_name: string; remaining: number; loaded: boolean }>(
        "subtitle_download_online",
        { fileId: r.file_id, language: r.language },
      );
      await usePlayerStore.getState().refreshSubtitles();
      toast(
        "success",
        res.loaded
          ? `${res.file_name} — ${res.remaining} downloads left today`
          : `Saved ${res.file_name}, but it could not be loaded`,
      );
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
    }
  };

  const results = outcome?.results ?? [];

  return (
    <AnimatePresence>
      <motion.div
        ref={overlayRef}
        className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.15 }}
        onClick={(e) => {
          if (e.target === overlayRef.current) onClose();
        }}
        onContextMenu={(e) => {
          if (e.target === overlayRef.current) e.preventDefault();
        }}
      >
        <motion.div
          className="gradient-border flex max-h-[88vh] w-[560px] flex-col rounded-2xl p-5 shadow-2xl"
          style={{ background: "var(--bg-secondary, #111827)" }}
          initial={{ scale: 0.92, opacity: 0, y: 12 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.92, opacity: 0, y: 12 }}
          transition={{ duration: 0.2, ease: "easeOut" }}
        >
          <div className="mb-4 flex items-center justify-between">
            <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">
              {t.onlineSubtitles.title}
            </h2>
            <button
              className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={onClose}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>

          {config === null && (
            <p className="py-6 text-center text-[11px] text-white/30">{t.common.loading}</p>
          )}

          {/* Setup. Asking for the key here rather than sending the user off
              to Settings keeps them in the flow they started; there is
              nothing else on this screen to do until it exists. */}
          {config && !config.configured && (
            <div className="space-y-3">
              <p className="text-[11px] leading-relaxed text-white/50">
                {t.onlineSubtitles.needKey}
              </p>
              <a
                href={CONSUMERS_URL}
                target="_blank"
                rel="noreferrer"
                className="block text-[11px] text-brand-purple underline decoration-brand-purple/40 underline-offset-2"
              >
                {t.onlineSubtitles.getKey}
              </a>
              <div className="flex gap-2">
                <input
                  type="password"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") saveKey();
                  }}
                  placeholder={t.onlineSubtitles.keyPlaceholder}
                  className="flex-1 rounded-lg bg-white/5 px-3 py-2 text-[11px] text-white/80 outline-none ring-1 ring-white/8 placeholder:text-white/20 focus:ring-brand-purple/50"
                />
                <button
                  disabled={!apiKey.trim() || savingKey}
                  onClick={saveKey}
                  className="rounded-lg bg-brand-purple px-3 py-2 text-[11px] font-medium text-white transition-opacity disabled:opacity-30"
                >
                  {savingKey ? t.common.saving : t.common.save}
                </button>
              </div>
            </div>
          )}

          {config?.configured && (
            <>
              <div className="mb-2 flex gap-2">
                <input
                  ref={inputRef}
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") runSearch();
                  }}
                  placeholder={file ? t.onlineSubtitles.queryPlaceholder : t.onlineSubtitles.queryPlaceholderIdle}
                  className="flex-1 rounded-lg bg-white/5 px-3 py-2 text-[11px] text-white/80 outline-none ring-1 ring-white/8 placeholder:text-white/20 focus:ring-brand-purple/50"
                />
                <input
                  type="text"
                  value={languages}
                  onChange={(e) => setLanguages(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") runSearch();
                  }}
                  title={t.onlineSubtitles.languagesHint}
                  className="w-24 rounded-lg bg-white/5 px-3 py-2 text-[11px] tabular-nums text-white/80 outline-none ring-1 ring-white/8 focus:ring-brand-purple/50"
                />
                <button
                  disabled={searching}
                  onClick={() => runSearch()}
                  className="rounded-lg bg-brand-purple px-3 py-2 text-[11px] font-medium text-white transition-opacity disabled:opacity-30"
                >
                  {searching ? "…" : t.common.search}
                </button>
              </div>

              {/* An exact-file match means the timing is already right. It is
                  the single most useful thing on this screen, so it gets said
                  in words rather than only as a per-row badge. */}
              {outcome && outcome.moviehash_matches > 0 && (
                <p className="mb-2 text-[10px] text-emerald-400/80">
                  {t.onlineSubtitles.hashMatches.replace("{count}", String(outcome.moviehash_matches))}
                </p>
              )}
              {outcome && outcome.moviehash && outcome.moviehash_matches === 0 && (
                <p className="mb-2 text-[10px] text-white/30">
                  {t.onlineSubtitles.noHashMatch}
                </p>
              )}

              {error && (
                <p className="mb-2 rounded-lg bg-red-500/10 px-3 py-2 text-[10px] leading-relaxed text-red-300/90">
                  {error}
                </p>
              )}

              <div className="-mx-1 flex-1 overflow-y-auto px-1">
                {!searching && outcome && results.length === 0 && (
                  <p className="py-6 text-center text-[11px] text-white/25">
                    {t.onlineSubtitles.noResults.replace("{query}", outcome.query ?? query)}
                  </p>
                )}

                {results.map((r) => (
                  <div
                    key={r.file_id}
                    className="mb-1 flex items-center gap-3 rounded-lg px-2 py-2 transition-colors hover:bg-white/5"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="rounded bg-white/8 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-white/60">
                          {r.language}
                        </span>
                        {r.moviehash_match && (
                          <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 text-[9px] font-medium text-emerald-400">
                            {t.onlineSubtitles.exactMatch}
                          </span>
                        )}
                        {r.hearing_impaired && (
                          <span className="rounded bg-white/8 px-1.5 py-0.5 text-[9px] text-white/40">
                            {t.onlineSubtitles.sdh}
                          </span>
                        )}
                        <span className="truncate text-[11px] text-white/70" title={r.file_name}>
                          {r.release || r.file_name}
                        </span>
                      </div>
                      <p className="mt-0.5 truncate text-[10px] text-white/25">
                        {t.onlineSubtitles.downloads.replace("{count}", r.downloads.toLocaleString())}
                        {r.uploader ? ` · ${r.uploader}` : ""}
                        {r.from_trusted ? ` · ${t.onlineSubtitles.trusted}` : ""}
                      </p>
                    </div>
                    <button
                      disabled={downloading !== null}
                      onClick={() => download(r)}
                      className="shrink-0 rounded-lg bg-white/8 px-2.5 py-1.5 text-[10px] font-medium text-white/80 transition-colors hover:bg-white/14 disabled:opacity-30"
                    >
                      {downloading === r.file_id ? "…" : t.onlineSubtitles.use}
                    </button>
                  </div>
                ))}
              </div>
            </>
          )}
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

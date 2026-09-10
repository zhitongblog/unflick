import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useSettingsStore, type PreferredQuality } from "../stores/settingsStore";
import { useStrings } from "../i18n/utils";
import { STREAMING_SITES } from "../lib/streamingSites";

const STORAGE_KEY = "unflick_recent_urls";
const MAX_RECENT = 5;

function getRecentUrls(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch { return []; }
}

function addRecentUrl(url: string) {
  const existing = getRecentUrls().filter((u) => u !== url);
  const updated = [url, ...existing].slice(0, MAX_RECENT);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}

// Pull the human-readable list from the canonical regex table so we never
// drift between "what we recognise" and "what we tell users we recognise".
// We append "+ 1500 more via yt-dlp" because yt-dlp's coverage is much
// broader than our pretty-name list. Site names are proper nouns, so this
// one line stays out of i18n while everything around it went in.
const SUPPORTED_EXTRACT =
  STREAMING_SITES.map((s) => s.name).join(", ") + " + 1500 more (via yt-dlp)";

export default function UrlDialog({ onClose }: { onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [recentUrls, setRecentUrls] = useState<string[]>([]);
  const [ytDlpAvailable, setYtDlpAvailable] = useState<boolean | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const play = usePlayerStore((s) => s.play);
  const extracting = usePlayerStore((s) => s.extracting);
  const extractError = usePlayerStore((s) => s.extractError);
  const openError = usePlayerStore((s) => s.openError);
  const proxy = useSettingsStore((s) => s.proxy);
  const savedQuality = useSettingsStore((s) => s.preferredQuality);
  const t = useStrings();

  // Per-dialog quality override. Defaults to the saved setting (or "auto"
  // if none) and can be bumped down for a single play (e.g. on a slow
  // connection) without touching settings.
  const [quality, setQuality] = useState<PreferredQuality>(savedQuality ?? "auto");
  // Keep in sync if the user opens the dialog after changing the saved
  // setting elsewhere.
  useEffect(() => {
    setQuality(savedQuality ?? "auto");
  }, [savedQuality]);

  useEffect(() => {
    setRecentUrls(getRecentUrls());
    setTimeout(() => inputRef.current?.focus(), 50);
    invoke<{ available: boolean }>("check_yt_dlp")
      .then((r) => setYtDlpAvailable(r.available))
      .catch(() => setYtDlpAvailable(false));
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  /**
   * Play, then decide whether to close — after `play` has settled, not
   * before.
   *
   * The old version fired `play` and left the closing to an effect that ran
   * on `[played, extracting, extractError, openError]`. For anything that
   * does not go through yt-dlp — a share path, a direct .mp4, an smb:// URL
   * — `extracting` is never set, so the first render after the click already
   * satisfied the condition and closed the dialog before the failure came
   * back. The inline error below was unreachable in exactly the case its own
   * comment cites.
   */
  const handlePlay = async () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    addRecentUrl(trimmed);
    // Pass the dropdown's current value as a one-off override. "auto" is
    // forwarded as-is and resolves to "use yt-dlp default" downstream.
    await play(trimmed, quality);
    const settled = usePlayerStore.getState();
    if (!settled.openError && !settled.extractError) onClose();
  };

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === overlayRef.current) onClose();
  };

  return (
    <AnimatePresence>
      <motion.div
        ref={overlayRef}
        className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.15 }}
        onClick={handleOverlayClick}
        onContextMenu={(e) => { if (e.target === overlayRef.current) e.preventDefault(); }}
      >
        <motion.div
          className="gradient-border w-[440px] max-h-[88vh] overflow-y-auto rounded-2xl p-5 shadow-2xl"
          style={{ background: "var(--bg-secondary, #111827)" }}
          initial={{ scale: 0.92, opacity: 0, y: 12 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.92, opacity: 0, y: 12 }}
          transition={{ duration: 0.2, ease: "easeOut" }}
        >
          <div className="mb-4 flex items-center justify-between">
            <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">{t.urlDialog.title}</h2>
            <button className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50" onClick={onClose}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>

          <div className="mb-3">
            <input
              ref={inputRef}
              type="text"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") void handlePlay(); }}
              placeholder={t.urlDialog.inputPlaceholder}
              disabled={extracting !== null}
              className="w-full rounded-lg border border-white/6 bg-white/4 px-3 py-2.5 text-[12px] text-white/70 outline-none transition-colors placeholder:text-white/15 focus:border-brand-purple/40 disabled:opacity-50"
            />
          </div>

          {/* Per-call quality override. Defaults to the saved setting; the
              user can drop it for a single play (e.g. on a slow link). */}
          <div className="mb-3 flex items-center gap-2">
            <label className="flex-shrink-0 text-[10px] font-semibold uppercase tracking-widest text-white/30">
              {t.urlDialog.qualityLabel}
            </label>
            <select
              value={quality}
              onChange={(e) => setQuality(e.target.value as PreferredQuality)}
              disabled={extracting !== null}
              className="flex-1 rounded-lg border border-white/10 bg-[#1c1c26] px-2.5 py-1.5 text-[11px] text-white outline-none focus:border-brand-purple/40 disabled:opacity-50"
            >
              <option value="auto" style={{ background: "#1c1c26", color: "#ffffff" }}>{t.settings.streaming.qualityAuto}</option>
              <option value="2160p" style={{ background: "#1c1c26", color: "#ffffff" }}>2160p</option>
              <option value="1440p" style={{ background: "#1c1c26", color: "#ffffff" }}>1440p</option>
              <option value="1080p" style={{ background: "#1c1c26", color: "#ffffff" }}>1080p</option>
              <option value="720p" style={{ background: "#1c1c26", color: "#ffffff" }}>720p</option>
              <option value="480p" style={{ background: "#1c1c26", color: "#ffffff" }}>480p</option>
              <option value="audio_only" style={{ background: "#1c1c26", color: "#ffffff" }}>{t.settings.streaming.qualityAudioOnly}</option>
            </select>
          </div>

          {/* Status during extraction */}
          {extracting && (
            <div className="mb-3 flex items-center gap-2 rounded-lg border border-brand-purple/20 bg-brand-purple/5 px-3 py-2 text-[11px] text-brand-purple">
              <svg className="animate-spin" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56" /></svg>
              <span className="flex-1">{t.urlDialog.resolvingSite.replace("{site}", extracting.site)}</span>
              <button
                className="rounded-md border border-white/10 bg-white/5 px-2 py-0.5 text-[10.5px] font-medium text-white/60 transition-colors hover:border-white/20 hover:bg-white/10 hover:text-white/85"
                onClick={async () => {
                  try { await invoke("cancel_url_extraction"); } catch { /* ignore */ }
                  onClose();
                }}
              >
                {t.urlDialog.cancel}
              </button>
            </div>
          )}

          {/* Extraction failed, or the source itself would not open. The
              second one is what a share URL or a dead host lands on, and it
              carries the advice for getting to the file another way. */}
          {(extractError || openError) && (
            <div className="mb-3 rounded-lg border border-red-500/20 bg-red-500/5 px-3 py-2 text-[11px] text-red-300/90">
              {extractError || openError}
            </div>
          )}

          {/* Help text — what's supported */}
          <details className="mb-4 group">
            <summary className="cursor-pointer text-[11px] font-medium text-white/40 transition-colors hover:text-white/60 select-none list-none flex items-center gap-1.5">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="transition-transform group-open:rotate-90">
                <polyline points="9 18 15 12 9 6" />
              </svg>
              {t.urlDialog.whatSupported}
            </summary>
            <div className="mt-2 space-y-2 rounded-lg border border-white/6 bg-white/3 p-3 text-[10.5px] leading-relaxed">
              <div>
                <span className="text-emerald-300/90">✓ {t.urlDialog.directTitle}</span>
                <p className="text-white/30 mt-0.5">{t.urlDialog.directBody}</p>
              </div>
              <div>
                <span className="text-emerald-300/90">✓ {t.urlDialog.streamTitle}</span>
                <p className="text-white/30 mt-0.5">{t.urlDialog.streamBody}</p>
              </div>
              <div>
                <span className={ytDlpAvailable ? "text-emerald-300/90" : "text-amber-300/90"}>
                  {ytDlpAvailable ? "✓" : "⚠"} {t.urlDialog.sitesTitle}
                </span>
                <p className="text-white/30 mt-0.5">{SUPPORTED_EXTRACT}</p>
                {ytDlpAvailable === false && (
                  <p className="mt-1 text-amber-300/70">
                    {t.urlDialog.ytDlpMissingPrefix}{" "}
                    <a href="https://github.com/yt-dlp/yt-dlp/releases" target="_blank" rel="noreferrer" className="underline hover:text-amber-200">
                      github.com/yt-dlp/yt-dlp
                    </a>
                    {" "}{t.urlDialog.ytDlpMissingSuffix}
                  </p>
                )}
              </div>
              <div>
                <span className="text-emerald-300/90">✓ {t.urlDialog.networkTitle}</span>
                <p className="text-white/30 mt-0.5">{t.urlDialog.networkBody}</p>
              </div>
              <div>
                <span className="text-red-400/80">✗ {t.urlDialog.drmTitle}</span>
                <p className="text-white/30 mt-0.5">{t.urlDialog.drmBody}</p>
              </div>
              {proxy && (
                <div className="mt-3 pt-2 border-t border-white/6 text-white/35">
                  <span className="text-brand-purple/80">{t.urlDialog.proxyActive}</span> {proxy}
                </div>
              )}
            </div>
          </details>

          {recentUrls.length > 0 && (
            <div className="mb-4">
              <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-white/20">{t.urlDialog.recent}</p>
              <div className="flex flex-col gap-0.5">
                {recentUrls.map((recentUrl) => (
                  <button
                    key={recentUrl}
                    className="truncate rounded-lg px-2.5 py-1.5 text-left text-[11px] text-white/30 transition-colors hover:bg-white/5 hover:text-white/60"
                    onClick={() => { setUrl(recentUrl); setTimeout(() => inputRef.current?.focus(), 0); }}
                    title={recentUrl}
                  >
                    {recentUrl}
                  </button>
                ))}
              </div>
            </div>
          )}

          <button
            className="w-full rounded-xl py-2.5 text-[12px] font-semibold text-white transition-all hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
            style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
            onClick={() => void handlePlay()}
            disabled={!url.trim() || extracting !== null}
          >
            {extracting ? t.urlDialog.resolving : t.urlDialog.play}
          </button>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

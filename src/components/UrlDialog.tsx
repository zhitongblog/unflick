import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useSettingsStore } from "../stores/settingsStore";

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

const SUPPORTED_DIRECT = "MP4, WebM, MKV, M4V, MOV — paste any HTTP/HTTPS link";
const SUPPORTED_STREAMING = "HLS (.m3u8), MPEG-DASH (.mpd) live and VOD streams";
const SUPPORTED_EXTRACT = "YouTube, Bilibili, Twitch, Vimeo, Douyin, TikTok, Weibo (via yt-dlp)";
const UNSUPPORTED = "Netflix, Disney+, iQIYI VIP — DRM-protected, cannot be played";

export default function UrlDialog({ onClose }: { onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [recentUrls, setRecentUrls] = useState<string[]>([]);
  const [ytDlpAvailable, setYtDlpAvailable] = useState<boolean | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const play = usePlayerStore((s) => s.play);
  const extracting = usePlayerStore((s) => s.extracting);
  const extractError = usePlayerStore((s) => s.extractError);
  const proxy = useSettingsStore((s) => s.proxy);

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

  const [played, setPlayed] = useState(false);

  const handlePlay = () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    addRecentUrl(trimmed);
    setPlayed(true);
    play(trimmed);
    // Don't close immediately — extraction may take a moment and we want to
    // surface any error inline. Close once extraction completes successfully.
  };

  // Close only after the user clicked Play AND extraction completed without
  // error (or there was no extraction needed). This prevents the dialog from
  // auto-closing the moment it opens while a video is already playing.
  useEffect(() => {
    if (played && extracting === null && !extractError) {
      onClose();
    }
  }, [played, extracting, extractError, onClose]);

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
            <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">Open URL</h2>
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
              onKeyDown={(e) => { if (e.key === "Enter") handlePlay(); }}
              placeholder="https://...  or  https://www.youtube.com/watch?v=..."
              disabled={extracting !== null}
              className="w-full rounded-lg border border-white/6 bg-white/4 px-3 py-2.5 text-[12px] text-white/70 outline-none transition-colors placeholder:text-white/15 focus:border-brand-purple/40 disabled:opacity-50"
            />
          </div>

          {/* Status during extraction */}
          {extracting && (
            <div className="mb-3 flex items-center gap-2 rounded-lg border border-brand-purple/20 bg-brand-purple/5 px-3 py-2 text-[11px] text-brand-purple">
              <svg className="animate-spin" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56" /></svg>
              <span>Resolving {extracting.site} link…</span>
            </div>
          )}

          {/* Error from extraction */}
          {extractError && (
            <div className="mb-3 rounded-lg border border-red-500/20 bg-red-500/5 px-3 py-2 text-[11px] text-red-300/90">
              {extractError}
            </div>
          )}

          {/* Help text — what's supported */}
          <details className="mb-4 group">
            <summary className="cursor-pointer text-[11px] font-medium text-white/40 transition-colors hover:text-white/60 select-none list-none flex items-center gap-1.5">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="transition-transform group-open:rotate-90">
                <polyline points="9 18 15 12 9 6" />
              </svg>
              What URLs are supported?
            </summary>
            <div className="mt-2 space-y-2 rounded-lg border border-white/6 bg-white/3 p-3 text-[10.5px] leading-relaxed">
              <div>
                <span className="text-emerald-300/90">✓ Direct video files</span>
                <p className="text-white/30 mt-0.5">{SUPPORTED_DIRECT}</p>
              </div>
              <div>
                <span className="text-emerald-300/90">✓ Live/VOD streams</span>
                <p className="text-white/30 mt-0.5">{SUPPORTED_STREAMING}</p>
              </div>
              <div>
                <span className={ytDlpAvailable ? "text-emerald-300/90" : "text-amber-300/90"}>
                  {ytDlpAvailable ? "✓" : "⚠"} Streaming sites
                </span>
                <p className="text-white/30 mt-0.5">{SUPPORTED_EXTRACT}</p>
                {ytDlpAvailable === false && (
                  <p className="mt-1 text-amber-300/70">
                    yt-dlp not found. Install from{" "}
                    <a href="https://github.com/yt-dlp/yt-dlp/releases" target="_blank" rel="noreferrer" className="underline hover:text-amber-200">
                      github.com/yt-dlp/yt-dlp
                    </a>
                    {" "}and place yt-dlp.exe on your PATH.
                  </p>
                )}
              </div>
              <div>
                <span className="text-red-400/80">✗ DRM-protected</span>
                <p className="text-white/30 mt-0.5">{UNSUPPORTED}</p>
              </div>
              {proxy && (
                <div className="mt-3 pt-2 border-t border-white/6 text-white/35">
                  <span className="text-brand-purple/80">Proxy active:</span> {proxy}
                </div>
              )}
            </div>
          </details>

          {recentUrls.length > 0 && (
            <div className="mb-4">
              <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-white/20">Recent</p>
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
            onClick={handlePlay}
            disabled={!url.trim() || extracting !== null}
          >
            {extracting ? "Resolving…" : "Play"}
          </button>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

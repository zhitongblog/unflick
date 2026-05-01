import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../../stores/settingsStore";
import { LOCALES, LOCALE_NAMES } from "../../i18n/config";
import { useStrings } from "../../i18n/utils";

interface SettingsPanelProps {
  onClose: () => void;
}

export default function SettingsPanel({ onClose }: SettingsPanelProps) {
  const {
    whisperMode, whisperModelPath, whisperBinaryPath, theme, proxy, locale, screenshotDir,
    setWhisperMode, setWhisperModelPath, setWhisperBinaryPath,
    setTheme, setProxy, setLocale, setScreenshotDir, saveSettings,
  } = useSettingsStore();
  const t = useStrings();

  const [draftMode, setDraftMode] = useState(whisperMode);
  const [draftModelPath, setDraftModelPath] = useState(whisperModelPath ?? "");
  const [draftBinaryPath, setDraftBinaryPath] = useState(whisperBinaryPath ?? "");
  const [draftProxy, setDraftProxy] = useState(proxy ?? "");
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved">("idle");
  const [bundled, setBundled] = useState<{ binary: string; model: string } | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [ytDlpAvailable, setYtDlpAvailable] = useState<boolean | null>(null);
  const [ytDlpVersion, setYtDlpVersion] = useState<string | null>(null);
  const [ytDlpSource, setYtDlpSource] = useState<string | null>(null);
  const [ytDlpUpdating, setYtDlpUpdating] = useState(false);
  const [ytDlpUpdateError, setYtDlpUpdateError] = useState<string | null>(null);
  const [systemProxy, setSystemProxy] = useState<string | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);

  // Check if bundled whisper is available
  useEffect(() => {
    invoke<{ bundled: boolean; whisper_binary?: string; model_path?: string }>("check_bundled_whisper")
      .then((r) => {
        if (r.bundled && r.whisper_binary && r.model_path) {
          setBundled({ binary: r.whisper_binary, model: r.model_path });
        }
      })
      .catch(() => {});
    refreshYtDlpInfo();
  }, []);

  const refreshYtDlpInfo = () => {
    invoke<{ available: boolean; version?: string; source?: string }>("yt_dlp_info")
      .then((r) => {
        setYtDlpAvailable(r.available);
        setYtDlpVersion(r.version ?? null);
        setYtDlpSource(r.source ?? null);
      })
      .catch(() => setYtDlpAvailable(false));
    invoke<{ enabled: boolean; url: string | null }>("get_system_proxy")
      .then((r) => setSystemProxy(r.url))
      .catch(() => setSystemProxy(null));
  };

  const handleUpdateYtDlp = async () => {
    setYtDlpUpdating(true);
    setYtDlpUpdateError(null);
    try {
      await invoke("update_yt_dlp", { proxy: draftProxy.trim() || null });
      refreshYtDlpInfo();
    } catch (e) {
      setYtDlpUpdateError(typeof e === "string" ? e : "Update failed");
    } finally {
      setYtDlpUpdating(false);
    }
  };

  // Whether the currently configured local paths match the bundled installation
  const usingBundled =
    bundled !== null &&
    draftBinaryPath === bundled.binary &&
    draftModelPath === bundled.model;

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === overlayRef.current) onClose();
  };

  const handleBrowse = async (setter: (v: string) => void) => {
    try {
      const result = await invoke<{ path: string | null }>("open_file_dialog");
      if (result.path) setter(result.path);
    } catch { /* ignore */ }
  };

  const handleSave = async () => {
    setSaveStatus("saving");
    setWhisperMode(draftMode);
    setWhisperModelPath(draftModelPath || null);
    setWhisperBinaryPath(draftBinaryPath || null);
    setProxy(draftProxy.trim() || null);
    await saveSettings();
    setSaveStatus("saved");
    setTimeout(() => setSaveStatus("idle"), 1500);
  };

  const inputClass = "min-w-0 flex-1 rounded-lg border border-white/6 bg-white/4 px-2.5 py-1.5 text-[11px] text-white/70 outline-none transition-colors focus:border-brand-purple/40 placeholder-white/15";
  const browseBtnClass = "flex flex-shrink-0 items-center gap-1.5 rounded-lg border border-white/6 bg-white/4 px-2.5 py-1.5 text-[11px] text-white/35 transition-colors hover:bg-white/8 hover:text-white/60";

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
          className="gradient-border rounded-2xl shadow-2xl"
          style={{
            background: "var(--bg-secondary, #111827)",
            width: "440px",
            maxHeight: "calc(100vh - 80px)",
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
          }}
          initial={{ scale: 0.92, opacity: 0, y: 12 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.92, opacity: 0, y: 12 }}
          transition={{ duration: 0.2, ease: "easeOut" }}
        >
          {/* Header */}
          <div className="flex flex-shrink-0 items-center justify-between px-5 py-4" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
            <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">{t.settings.title}</h2>
            <button
              className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={onClose}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>

          {/* Body */}
          <div
            className="settings-scroll p-5 space-y-5"
            style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}
          >
            {/* Language */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.language}
              </p>
              <select
                value={locale}
                onChange={(e) => {
                  const next = e.target.value;
                  // Cast is safe — option list comes from LOCALES.
                  setLocale(next as typeof LOCALES[number]);
                  // Persist immediately so a restart picks up the new menu language.
                  void saveSettings();
                }}
                className="w-full rounded-lg border border-white/10 bg-[#1c1c26] px-3 py-2 text-[12px] text-white outline-none focus:border-brand-purple/40"
              >
                {LOCALES.map((l) => (
                  // <option> doesn't inherit Tailwind colors in WebView2 —
                  // set bg + text explicitly so the dropdown isn't white-on-white.
                  <option key={l} value={l} style={{ background: "#1c1c26", color: "#ffffff" }}>
                    {LOCALE_NAMES[l]}
                  </option>
                ))}
              </select>
              <p className="mt-2 text-[10px] leading-relaxed text-white/30">
                {t.settings.languageHint}
              </p>
            </div>

            {/* Screenshots */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                Screenshots
              </p>
              <div className="flex items-center gap-2">
                <div className="flex-1 truncate rounded-lg border border-white/6 bg-white/4 px-3 py-2 text-[11px] text-white/70 font-mono">
                  {screenshotDir || "Ask each time (default)"}
                </div>
                <button
                  className="rounded-lg border border-white/10 bg-white/4 px-3 py-2 text-[11px] text-white/70 hover:border-white/20 hover:text-white transition"
                  onClick={async () => {
                    const result = await invoke<{ path: string | null }>("open_folder_dialog");
                    if (result.path) {
                      setScreenshotDir(result.path);
                      void saveSettings();
                    }
                  }}
                >
                  Choose…
                </button>
                {screenshotDir && (
                  <button
                    className="rounded-lg p-2 text-white/30 hover:bg-white/6 hover:text-white/70 transition"
                    onClick={() => {
                      setScreenshotDir(null);
                      void saveSettings();
                    }}
                    title="Clear (back to dialog)"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                  </button>
                )}
              </div>
              <p className="mt-2 text-[10px] leading-relaxed text-white/30">
                When set, screenshots save here automatically with the video name and timestamp. Leave empty to be asked each time.
              </p>
            </div>

            {/* AI Subtitles */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                AI Subtitles
              </p>

              <div className="mb-4 flex gap-2">
                {(["off", "local"] as const).map((mode) => {
                  const labels = { off: "Off", local: "Local Whisper" };
                  const active = draftMode === mode;
                  return (
                    <button
                      key={mode}
                      onClick={() => setDraftMode(mode)}
                      className={`flex-1 rounded-lg border px-3 py-2 text-[11px] font-medium transition-all duration-150 ${
                        active
                          ? "border-brand-purple/30 bg-brand-purple/10 text-brand-purple"
                          : "border-white/6 bg-white/4 text-white/35 hover:border-white/10 hover:text-white/50"
                      }`}
                    >
                      {labels[mode]}
                    </button>
                  );
                })}
              </div>

              {draftMode === "local" && usingBundled && !showAdvanced && (
                <div className="rounded-xl border border-emerald-500/20 bg-emerald-500/5 p-4">
                  <div className="flex items-center gap-2 text-[11px] text-emerald-300/90">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                    <span className="font-medium">Bundled Whisper ready</span>
                  </div>
                  <p className="mt-1.5 text-[10px] leading-relaxed text-white/30">
                    AI subtitle generation works out of the box. No configuration needed.
                  </p>
                  <button
                    className="mt-2 text-[10px] text-white/30 hover:text-white/50"
                    onClick={() => setShowAdvanced(true)}
                  >
                    Use a custom installation →
                  </button>
                </div>
              )}

              {draftMode === "local" && (!usingBundled || showAdvanced) && (
                <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                  {bundled && showAdvanced && (
                    <button
                      className="text-[10px] text-white/30 hover:text-white/50"
                      onClick={() => {
                        setDraftBinaryPath(bundled.binary);
                        setDraftModelPath(bundled.model);
                        setShowAdvanced(false);
                      }}
                    >
                      ← Reset to bundled installation
                    </button>
                  )}
                  <div>
                    <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      Whisper Binary
                    </label>
                    <div className="flex gap-2">
                      <input type="text" value={draftBinaryPath} onChange={(e) => setDraftBinaryPath(e.target.value)} placeholder="path/to/whisper-cli.exe" className={inputClass} />
                      <button className={browseBtnClass} onClick={() => handleBrowse(setDraftBinaryPath)}>
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" /></svg>
                        Browse
                      </button>
                    </div>
                  </div>
                  <div>
                    <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      Model File
                    </label>
                    <div className="flex gap-2">
                      <input type="text" value={draftModelPath} onChange={(e) => setDraftModelPath(e.target.value)} placeholder="path/to/ggml-base.en.bin" className={inputClass} />
                      <button className={browseBtnClass} onClick={() => handleBrowse(setDraftModelPath)}>
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" /></svg>
                        Browse
                      </button>
                    </div>
                  </div>
                  <p className="text-[10px] leading-relaxed text-white/15">
                    Download whisper.cpp from GitHub and a GGML model file.
                  </p>
                </div>
              )}

              {draftMode === "off" && (
                <p className="text-[10px] leading-relaxed text-white/15">
                  AI subtitle generation is disabled. Select Local Whisper or OpenAI API to enable.
                </p>
              )}
            </div>

            {/* Network */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                Network
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div>
                  <div className="mb-1.5 flex items-center justify-between">
                    <label className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      Use system proxy
                    </label>
                    <button
                      type="button"
                      onClick={() => setDraftProxy(draftProxy === "system" ? "" : "system")}
                      className={`relative h-5 w-9 rounded-full transition-colors ${
                        draftProxy === "system" ? "bg-brand-purple" : "bg-white/10"
                      }`}
                    >
                      <span
                        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
                          draftProxy === "system" ? "translate-x-[18px]" : "translate-x-0.5"
                        }`}
                      />
                    </button>
                  </div>
                  {draftProxy === "system" && systemProxy && (
                    <p className="text-[10px] leading-relaxed text-emerald-300/80">
                      ✓ Detected: <span className="font-mono text-white/50">{systemProxy}</span>
                    </p>
                  )}
                  {draftProxy === "system" && !systemProxy && (
                    <p className="text-[10px] leading-relaxed text-amber-300/80">
                      No system proxy is currently set. Streaming-site fetches will go direct.
                    </p>
                  )}
                  {draftProxy !== "system" && (
                    <p className="text-[10px] leading-relaxed text-white/30">
                      When on, unflick reads your Windows proxy setting (Settings → Network → Proxy)
                      automatically and uses it for YouTube/Bilibili extraction.
                    </p>
                  )}
                </div>

                <div className="border-t border-white/6 pt-3">
                  <div className="mb-1.5 flex items-center justify-between">
                    <p className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      URL Extractor (yt-dlp)
                    </p>
                    {ytDlpAvailable && (
                      <button
                        className="rounded-md border border-white/8 bg-white/4 px-2 py-0.5 text-[10px] text-white/40 transition-colors hover:bg-white/8 hover:text-white/70 disabled:opacity-50"
                        onClick={handleUpdateYtDlp}
                        disabled={ytDlpUpdating}
                      >
                        {ytDlpUpdating ? "Updating…" : "Update"}
                      </button>
                    )}
                  </div>
                  {ytDlpAvailable === null ? (
                    <p className="text-[10px] text-white/25">Checking…</p>
                  ) : ytDlpAvailable ? (
                    <div className="text-[10px] text-emerald-300/80">
                      ✓ Ready — supports YouTube, Bilibili, Twitch, Vimeo, Douyin, TikTok, Weibo
                      <p className="mt-1 text-white/30">
                        {ytDlpVersion && <>Version <span className="font-mono text-white/40">{ytDlpVersion}</span></>}
                        {ytDlpSource === "user" && <> · auto-updated</>}
                        {ytDlpSource === "bundled" && <> · bundled with unflick</>}
                        {ytDlpSource === "path" && <> · from system PATH</>}
                      </p>
                      <p className="mt-1 text-white/25">
                        Click Update to fetch the latest version (recommended every few weeks since streaming sites change formats often).
                      </p>
                    </div>
                  ) : (
                    <div className="text-[10px] text-amber-300/80">
                      ⚠ yt-dlp is not installed. Streaming-site URLs won't work.
                      <p className="mt-1 text-white/30">
                        Install from{" "}
                        <a href="https://github.com/yt-dlp/yt-dlp/releases" target="_blank" rel="noreferrer" className="text-amber-200 underline hover:text-amber-100">
                          github.com/yt-dlp/yt-dlp
                        </a>
                        {" "}— or click below to download automatically.
                      </p>
                      <button
                        className="mt-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2.5 py-1 text-[10px] text-amber-200 transition-colors hover:bg-amber-500/20 disabled:opacity-50"
                        onClick={handleUpdateYtDlp}
                        disabled={ytDlpUpdating}
                      >
                        {ytDlpUpdating ? "Downloading…" : "Download yt-dlp"}
                      </button>
                    </div>
                  )}
                  {ytDlpUpdateError && (
                    <p className="mt-1.5 text-[10px] text-red-300/80">{ytDlpUpdateError}</p>
                  )}
                </div>
              </div>
            </div>

            {/* Theme */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                Theme
              </p>
              <div className="flex gap-2">
                {([
                  { value: "dark" as const, label: "Dark", color: "#030712" },
                  { value: "midnight" as const, label: "Midnight", color: "#000000" },
                  { value: "purple" as const, label: "Purple", color: "#0a0015" },
                ]).map((t) => (
                  <button
                    key={t.value}
                    onClick={() => setTheme(t.value)}
                    className={`flex flex-1 flex-col items-center gap-1.5 rounded-lg border px-3 py-2 text-[11px] font-medium transition-all duration-150 ${
                      theme === t.value
                        ? "border-brand-purple/30 bg-brand-purple/10 text-brand-purple"
                        : "border-white/6 bg-white/4 text-white/35 hover:border-white/10 hover:text-white/50"
                    }`}
                  >
                    <span className="h-5 w-full rounded border border-white/6" style={{ backgroundColor: t.color }} />
                    {t.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Footer */}
          <div className="flex flex-shrink-0 items-center justify-end gap-3 px-5 py-4" style={{ borderTop: "1px solid var(--border-subtle)" }}>
            <button
              className="rounded-lg px-4 py-2 text-[11px] font-medium text-white/30 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={onClose}
            >
              Cancel
            </button>
            <button
              className="rounded-xl px-5 py-2 text-[11px] font-semibold text-white transition-all hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
              style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
              onClick={handleSave}
              disabled={saveStatus === "saving"}
            >
              {saveStatus === "saving" ? (
                <span className="flex items-center gap-2">
                  <span className="h-3 w-3 animate-spin rounded-full border-2 border-white border-t-transparent" />
                  Saving...
                </span>
              ) : saveStatus === "saved" ? "Saved!" : "Save"}
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

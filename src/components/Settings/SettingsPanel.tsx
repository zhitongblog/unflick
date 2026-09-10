import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import {
  useSettingsStore,
  DEFAULT_SUBTITLE_STYLE,
  type SubtitleStyle,
} from "../../stores/settingsStore";
import { LOCALES, LOCALE_NAMES } from "../../i18n/config";
import { useStrings } from "../../i18n/utils";
import { SHOW_ONBOARDING_EVENT } from "../../lib/onboardingEvent";
import Toggle, { ToggleTrack } from "../ui/Toggle";
import KeybindSettings from "./KeybindSettings";
import MouseSettings from "./MouseSettings";

interface SettingsPanelProps {
  onClose: () => void;
}

/**
 * Offers back the disk an earlier install stranded.
 *
 * Renders nothing when there is nothing to reclaim — a permanent row
 * reading "0 B to clean up" is the settings bloat this project keeps
 * saying no to. Someone who never opens a terminal has no other way to
 * learn those files exist, so this is the only surface that reaches them.
 */
function DiskCleanup() {
  const t = useStrings();
  const [report, setReport] = useState<{ total_bytes: number; items: unknown[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [freed, setFreed] = useState<number | null>(null);

  useEffect(() => {
    invoke<{ total_bytes: number; items: unknown[] }>("cleanup_scan")
      .then((r) => setReport(r.items.length > 0 ? r : null))
      .catch(() => setReport(null));
  }, []);

  if (!report && freed === null) return null;

  const size = (bytes: number) => {
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
  };

  const reclaim = async () => {
    if (!report) return;
    setBusy(true);
    try {
      await invoke("cleanup_apply");
      setFreed(report.total_bytes);
      setReport(null);
    } catch (e) {
      console.error("[settings] cleanup_apply failed", e);
      window.dispatchEvent(
        new CustomEvent("unflick:toast", { detail: { kind: "error", message: t.disk.failed } }),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
        {t.disk.section}
      </p>
      {freed !== null ? (
        <p className="rounded-lg border border-white/6 bg-white/4 px-3 py-2.5 text-[11px] text-brand-purple">
          {t.disk.done.replace("{size}", size(freed))}
        </p>
      ) : (
        report && (
          <div className="rounded-lg border border-white/6 bg-white/4 px-3 py-2.5">
            <p className="text-[11px] text-white/55">
              {t.disk.found.replace("{size}", size(report.total_bytes))}
            </p>
            <p className="mt-0.5 text-[10px] leading-snug text-white/30">{t.disk.hint}</p>
            <button
              onClick={reclaim}
              disabled={busy}
              className="mt-2 rounded-lg bg-brand-purple/15 px-3 py-1.5 text-[11px] font-medium text-brand-purple transition-colors hover:bg-brand-purple/25 disabled:opacity-40"
            >
              {busy ? t.disk.working : t.disk.reclaim.replace("{size}", size(report.total_bytes))}
            </button>
          </div>
        )
      )}
    </div>
  );
}

export default function SettingsPanel({ onClose }: SettingsPanelProps) {
  const {
    whisperMode, whisperModelPath, whisperBinaryPath, theme, proxy, locale, screenshotDir,
    alwaysOnTop, musicModeAuto, preferredQuality, cookiesBrowser,
    sponsorblockEnabled, sponsorblockCategories, autoDownloadSubtitles, subtitleLanguages,
    setWhisperMode, setWhisperModelPath, setWhisperBinaryPath,
    setTheme, setProxy, setLocale, setScreenshotDir, setAlwaysOnTop, setMusicModeAuto,
    setPreferredQuality, setCookiesBrowser,
    setSponsorblockEnabled, setSponsorblockCategories,
    setAutoDownloadSubtitles, setSubtitleLanguages,
    subtitleStyle, setSubtitleStyle,
    saveSettings,
  } = useSettingsStore();
  const t = useStrings();

  /**
   * Apply one subtitle style property everywhere at once: to mpv (so the
   * change is visible while the panel is still open), to the store, and to
   * disk. Applying live is the whole point — you can't judge subtitle size
   * from a number.
   */
  const applySubtitleStyle = (name: keyof SubtitleStyle, value: number | string | boolean) => {
    setSubtitleStyle({ [name]: value } as Partial<SubtitleStyle>);
    invoke("subtitle_style_set", { name, value }).catch((e) =>
      console.error("subtitle_style_set failed:", e),
    );
    void saveSettings();
  };

  const resetSubtitleStyle = () => {
    setSubtitleStyle(DEFAULT_SUBTITLE_STYLE);
    for (const [name, value] of Object.entries(DEFAULT_SUBTITLE_STYLE)) {
      invoke("subtitle_style_set", { name, value }).catch(() => {});
    }
    void saveSettings();
  };

  const [draftMode, setDraftMode] = useState(whisperMode);
  const [draftModelPath, setDraftModelPath] = useState(whisperModelPath ?? "");
  const [draftBinaryPath, setDraftBinaryPath] = useState(whisperBinaryPath ?? "");
  // Read straight from settings.json rather than through settingsStore: the
  // store owns a fixed set of fields and rewrites them as one blob, while
  // these keys are shared with the CLI and the subtitle dialog.
  const [osKey, setOsKey] = useState("");
  const [osLanguages, setOsLanguages] = useState("");
  const [osConfigured, setOsConfigured] = useState(false);

  useEffect(() => {
    invoke<{ configured: boolean; languages: string[] }>("opensubtitles_configured")
      .then((c) => {
        setOsConfigured(c.configured);
        setOsLanguages(c.languages.join(","));
      })
      .catch(() => undefined);
  }, []);

  const saveOsSetting = async (key: string, value: string) => {
    try {
      await invoke("settings_set_key", { key, value });
      if (key === "opensubtitles_api_key") {
        setOsConfigured(value.length > 0);
        // Don't keep the key in component state once it's stored; the
        // placeholder says it's saved.
        setOsKey("");
      }
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "error",
            message: t.settings.saveFailed.replace("{error}", String(e).slice(0, 100)),
          },
        }),
      );
    }
  };

  const [draftProxy, setDraftProxy] = useState(proxy ?? "");
  const [draftSubLangs, setDraftSubLangs] = useState(subtitleLanguages.join(","));
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
    // Parse the comma-separated language list into a string array.
    const langs = draftSubLangs
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    setSubtitleLanguages(langs.length > 0 ? langs : ["en"]);
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

            {/* File Associations */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.fileAssoc.section}
              </p>
              <p className="mb-3 text-[11px] leading-relaxed text-white/55">
                {t.settings.fileAssoc.body}
              </p>
              <button
                className="rounded-lg border border-white/10 bg-white/4 px-4 py-2 text-[11px] font-medium text-white hover:border-white/20 hover:bg-white/8 transition"
                onClick={() => {
                  invoke("open_default_apps_settings").catch((e) => console.error(e));
                }}
              >
                {t.settings.fileAssoc.button}
              </button>
              <p className="mt-2 text-[10px] leading-relaxed text-white/30">
                {t.settings.fileAssoc.hint}
              </p>
            </div>

            {/* Welcome screen — the only way back to the first-run card
                from inside the window. Also `unflick settings set
                onboarding_seen false`, which is the same key. */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.onboarding.section}
              </p>
              <p className="mb-3 text-[11px] leading-relaxed text-white/55">
                {t.settings.onboarding.body}
              </p>
              <button
                className="rounded-lg border border-white/10 bg-white/4 px-4 py-2 text-[11px] font-medium text-white hover:border-white/20 hover:bg-white/8 transition"
                onClick={() => {
                  window.dispatchEvent(new CustomEvent(SHOW_ONBOARDING_EVENT));
                  onClose();
                }}
              >
                {t.settings.onboarding.button}
              </button>
            </div>

            {/* Screenshots */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.screenshots.section}
              </p>
              <div className="flex items-center gap-2">
                <div className="flex-1 truncate rounded-lg border border-white/6 bg-white/4 px-3 py-2 text-[11px] text-white/70 font-mono">
                  {screenshotDir || t.settings.screenshots.askEachTime}
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
                  {t.settings.screenshots.choose}
                </button>
                {screenshotDir && (
                  <button
                    className="rounded-lg p-2 text-white/30 hover:bg-white/6 hover:text-white/70 transition"
                    onClick={() => {
                      setScreenshotDir(null);
                      void saveSettings();
                    }}
                    title={t.settings.screenshots.clear}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                  </button>
                )}
              </div>
              <p className="mt-2 text-[10px] leading-relaxed text-white/30">
                {t.settings.screenshots.hint}
              </p>
            </div>

            <KeybindSettings />

            <MouseSettings />

            {/* Subtitle style. Changes apply to mpv immediately so the
                sliders can be judged against the video behind the panel. */}
            <div>
              <div className="mb-3 flex items-center justify-between">
                <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
                  {t.subtitle.style}
                </p>
                <button
                  className="rounded-lg px-2 py-1 text-[10px] font-medium text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
                  onClick={resetSubtitleStyle}
                >
                  {t.subtitle.reset}
                </button>
              </div>

              <div className="space-y-3">
                {([
                  { key: "scale", label: t.subtitle.styleSize, min: 0.5, max: 2.5, step: 0.05, format: (v: number) => `${v.toFixed(2)}×` },
                  { key: "pos", label: t.subtitle.stylePosition, min: 0, max: 150, step: 5, format: (v: number) => String(v) },
                  { key: "border_size", label: t.subtitle.styleOutline, min: 0, max: 10, step: 0.5, format: (v: number) => v.toFixed(1) },
                ] as const).map(({ key, label, min, max, step, format }) => (
                  <div key={key} className="flex items-center gap-3">
                    <span className="w-20 flex-shrink-0 text-[11px] text-white/55">{label}</span>
                    <input
                      type="range"
                      min={min}
                      max={max}
                      step={step}
                      value={subtitleStyle[key]}
                      onChange={(e) => applySubtitleStyle(key, Number(e.target.value))}
                      className="h-1 flex-1 cursor-pointer appearance-none rounded-full bg-white/10 accent-brand-purple"
                    />
                    <span className="w-12 flex-shrink-0 text-right text-[11px] tabular-nums text-white/40">
                      {format(subtitleStyle[key])}
                    </span>
                  </div>
                ))}

                <div className="flex items-center gap-3">
                  <span className="w-20 flex-shrink-0 text-[11px] text-white/55">
                    {t.subtitle.styleColor}
                  </span>
                  <input
                    type="color"
                    // mpv wants #RRGGBBAA; the native picker only gives
                    // #RRGGBB, so opacity is pinned opaque here.
                    value={subtitleStyle.color.slice(0, 7)}
                    onChange={(e) => applySubtitleStyle("color", `${e.target.value.toUpperCase()}FF`)}
                    className="h-6 w-10 cursor-pointer rounded border border-white/10 bg-transparent"
                  />
                  <button
                    className={`ml-auto rounded-lg border px-3 py-1 text-[11px] font-medium transition-all ${
                      subtitleStyle.bold
                        ? "border-brand-purple/40 bg-brand-purple/15 text-white"
                        : "border-white/10 bg-white/4 text-white/50 hover:border-white/20 hover:text-white/80"
                    }`}
                    onClick={() => applySubtitleStyle("bold", !subtitleStyle.bold)}
                  >
                    {t.subtitle.styleBold}
                  </button>
                </div>
              </div>
            </div>

            {/* AI Subtitles */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.ai.section}
              </p>

              <div className="mb-4 flex gap-2">
                {(["off", "local"] as const).map((mode) => {
                  const labels = { off: t.settings.ai.modeOff, local: t.settings.ai.modeLocal };
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
                    <span className="font-medium">{t.settings.ai.bundledReady}</span>
                  </div>
                  <p className="mt-1.5 text-[10px] leading-relaxed text-white/30">
                    {t.settings.ai.bundledBody}
                  </p>
                  <button
                    className="mt-2 text-[10px] text-white/30 hover:text-white/50"
                    onClick={() => setShowAdvanced(true)}
                  >
                    {t.settings.ai.useCustom}
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
                      {t.settings.ai.resetBundled}
                    </button>
                  )}
                  <div>
                    <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.ai.whisperBinary}
                    </label>
                    <div className="flex gap-2">
                      <input type="text" value={draftBinaryPath} onChange={(e) => setDraftBinaryPath(e.target.value)} placeholder="path/to/whisper-cli.exe" className={inputClass} />
                      <button className={browseBtnClass} onClick={() => handleBrowse(setDraftBinaryPath)}>
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" /></svg>
                        {t.common.browse}
                      </button>
                    </div>
                  </div>
                  <div>
                    <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.ai.whisperModel}
                    </label>
                    <div className="flex gap-2">
                      <input type="text" value={draftModelPath} onChange={(e) => setDraftModelPath(e.target.value)} placeholder="path/to/ggml-base.en.bin" className={inputClass} />
                      <button className={browseBtnClass} onClick={() => handleBrowse(setDraftModelPath)}>
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" /></svg>
                        {t.common.browse}
                      </button>
                    </div>
                  </div>
                  <p className="text-[10px] leading-relaxed text-white/15">
                    {t.settings.ai.downloadHint}
                  </p>
                </div>
              )}

              {draftMode === "off" && (
                <p className="text-[10px] leading-relaxed text-white/15">
                  {t.settings.ai.disabledBody}
                </p>
              )}
            </div>

            {/* Online subtitles. Kept out of the settings draft/save cycle:
                these two keys are written straight through with
                settings_set_key, so they can also be set from the subtitle
                dialog and from the CLI without three writers racing over one
                blob. */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.onlineSubs.section}
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div>
                  <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                    {t.settings.onlineSubs.keyLabel}
                  </label>
                  <div className="flex gap-2">
                    <input
                      type="password"
                      value={osKey}
                      onChange={(e) => setOsKey(e.target.value)}
                      placeholder={osConfigured ? t.settings.onlineSubs.keySaved : t.onlineSubtitles.keyPlaceholder}
                      className="flex-1 rounded-lg bg-white/5 px-3 py-2 text-[11px] text-white/80 outline-none ring-1 ring-white/8 placeholder:text-white/20 focus:ring-brand-purple/50"
                    />
                    <button
                      type="button"
                      disabled={!osKey.trim()}
                      onClick={() => saveOsSetting("opensubtitles_api_key", osKey.trim())}
                      className="rounded-lg bg-white/8 px-3 py-2 text-[11px] text-white/70 transition-colors hover:bg-white/14 disabled:opacity-30"
                    >
                      {t.common.save}
                    </button>
                  </div>
                  <p className="mt-1.5 text-[10px] leading-relaxed text-white/30">
                    {t.settings.onlineSubs.keyHintPrefix}{" "}
                    <a
                      href="https://www.opensubtitles.com/consumers"
                      target="_blank"
                      rel="noreferrer"
                      className="text-brand-purple underline decoration-brand-purple/40 underline-offset-2"
                    >
                      opensubtitles.com
                    </a>
                    {t.settings.onlineSubs.keyHintSuffix}
                  </p>
                </div>

                <div className="border-t border-white/6 pt-3">
                  <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                    {t.settings.onlineSubs.languagesLabel}
                  </label>
                  <input
                    type="text"
                    value={osLanguages}
                    onChange={(e) => setOsLanguages(e.target.value)}
                    onBlur={() =>
                      saveOsSetting("opensubtitles_languages", osLanguages.trim())
                    }
                    placeholder="en"
                    className="w-full rounded-lg bg-white/5 px-3 py-2 text-[11px] text-white/80 outline-none ring-1 ring-white/8 placeholder:text-white/20 focus:ring-brand-purple/50"
                  />
                  <p className="mt-1.5 text-[10px] leading-relaxed text-white/30">
                    {t.settings.onlineSubs.languagesHint}{" "}
                    <span className="font-mono text-white/45">zh-CN,en</span>.
                  </p>
                </div>
              </div>
            </div>

            {/* Network */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.network.section}
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div>
                  <div className="mb-1.5 flex items-center justify-between">
                    <label className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.network.useSystemProxy}
                    </label>
                    <Toggle
                      checked={draftProxy === "system"}
                      onChange={(next) => setDraftProxy(next ? "system" : "")}
                      label={t.settings.network.useSystemProxy}
                    />
                  </div>
                  {draftProxy === "system" && systemProxy && (
                    <p className="text-[10px] leading-relaxed text-emerald-300/80">
                      ✓ {t.settings.network.detected}{" "}
                      <span className="font-mono text-white/50">{systemProxy}</span>
                    </p>
                  )}
                  {draftProxy === "system" && !systemProxy && (
                    <p className="text-[10px] leading-relaxed text-amber-300/80">
                      {t.settings.network.none}
                    </p>
                  )}
                  {draftProxy !== "system" && (
                    <p className="text-[10px] leading-relaxed text-white/30">
                      {t.settings.network.hint}
                    </p>
                  )}
                </div>

                <div className="border-t border-white/6 pt-3">
                  <div className="mb-1.5 flex items-center justify-between">
                    <p className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.tools.section}
                    </p>
                    {ytDlpAvailable && (
                      <button
                        className="rounded-md border border-white/8 bg-white/4 px-2 py-0.5 text-[10px] text-white/40 transition-colors hover:bg-white/8 hover:text-white/70 disabled:opacity-50"
                        onClick={handleUpdateYtDlp}
                        disabled={ytDlpUpdating}
                      >
                        {ytDlpUpdating ? t.settings.tools.updating : t.settings.tools.update}
                      </button>
                    )}
                  </div>
                  {ytDlpAvailable === null ? (
                    <p className="text-[10px] text-white/25">{t.settings.tools.ytDlpChecking}</p>
                  ) : ytDlpAvailable ? (
                    <div className="text-[10px] text-emerald-300/80">
                      ✓ {t.settings.tools.ready}
                      <p className="mt-1 text-white/30">
                        {ytDlpVersion && (
                          <>
                            {t.settings.tools.ytDlpVersion.split("{version}")[0]}
                            <span className="font-mono text-white/40">{ytDlpVersion}</span>
                            {t.settings.tools.ytDlpVersion.split("{version}")[1]}
                          </>
                        )}
                        {ytDlpSource === "user" && <> · {t.settings.tools.sourceUser}</>}
                        {ytDlpSource === "bundled" && <> · {t.settings.tools.sourceBundled}</>}
                        {ytDlpSource === "path" && <> · {t.settings.tools.sourcePath}</>}
                      </p>
                      <p className="mt-1 text-white/25">
                        {t.settings.tools.updateHint}
                      </p>
                    </div>
                  ) : (
                    <div className="text-[10px] text-amber-300/80">
                      ⚠ {t.settings.tools.missing}
                      <p className="mt-1 text-white/30">
                        {t.settings.tools.installFrom}{" "}
                        <a href="https://github.com/yt-dlp/yt-dlp/releases" target="_blank" rel="noreferrer" className="text-amber-200 underline hover:text-amber-100">
                          github.com/yt-dlp/yt-dlp
                        </a>
                        {" "}{t.settings.tools.orDownload}
                      </p>
                      <button
                        className="mt-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2.5 py-1 text-[10px] text-amber-200 transition-colors hover:bg-amber-500/20 disabled:opacity-50"
                        onClick={handleUpdateYtDlp}
                        disabled={ytDlpUpdating}
                      >
                        {ytDlpUpdating ? t.settings.tools.downloading : t.settings.tools.download}
                      </button>
                    </div>
                  )}
                  {ytDlpUpdateError && (
                    <p className="mt-1.5 text-[10px] text-red-300/80">{ytDlpUpdateError}</p>
                  )}
                </div>
              </div>
            </div>

            {/* Streaming (P1 — quality + cookies) */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.streaming.title}
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div>
                  <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                    {t.settings.streaming.qualityLabel}
                  </label>
                  <select
                    value={preferredQuality ?? "auto"}
                    onChange={(e) => {
                      const v = e.target.value;
                      setPreferredQuality(v === "auto" ? null : (v as Exclude<typeof preferredQuality, null>));
                      void saveSettings();
                    }}
                    className="w-full rounded-lg border border-white/10 bg-[#1c1c26] px-3 py-2 text-[12px] text-white outline-none focus:border-brand-purple/40"
                  >
                    {/* Same explicit option styling as the language picker —
                        WebView2 ignores Tailwind on <option>. */}
                    <option value="auto" style={{ background: "#1c1c26", color: "#ffffff" }}>{t.settings.streaming.qualityAuto}</option>
                    <option value="2160p" style={{ background: "#1c1c26", color: "#ffffff" }}>2160p</option>
                    <option value="1440p" style={{ background: "#1c1c26", color: "#ffffff" }}>1440p</option>
                    <option value="1080p" style={{ background: "#1c1c26", color: "#ffffff" }}>1080p</option>
                    <option value="720p" style={{ background: "#1c1c26", color: "#ffffff" }}>720p</option>
                    <option value="480p" style={{ background: "#1c1c26", color: "#ffffff" }}>480p</option>
                    <option value="audio_only" style={{ background: "#1c1c26", color: "#ffffff" }}>{t.settings.streaming.qualityAudioOnly}</option>
                  </select>
                </div>

                <div className="border-t border-white/6 pt-3">
                  <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                    {t.settings.streaming.cookiesLabel}
                  </label>
                  <select
                    value={cookiesBrowser ?? "none"}
                    onChange={(e) => {
                      const v = e.target.value;
                      setCookiesBrowser(v === "none" ? null : (v as Exclude<typeof cookiesBrowser, null>));
                      void saveSettings();
                    }}
                    className="w-full rounded-lg border border-white/10 bg-[#1c1c26] px-3 py-2 text-[12px] text-white outline-none focus:border-brand-purple/40"
                  >
                    <option value="none" style={{ background: "#1c1c26", color: "#ffffff" }}>{t.settings.streaming.cookiesNone}</option>
                    <option value="firefox" style={{ background: "#1c1c26", color: "#ffffff" }}>Firefox</option>
                    <option value="chrome" style={{ background: "#1c1c26", color: "#ffffff" }}>Chrome</option>
                    <option value="chromium" style={{ background: "#1c1c26", color: "#ffffff" }}>Chromium</option>
                    <option value="safari" style={{ background: "#1c1c26", color: "#ffffff" }}>Safari</option>
                    <option value="edge" style={{ background: "#1c1c26", color: "#ffffff" }}>Edge</option>
                    <option value="brave" style={{ background: "#1c1c26", color: "#ffffff" }}>Brave</option>
                  </select>
                  <p className="mt-2 text-[10px] leading-relaxed text-white/30">
                    {t.settings.streaming.cookiesHelp}
                  </p>
                </div>
              </div>
            </div>

            {/* SponsorBlock (P2 — auto-skip community-curated ad/intro/outro segments) */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.sponsorblock.title}
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-[11px] text-white/55">
                    {t.settings.sponsorblock.enabled}
                  </span>
                  <Toggle
                    checked={sponsorblockEnabled}
                    onChange={(next) => {
                      setSponsorblockEnabled(next);
                      void saveSettings();
                    }}
                    label={t.settings.sponsorblock.enabled}
                  />
                </div>
                {sponsorblockEnabled && (
                  <div className="border-t border-white/6 pt-3">
                    <p className="mb-2 text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.sponsorblock.categories}
                    </p>
                    <div className="grid grid-cols-2 gap-1.5">
                      {(["sponsor", "selfpromo", "intro", "outro", "interaction"] as const).map(
                        (cat) => {
                          const checked = sponsorblockCategories.includes(cat);
                          const labels: Record<typeof cat, string> = {
                            sponsor: t.settings.sponsorblock.cat.sponsor,
                            selfpromo: t.settings.sponsorblock.cat.selfpromo,
                            intro: t.settings.sponsorblock.cat.intro,
                            outro: t.settings.sponsorblock.cat.outro,
                            interaction: t.settings.sponsorblock.cat.interaction,
                          };
                          return (
                            <button
                              key={cat}
                              type="button"
                              onClick={() => {
                                const next = checked
                                  ? sponsorblockCategories.filter((c) => c !== cat)
                                  : [...sponsorblockCategories, cat];
                                setSponsorblockCategories(next);
                                void saveSettings();
                              }}
                              className={`flex items-center gap-2 rounded-md border px-2 py-1.5 text-[10px] text-left transition-colors ${
                                checked
                                  ? "border-brand-purple/30 bg-brand-purple/10 text-brand-purple"
                                  : "border-white/6 bg-white/4 text-white/40 hover:border-white/10 hover:text-white/60"
                              }`}
                            >
                              <span
                                className={`flex h-3 w-3 flex-shrink-0 items-center justify-center rounded-sm border ${
                                  checked
                                    ? "border-brand-purple bg-brand-purple"
                                    : "border-white/20"
                                }`}
                              >
                                {checked && (
                                  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                                    <polyline points="20 6 9 17 4 12" />
                                  </svg>
                                )}
                              </span>
                              {labels[cat]}
                            </button>
                          );
                        }
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* Auto-download subtitles. Two engines behind one switch:
                yt-dlp --write-sub for streaming URLs, OpenSubtitles for
                files on disk. The second needs a key, so say so here
                rather than leaving the setting quietly inert. */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.subtitles.autoDownload}
              </p>
              <div className="space-y-3 rounded-xl border border-white/6 bg-white/3 p-4">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-[11px] text-white/55">
                    {t.settings.subtitles.autoDownload}
                  </span>
                  <Toggle
                    checked={autoDownloadSubtitles}
                    onChange={(next) => {
                      setAutoDownloadSubtitles(next);
                      void saveSettings();
                    }}
                    label={t.settings.subtitles.autoDownload}
                  />
                </div>
                {autoDownloadSubtitles && (
                  <div className="border-t border-white/6 pt-3">
                    <p className="mb-3 text-[10px] leading-relaxed text-white/30">
                      {t.settings.subtitles.autoDownloadHint}
                    </p>
                    {!osConfigured && (
                      <p className="mb-3 text-[10px] leading-relaxed text-amber-300/80">
                        {t.settings.subtitles.autoDownloadNeedsKey}
                      </p>
                    )}
                    <label className="mb-1.5 block text-[10px] font-semibold uppercase tracking-widest text-white/20">
                      {t.settings.subtitles.languages}
                    </label>
                    <input
                      type="text"
                      value={draftSubLangs}
                      onChange={(e) => setDraftSubLangs(e.target.value)}
                      placeholder="en,zh-CN"
                      className={inputClass}
                    />
                    <p className="mt-1.5 text-[10px] text-white/25">
                      {t.settings.subtitles.languagesHint}
                    </p>
                  </div>
                )}
              </div>
            </div>

            {/* Theme */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.theme}
              </p>
              <div className="flex gap-2">
                {([
                  { value: "dark" as const, label: t.settings.themeDark, color: "#030712" },
                  { value: "midnight" as const, label: t.settings.themeMidnight, color: "#000000" },
                  { value: "purple" as const, label: t.settings.themePurple, color: "#0a0015" },
                ]).map((choice) => (
                  <button
                    key={choice.value}
                    onClick={() => setTheme(choice.value)}
                    className={`flex flex-1 flex-col items-center gap-1.5 rounded-lg border px-3 py-2 text-[11px] font-medium transition-all duration-150 ${
                      theme === choice.value
                        ? "border-brand-purple/30 bg-brand-purple/10 text-brand-purple"
                        : "border-white/6 bg-white/4 text-white/35 hover:border-white/10 hover:text-white/50"
                    }`}
                  >
                    <span className="h-5 w-full rounded border border-white/6" style={{ backgroundColor: choice.color }} />
                    {choice.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Always on top */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.settings.alwaysOnTop}
              </p>
              <button
                onClick={async () => {
                  const next = !alwaysOnTop;
                  setAlwaysOnTop(next);
                  try {
                    await invoke("set_always_on_top", { enabled: next });
                  } catch (e) {
                    console.error("[settings] set_always_on_top failed", e);
                  }
                }}
                className={`flex w-full items-center justify-between rounded-lg border px-3 py-2.5 text-[11px] transition-all ${
                  alwaysOnTop
                    ? "border-brand-purple/30 bg-brand-purple/10"
                    : "border-white/6 bg-white/4 hover:border-white/10"
                }`}
              >
                <div className="flex flex-col items-start gap-0.5 text-left">
                  <span className={alwaysOnTop ? "text-brand-purple" : "text-white/55"}>
                    {alwaysOnTop ? t.common.on : t.common.off}
                  </span>
                  <span className="text-[10px] leading-snug text-white/30">
                    {t.settings.alwaysOnTopHint}
                  </span>
                </div>
                <ToggleTrack checked={alwaysOnTop} />
              </button>
            </div>

            <DiskCleanup />

            {/* Music mode for audio files */}
            <div>
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-white/25">
                {t.music.section}
              </p>
              <button
                onClick={() => setMusicModeAuto(!musicModeAuto)}
                className={`flex w-full items-center justify-between rounded-lg border px-3 py-2.5 text-[11px] transition-all ${
                  musicModeAuto
                    ? "border-brand-purple/30 bg-brand-purple/10"
                    : "border-white/6 bg-white/4 hover:border-white/10"
                }`}
              >
                <div className="flex flex-col items-start gap-0.5 text-left">
                  <span className={musicModeAuto ? "text-brand-purple" : "text-white/55"}>
                    {t.music.auto}
                  </span>
                  <span className="text-[10px] leading-snug text-white/30">
                    {t.music.autoHint}
                  </span>
                </div>
                <ToggleTrack checked={musicModeAuto} />
              </button>
            </div>
          </div>

          {/* Footer */}
          <div className="flex flex-shrink-0 items-center justify-end gap-3 px-5 py-4" style={{ borderTop: "1px solid var(--border-subtle)" }}>
            <button
              className="rounded-lg px-4 py-2 text-[11px] font-medium text-white/30 transition-colors hover:bg-white/6 hover:text-white/50"
              onClick={onClose}
            >
              {t.common.cancel}
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
                  {t.common.loading}
                </span>
              ) : saveStatus === "saved" ? t.settings.savedToast : t.common.save}
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

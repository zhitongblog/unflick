import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../../stores/settingsStore";

function CloseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function FolderIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
    </svg>
  );
}

function EyeIcon({ visible }: { visible: boolean }) {
  if (visible) {
    return (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    );
  }
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17.94 17.94A10.07 10.07 0 0112 20c-7 0-11-8-11-8a18.45 18.45 0 015.06-5.94" />
      <path d="M9.9 4.24A9.12 9.12 0 0112 4c7 0 11 8 11 8a18.5 18.5 0 01-2.16 3.19" />
      <line x1="1" y1="1" x2="23" y2="23" />
    </svg>
  );
}

interface SettingsPanelProps {
  onClose: () => void;
}

export default function SettingsPanel({ onClose }: SettingsPanelProps) {
  const {
    whisperMode,
    whisperModelPath,
    whisperBinaryPath,
    openaiApiKey,
    setWhisperMode,
    setWhisperModelPath,
    setWhisperBinaryPath,
    setOpenaiApiKey,
    saveSettings,
  } = useSettingsStore();

  // Local draft state — only commit on Save
  const [draftMode, setDraftMode] = useState(whisperMode);
  const [draftModelPath, setDraftModelPath] = useState(whisperModelPath ?? "");
  const [draftBinaryPath, setDraftBinaryPath] = useState(whisperBinaryPath ?? "");
  const [draftApiKey, setDraftApiKey] = useState(openaiApiKey ?? "");
  const [showApiKey, setShowApiKey] = useState(false);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved">("idle");

  const overlayRef = useRef<HTMLDivElement>(null);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === overlayRef.current) onClose();
  };

  const handleBrowseBinary = async () => {
    try {
      const result = await invoke<{ path: string | null }>("open_file_dialog");
      if (result.path) setDraftBinaryPath(result.path);
    } catch {
      // ignore
    }
  };

  const handleBrowseModel = async () => {
    try {
      const result = await invoke<{ path: string | null }>("open_file_dialog");
      if (result.path) setDraftModelPath(result.path);
    } catch {
      // ignore
    }
  };

  const handleSave = async () => {
    setSaveStatus("saving");
    setWhisperMode(draftMode);
    setWhisperModelPath(draftModelPath || null);
    setWhisperBinaryPath(draftBinaryPath || null);
    setOpenaiApiKey(draftApiKey || null);
    await saveSettings();
    setSaveStatus("saved");
    setTimeout(() => setSaveStatus("idle"), 1500);
  };

  return (
    <AnimatePresence>
      <motion.div
        ref={overlayRef}
        className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.15 }}
        onClick={handleOverlayClick}
      >
        <motion.div
          className="w-[440px] rounded-2xl border border-white/10 bg-gray-900 shadow-2xl"
          initial={{ scale: 0.95, opacity: 0, y: 8 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.95, opacity: 0, y: 8 }}
          transition={{ duration: 0.15 }}
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-white/5 px-5 py-4">
            <h2 className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-sm font-semibold text-transparent">
              Settings
            </h2>
            <button
              className="rounded-lg p-1 text-gray-500 transition-colors hover:bg-gray-800 hover:text-gray-300"
              onClick={onClose}
            >
              <CloseIcon />
            </button>
          </div>

          {/* Body */}
          <div className="p-5">
            {/* AI Subtitles Section */}
            <div className="mb-5">
              <p className="mb-3 text-[10px] font-medium uppercase tracking-wider text-gray-500">
                AI Subtitles
              </p>

              {/* Mode radio group */}
              <div className="mb-4 flex gap-2">
                {(["off", "local", "api"] as const).map((mode) => {
                  const labels = { off: "Off", local: "Local Whisper", api: "OpenAI API" };
                  const active = draftMode === mode;
                  return (
                    <button
                      key={mode}
                      onClick={() => setDraftMode(mode)}
                      className={`flex-1 rounded-lg border px-3 py-2 text-xs font-medium transition-colors ${
                        active
                          ? "border-brand-purple bg-brand-purple/10 text-brand-purple"
                          : "border-gray-700 bg-gray-800 text-gray-400 hover:border-gray-600 hover:text-gray-300"
                      }`}
                    >
                      {labels[mode]}
                    </button>
                  );
                })}
              </div>

              {/* Local Whisper config */}
              {draftMode === "local" && (
                <div className="space-y-3 rounded-xl border border-gray-800 bg-gray-800/40 p-4">
                  {/* Binary path */}
                  <div>
                    <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
                      Whisper Binary
                    </label>
                    <div className="flex gap-2">
                      <input
                        type="text"
                        value={draftBinaryPath}
                        onChange={(e) => setDraftBinaryPath(e.target.value)}
                        placeholder="path/to/whisper-cli.exe"
                        className="min-w-0 flex-1 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
                      />
                      <button
                        className="flex flex-shrink-0 items-center gap-1.5 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-400 transition-colors hover:bg-gray-700 hover:text-gray-200"
                        onClick={handleBrowseBinary}
                      >
                        <FolderIcon />
                        Browse
                      </button>
                    </div>
                  </div>

                  {/* Model path */}
                  <div>
                    <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
                      Model File
                    </label>
                    <div className="flex gap-2">
                      <input
                        type="text"
                        value={draftModelPath}
                        onChange={(e) => setDraftModelPath(e.target.value)}
                        placeholder="path/to/ggml-base.en.bin"
                        className="min-w-0 flex-1 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
                      />
                      <button
                        className="flex flex-shrink-0 items-center gap-1.5 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-400 transition-colors hover:bg-gray-700 hover:text-gray-200"
                        onClick={handleBrowseModel}
                      >
                        <FolderIcon />
                        Browse
                      </button>
                    </div>
                  </div>

                  {/* Help text */}
                  <p className="text-[11px] leading-relaxed text-gray-600">
                    Download{" "}
                    <span className="text-gray-500">whisper.cpp</span>{" "}
                    from GitHub and a GGML model file (e.g.{" "}
                    <span className="text-gray-500">ggml-base.en.bin</span>
                    ).
                  </p>
                </div>
              )}

              {/* OpenAI API config */}
              {draftMode === "api" && (
                <div className="space-y-3 rounded-xl border border-gray-800 bg-gray-800/40 p-4">
                  <div>
                    <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
                      OpenAI API Key
                    </label>
                    <div className="flex gap-2">
                      <input
                        type={showApiKey ? "text" : "password"}
                        value={draftApiKey}
                        onChange={(e) => setDraftApiKey(e.target.value)}
                        placeholder="sk-..."
                        className="min-w-0 flex-1 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 font-mono text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
                      />
                      <button
                        className="flex flex-shrink-0 items-center rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-gray-400 transition-colors hover:bg-gray-700 hover:text-gray-200"
                        onClick={() => setShowApiKey((v) => !v)}
                        title={showApiKey ? "Hide key" : "Show key"}
                      >
                        <EyeIcon visible={showApiKey} />
                      </button>
                    </div>
                  </div>

                  {/* Help text */}
                  <p className="text-[11px] leading-relaxed text-gray-600">
                    Uses the OpenAI Whisper API for transcription. Requires{" "}
                    <span className="text-gray-500">curl</span> and{" "}
                    <span className="text-gray-500">ffmpeg</span> on your PATH.
                  </p>
                </div>
              )}

              {draftMode === "off" && (
                <p className="text-[11px] leading-relaxed text-gray-600">
                  AI subtitle generation is disabled. Enable Local Whisper or OpenAI API to generate subtitles automatically from video audio.
                </p>
              )}
            </div>
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-3 border-t border-white/5 px-5 py-4">
            <button
              className="rounded-lg px-4 py-2 text-xs text-gray-400 transition-colors hover:bg-gray-800 hover:text-gray-200"
              onClick={onClose}
            >
              Cancel
            </button>
            <button
              className="rounded-xl bg-gradient-to-r from-brand-purple to-brand-pink px-5 py-2 text-xs font-semibold text-white transition-opacity hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-60"
              onClick={handleSave}
              disabled={saveStatus === "saving"}
            >
              {saveStatus === "saving" ? (
                <span className="flex items-center gap-2">
                  <span className="h-3 w-3 animate-spin rounded-full border-2 border-white border-t-transparent" />
                  Saving...
                </span>
              ) : saveStatus === "saved" ? (
                "Saved!"
              ) : (
                "Save"
              )}
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

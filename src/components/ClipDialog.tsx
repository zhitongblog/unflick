import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";

interface ClipDialogProps {
  onClose: () => void;
}

function formatTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
}

function parseTimeInput(value: string): number {
  // Accept "ss", "mm:ss", or "hh:mm:ss"
  const parts = value.split(":").map(Number);
  if (parts.some(isNaN)) return NaN;
  if (parts.length === 1) return parts[0];
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] * 3600 + parts[1] * 60 + parts[2];
}

export default function ClipDialog({ onClose }: ClipDialogProps) {
  const { position, file } = usePlayerStore();
  const overlayRef = useRef<HTMLDivElement>(null);

  const [startInput, setStartInput] = useState(() => formatTime(Math.floor(position)));
  const [endInput, setEndInput] = useState(() => formatTime(Math.floor(position + 10)));
  const [asGif, setAsGif] = useState(false);
  const [outputPath, setOutputPath] = useState("");
  const [status, setStatus] = useState<{ type: "idle" | "running" | "done" | "error"; message: string }>({
    type: "idle",
    message: "",
  });

  // Build a default output filename whenever asGif or outputPath changes
  useEffect(() => {
    if (!outputPath) {
      const ext = asGif ? "gif" : "mp4";
      const ts = Date.now();
      setOutputPath(`unflick-clip-${ts}.${ext}`);
    }
  }, []); // only on mount

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleBrowse = async () => {
    const ext = asGif ? "gif" : "mp4";
    const defaultName = `unflick-clip-${Date.now()}.${ext}`;
    try {
      const result = await invoke<{ path: string | null }>("save_file_dialog", { defaultName });
      if (result.path) setOutputPath(result.path);
    } catch {
      // save_file_dialog may not be available in dev — ignore
    }
  };

  const handleExtract = async () => {
    const start = parseTimeInput(startInput);
    const end = parseTimeInput(endInput);

    if (isNaN(start) || isNaN(end)) {
      setStatus({ type: "error", message: "Invalid time format. Use mm:ss or hh:mm:ss." });
      return;
    }
    if (end <= start) {
      setStatus({ type: "error", message: "End time must be after start time." });
      return;
    }
    if (!file) {
      setStatus({ type: "error", message: "No file is currently loaded." });
      return;
    }

    setStatus({ type: "running", message: "Extracting clip..." });

    try {
      const result = await invoke<{ output: string }>("extract_clip", {
        input: file,
        start,
        end,
        output: outputPath,
        asGif,
      });
      setStatus({ type: "done", message: `Saved to: ${result.output}` });
    } catch (err) {
      setStatus({ type: "error", message: String(err) });
    }
  };

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === overlayRef.current) onClose();
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
          className="w-80 rounded-2xl border border-white/10 bg-gray-900 p-5 shadow-2xl"
          initial={{ scale: 0.95, opacity: 0, y: 8 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.95, opacity: 0, y: 8 }}
          transition={{ duration: 0.15 }}
        >
          {/* Header */}
          <div className="mb-4 flex items-center justify-between">
            <h2 className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-sm font-semibold text-transparent">
              Extract Clip
            </h2>
            <button
              className="rounded-lg p-1 text-gray-500 transition-colors hover:bg-gray-800 hover:text-gray-300"
              onClick={onClose}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>

          {/* Time inputs */}
          <div className="mb-3 flex gap-3">
            <div className="flex-1">
              <label className="mb-1 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
                Start
              </label>
              <input
                type="text"
                value={startInput}
                onChange={(e) => setStartInput(e.target.value)}
                placeholder="0:00"
                className="w-full rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
              />
            </div>
            <div className="flex-1">
              <label className="mb-1 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
                End
              </label>
              <input
                type="text"
                value={endInput}
                onChange={(e) => setEndInput(e.target.value)}
                placeholder="0:10"
                className="w-full rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
              />
            </div>
          </div>

          {/* Output path */}
          <div className="mb-3">
            <label className="mb-1 block text-[10px] font-medium uppercase tracking-wider text-gray-500">
              Output
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={outputPath}
                onChange={(e) => setOutputPath(e.target.value)}
                placeholder="auto"
                className="min-w-0 flex-1 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-200 outline-none transition-colors focus:border-brand-purple"
              />
              <button
                className="flex-shrink-0 rounded-lg border border-gray-700 bg-gray-800 px-2.5 py-1.5 text-xs text-gray-400 transition-colors hover:bg-gray-700 hover:text-gray-200"
                onClick={handleBrowse}
              >
                Browse
              </button>
            </div>
          </div>

          {/* GIF toggle */}
          <div className="mb-4 flex items-center gap-2">
            <button
              role="switch"
              aria-checked={asGif}
              onClick={() => setAsGif((v) => !v)}
              className={`relative h-5 w-9 rounded-full transition-colors ${asGif ? "bg-brand-purple" : "bg-gray-700"}`}
            >
              <span
                className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${asGif ? "translate-x-4" : "translate-x-0.5"}`}
              />
            </button>
            <span className="text-xs text-gray-400">Save as GIF</span>
          </div>

          {/* Status message */}
          {status.message && (
            <div
              className={`mb-3 rounded-lg px-3 py-2 text-xs ${
                status.type === "error"
                  ? "bg-red-900/40 text-red-400"
                  : status.type === "done"
                  ? "bg-green-900/40 text-green-400"
                  : "bg-gray-800 text-gray-400"
              }`}
            >
              {status.message}
            </div>
          )}

          {/* Extract button */}
          <button
            className="w-full rounded-xl bg-gradient-to-r from-brand-purple to-brand-pink py-2 text-sm font-semibold text-white transition-opacity hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
            onClick={handleExtract}
            disabled={status.type === "running"}
          >
            {status.type === "running" ? (
              <span className="flex items-center justify-center gap-2">
                <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-white border-t-transparent" />
                Extracting...
              </span>
            ) : status.type === "done" ? (
              "Extract Another"
            ) : (
              "Extract"
            )}
          </button>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

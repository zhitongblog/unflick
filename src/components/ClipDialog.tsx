import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";

function formatTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function parseTimeInput(value: string): number {
  const parts = value.split(":").map(Number);
  if (parts.some(isNaN)) return NaN;
  if (parts.length === 1) return parts[0];
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] * 3600 + parts[1] * 60 + parts[2];
}

export default function ClipDialog({ onClose }: { onClose: () => void }) {
  const { position, file } = usePlayerStore();
  const overlayRef = useRef<HTMLDivElement>(null);

  const [startInput, setStartInput] = useState(() => formatTime(Math.floor(position)));
  const [endInput, setEndInput] = useState(() => formatTime(Math.floor(position + 10)));
  const [asGif, setAsGif] = useState(false);
  const [outputPath, setOutputPath] = useState("");
  const [status, setStatus] = useState<{ type: "idle" | "running" | "done" | "error"; message: string }>({ type: "idle", message: "" });

  useEffect(() => {
    if (!outputPath) {
      setOutputPath(`unflick-clip-${Date.now()}.${asGif ? "gif" : "mp4"}`);
    }
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleBrowse = async () => {
    try {
      const result = await invoke<{ path: string | null }>("save_file_dialog", { defaultName: `unflick-clip-${Date.now()}.${asGif ? "gif" : "mp4"}` });
      if (result.path) setOutputPath(result.path);
    } catch { /* ignore */ }
  };

  const handleExtract = async () => {
    const start = parseTimeInput(startInput);
    const end = parseTimeInput(endInput);
    if (isNaN(start) || isNaN(end)) { setStatus({ type: "error", message: "Invalid time format." }); return; }
    if (end <= start) { setStatus({ type: "error", message: "End must be after start." }); return; }
    if (!file) { setStatus({ type: "error", message: "No file loaded." }); return; }

    setStatus({ type: "running", message: "Extracting..." });
    try {
      const result = await invoke<{ output: string }>("extract_clip", { input: file, start, end, output: outputPath, asGif });
      setStatus({ type: "done", message: `Saved: ${result.output}` });
    } catch (err) {
      setStatus({ type: "error", message: String(err) });
    }
  };

  const inputClass = "w-full rounded-lg border border-white/6 bg-white/4 px-2.5 py-1.5 text-[11px] text-white/70 outline-none transition-colors focus:border-brand-purple/40 placeholder-white/15";

  return (
    <AnimatePresence>
      <motion.div
        ref={overlayRef}
        className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.15 }}
        onClick={(e) => { if (e.target === overlayRef.current) onClose(); }}
        onContextMenu={(e) => { if (e.target === overlayRef.current) e.preventDefault(); }}
      >
        <motion.div
          className="gradient-border w-80 max-h-[88vh] overflow-y-auto rounded-2xl p-5 shadow-2xl"
          style={{ background: "var(--bg-secondary, #111827)" }}
          initial={{ scale: 0.92, opacity: 0, y: 12 }}
          animate={{ scale: 1, opacity: 1, y: 0 }}
          exit={{ scale: 0.92, opacity: 0, y: 12 }}
          transition={{ duration: 0.2, ease: "easeOut" }}
        >
          <div className="mb-4 flex items-center justify-between">
            <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">Extract Clip</h2>
            <button className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50" onClick={onClose}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>

          {/* Time inputs */}
          <div className="mb-3 flex gap-3">
            <div className="flex-1">
              <label className="mb-1 block text-[10px] font-semibold uppercase tracking-widest text-white/20">Start</label>
              <input type="text" value={startInput} onChange={(e) => setStartInput(e.target.value)} placeholder="0:00" className={inputClass} />
            </div>
            <div className="flex-1">
              <label className="mb-1 block text-[10px] font-semibold uppercase tracking-widest text-white/20">End</label>
              <input type="text" value={endInput} onChange={(e) => setEndInput(e.target.value)} placeholder="0:10" className={inputClass} />
            </div>
          </div>

          {/* Output */}
          <div className="mb-3">
            <label className="mb-1 block text-[10px] font-semibold uppercase tracking-widest text-white/20">Output</label>
            <div className="flex gap-2">
              <input type="text" value={outputPath} onChange={(e) => setOutputPath(e.target.value)} className={`min-w-0 flex-1 ${inputClass}`} />
              <button className="flex-shrink-0 rounded-lg border border-white/6 bg-white/4 px-2.5 py-1.5 text-[11px] text-white/35 transition-colors hover:bg-white/8 hover:text-white/60" onClick={handleBrowse}>
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
              className={`relative h-5 w-9 rounded-full transition-colors ${asGif ? "bg-brand-purple" : "bg-white/10"}`}
            >
              <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${asGif ? "translate-x-4" : "translate-x-0.5"}`} />
            </button>
            <span className="text-[11px] text-white/35">Save as GIF</span>
          </div>

          {/* Status */}
          {status.message && (
            <div className={`mb-3 rounded-lg px-3 py-2 text-[11px] ${
              status.type === "error" ? "bg-red-500/10 text-red-400/80"
              : status.type === "done" ? "bg-green-500/10 text-green-400/80"
              : "bg-white/4 text-white/40"
            }`}>
              {status.message}
            </div>
          )}

          <button
            className="w-full rounded-xl py-2.5 text-[12px] font-semibold text-white transition-all hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
            style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
            onClick={handleExtract}
            disabled={status.type === "running"}
          >
            {status.type === "running" ? (
              <span className="flex items-center justify-center gap-2">
                <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-white border-t-transparent" />
                Extracting...
              </span>
            ) : status.type === "done" ? "Extract Another" : "Extract"}
          </button>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

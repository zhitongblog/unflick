import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

interface FilterValues {
  brightness: number;
  contrast: number;
  saturation: number;
  gamma: number;
  hue: number;
}

const DEFAULT_FILTERS: FilterValues = {
  brightness: 0, contrast: 0, saturation: 0, gamma: 0, hue: 0,
};

/** Picture geometry, as reported by `video_transform_get`. */
interface Geometry {
  /** "auto", or the override as a decimal string. */
  aspect: string;
  rotate: number;
  /** Plain multiplier: 1 is fit-to-window. */
  zoom: number;
  panscan: number;
  deinterlace: boolean;
}

const DEFAULT_GEOMETRY: Geometry = {
  aspect: "auto", rotate: 0, zoom: 1, panscan: 0, deinterlace: false,
};

/** Offered ratios. Anything else is still settable from the CLI. */
const ASPECT_CHOICES = ["auto", "16:9", "4:3", "21:9", "1.85:1", "2.35:1", "1:1"];

/**
 * Map the backend's decimal back onto a menu entry, so reopening the panel
 * shows "16:9" rather than "1.7778". Tolerance is wide enough to cover the
 * rounding in both directions.
 */
function aspectChoice(aspect: string): string {
  if (aspect === "auto") return "auto";
  const value = Number(aspect);
  if (!Number.isFinite(value)) return "auto";
  for (const choice of ASPECT_CHOICES) {
    if (choice === "auto") continue;
    const [w, h] = choice.split(":").map(Number);
    if (Math.abs(w / h - value) < 0.01) return choice;
  }
  return "auto";
}

const FILTER_LABELS: { key: keyof FilterValues; label: string }[] = [
  { key: "brightness", label: "Brightness" },
  { key: "contrast", label: "Contrast" },
  { key: "saturation", label: "Saturation" },
  { key: "gamma", label: "Gamma" },
  { key: "hue", label: "Hue" },
];

function FiltersIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <line x1="4" y1="6" x2="20" y2="6" />
      <line x1="4" y1="12" x2="20" y2="12" />
      <line x1="4" y1="18" x2="20" y2="18" />
      <circle cx="8" cy="6" r="2" fill="currentColor" stroke="none" />
      <circle cx="16" cy="12" r="2" fill="currentColor" stroke="none" />
      <circle cx="10" cy="18" r="2" fill="currentColor" stroke="none" />
    </svg>
  );
}

function FilterSlider({ label, filterKey, value, onChange }: {
  label: string;
  filterKey: keyof FilterValues;
  value: number;
  onChange: (key: keyof FilterValues, value: number) => void;
}) {
  const isActive = value !== 0;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className={`text-[11px] font-medium ${isActive ? "text-brand-purple" : "text-white/40"}`}>
          {label}
        </span>
        <span className={`text-[10px] tabular-nums font-medium ${isActive ? "text-brand-purple" : "text-white/20"}`}>
          {value > 0 ? `+${value}` : value}
        </span>
      </div>
      <input
        type="range"
        min="-100"
        max="100"
        value={value}
        onChange={(e) => onChange(filterKey, parseInt(e.target.value, 10))}
        className="h-1 w-full cursor-pointer rounded-full bg-white/10"
        style={{
          background: value !== 0
            ? `linear-gradient(to right, #7C3AED ${(value + 100) / 2}%, rgba(255,255,255,0.1) ${(value + 100) / 2}%)`
            : undefined,
        }}
      />
    </div>
  );
}

export default function VideoFilters({ disabled }: { disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  const [filters, setFilters] = useState<FilterValues>(DEFAULT_FILTERS);
  const [geometry, setGeometry] = useState<Geometry>(DEFAULT_GEOMETRY);
  const panelRef = useRef<HTMLDivElement>(null);
  const hasActive = Object.values(filters).some((v) => v !== 0);

  // Read geometry when the panel opens rather than on mount: it can be
  // changed from the CLI or by an agent, and this is the moment the user
  // is about to look at it.
  useEffect(() => {
    if (!open) return;
    invoke<Geometry>("video_transform_get").then(setGeometry).catch(() => {});
  }, [open]);

  useEffect(() => {
    if (!open) return;
    invoke<FilterValues>("get_video_filters")
      .then((vals) => setFilters(vals))
      .catch(() => setFilters(DEFAULT_FILTERS));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handleClick = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  // While the filter panel is open, push the mpv overlay window out of
  // the way so the panel isn't hidden behind it.
  useEffect(() => {
    if (!open) return;
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, [open]);

  const geometryActive =
    geometry.aspect !== "auto" ||
    geometry.rotate !== 0 ||
    Math.abs(geometry.zoom - 1) > 1e-6 ||
    geometry.panscan > 0 ||
    geometry.deinterlace;

  const handleChange = (key: keyof FilterValues, value: number) => {
    setFilters((prev) => ({ ...prev, [key]: value }));
    invoke("set_video_filter", { name: key, value }).catch(console.error);
  };

  const handleReset = () => {
    setFilters(DEFAULT_FILTERS);
    invoke("reset_video_filters").catch(console.error);
    setGeometry(DEFAULT_GEOMETRY);
    invoke("video_transform_reset").catch(console.error);
  };

  // Geometry — how the picture is fitted, as opposed to what colour it is.
  // Same popover because both answer "the picture looks wrong".
  const setTransform = (name: string, value: unknown) => {
    invoke<Geometry>("video_transform_set", { name, value })
      .then(setGeometry)
      .catch(console.error);
  };

  return (
    <div className="relative">
      <button
        className={`rounded-lg p-1.5 transition-all duration-150 ${
          open || hasActive || geometryActive ? "text-brand-purple" : "text-white/35 hover:text-white/70 hover:bg-white/6"
        } active:scale-90`}
        onClick={() => setOpen((v) => !v)}
        title="Video Filters"
        disabled={disabled}
      >
        <FiltersIcon />
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            ref={panelRef}
            initial={{ opacity: 0, y: 8, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.95 }}
            transition={{ duration: 0.12 }}
            className="glass-elevated absolute bottom-full right-0 mb-2 w-60 rounded-xl p-4 shadow-2xl"
          >
            <div className="mb-3 flex items-center justify-between">
              <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
                Video Filters
              </p>
              {hasActive && (
                <button
                  className="text-[10px] font-medium text-white/30 transition-colors hover:text-brand-pink"
                  onClick={handleReset}
                >
                  Reset
                </button>
              )}
            </div>
            <div className="flex flex-col gap-3">
              {FILTER_LABELS.map(({ key, label }) => (
                <FilterSlider key={key} label={label} filterKey={key} value={filters[key]} onChange={handleChange} />
              ))}
            </div>

            <div className="my-3 border-t border-white/6" />

            <p className="mb-2 text-[10px] font-semibold uppercase tracking-widest text-white/25">
              Geometry
            </p>

            <div className="flex flex-col gap-2.5">
              <div className="flex items-center gap-2">
                <span className="w-14 flex-shrink-0 text-[10px] text-white/40">Aspect</span>
                <select
                  value={aspectChoice(geometry.aspect)}
                  onChange={(e) => setTransform("aspect", e.target.value)}
                  className="min-w-0 flex-1 rounded-md border border-white/10 bg-[#1c1c26] px-1.5 py-1 text-[11px] text-white/80 outline-none focus:border-brand-purple/40"
                >
                  {ASPECT_CHOICES.map((a) => (
                    <option key={a} value={a} style={{ background: "#1c1c26", color: "#ffffff" }}>
                      {a}
                    </option>
                  ))}
                </select>
              </div>

              <div className="flex items-center gap-2">
                <span className="w-14 flex-shrink-0 text-[10px] text-white/40">Rotate</span>
                <div className="flex flex-1 gap-1">
                  {[0, 90, 180, 270].map((deg) => (
                    <button
                      key={deg}
                      onClick={() => setTransform("rotate", deg)}
                      className={`flex-1 rounded-md border px-1 py-1 text-[10px] tabular-nums transition-all ${
                        geometry.rotate === deg
                          ? "border-brand-purple/40 bg-brand-purple/15 text-white"
                          : "border-white/10 bg-white/4 text-white/50 hover:text-white/80"
                      }`}
                    >
                      {deg}°
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex items-center gap-2">
                <span className="w-14 flex-shrink-0 text-[10px] text-white/40">Zoom</span>
                <input
                  type="range"
                  min={0.5}
                  max={3}
                  step={0.05}
                  value={geometry.zoom}
                  onChange={(e) => setTransform("zoom", Number(e.target.value))}
                  className="h-1 flex-1 cursor-pointer appearance-none rounded-full bg-white/10 accent-brand-purple"
                />
                <span className="w-9 flex-shrink-0 text-right text-[10px] tabular-nums text-white/35">
                  {geometry.zoom.toFixed(2)}×
                </span>
              </div>

              <button
                onClick={() => setTransform("deinterlace", !geometry.deinterlace)}
                className={`rounded-md border px-2 py-1 text-[10px] font-medium transition-all ${
                  geometry.deinterlace
                    ? "border-brand-purple/40 bg-brand-purple/15 text-white"
                    : "border-white/10 bg-white/4 text-white/50 hover:text-white/80"
                }`}
              >
                Deinterlace
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

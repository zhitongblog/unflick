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
  const panelRef = useRef<HTMLDivElement>(null);
  const hasActive = Object.values(filters).some((v) => v !== 0);

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

  const handleChange = (key: keyof FilterValues, value: number) => {
    setFilters((prev) => ({ ...prev, [key]: value }));
    invoke("set_video_filter", { name: key, value }).catch(console.error);
  };

  const handleReset = () => {
    setFilters(DEFAULT_FILTERS);
    invoke("reset_video_filters").catch(console.error);
  };

  return (
    <div className="relative">
      <button
        className={`rounded-lg p-1.5 transition-all duration-150 ${
          open || hasActive ? "text-brand-purple" : "text-white/35 hover:text-white/70 hover:bg-white/6"
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
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

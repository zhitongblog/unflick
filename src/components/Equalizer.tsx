import { useState, useEffect, useRef, useCallback } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

type AudioState = {
  enabled: boolean;
  bands: number[];
  frequencies: number[];
  preamp: number;
  normalize: boolean;
  flat: boolean;
  max_gain: number;
  pitch_correction: boolean;
};

type Preset = { name: string; description: string; bands: number[] };

/**
 * How long to sit on a slider value before sending it.
 *
 * Every change rebuilds mpv's audio filter chain — there is no cheaper live
 * path on the libmpv we bundle (see `core::audio`). Without this, one drag
 * across a slider fires a rebuild per animation frame. 120 ms is short enough
 * to feel immediate and long enough that a drag sends once, at the end.
 */
const APPLY_DEBOUNCE_MS = 120;

function formatFreq(hz: number): string {
  return hz >= 1000 ? `${hz / 1000}k` : `${hz}`;
}

export default function Equalizer({ onClose }: { onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<AudioState | null>(null);
  const [presets, setPresets] = useState<Preset[]>([]);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  // Move the mpv overlay out of the way, or this renders behind the video.
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, []);

  useEffect(() => {
    invoke<AudioState>("equalizer_get").then(setState).catch((e) => setError(String(e)));
    invoke<Preset[]>("equalizer_presets").then(setPresets).catch(() => undefined);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, []);

  const apply = useCallback(async (args: Record<string, unknown>) => {
    try {
      setState(await invoke<AudioState>("equalizer_set", args));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** Update the slider immediately, send the value once the drag settles. */
  const dragBand = (index: number, gain: number) => {
    setState((prev) => {
      if (!prev) return prev;
      const bands = [...prev.bands];
      bands[index] = gain;
      return { ...prev, bands, flat: bands.every((g) => g === 0) };
    });
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      // Sending the whole curve rather than one band keeps the backend in
      // step even if two sliders were moved inside one debounce window.
      setState((cur) => {
        if (cur) void apply({ bands: cur.bands, enabled: true });
        return cur;
      });
    }, APPLY_DEBOUNCE_MS);
  };

  if (!state) {
    return (
      <motion.div
        ref={menuRef}
        initial={{ opacity: 0, y: 8, scale: 0.95 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        className="glass-elevated absolute bottom-full right-0 mb-2 w-[320px] rounded-xl p-4 shadow-2xl"
      >
        <p className="text-[11px] text-white/25">{error ?? "Loading…"}</p>
      </motion.div>
    );
  }

  const max = state.max_gain;

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, y: 8, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.95 }}
      transition={{ duration: 0.12 }}
      className="glass-elevated absolute bottom-full right-0 mb-2 w-[340px] rounded-xl p-4 shadow-2xl"
    >
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
          Equalizer
        </p>
        <button
          type="button"
          onClick={() => apply({ enabled: !state.enabled })}
          className={`relative h-5 w-9 rounded-full transition-colors ${
            state.enabled ? "bg-brand-purple" : "bg-white/10"
          }`}
          title={state.enabled ? "Bypass" : "Enable"}
        >
          <span
            className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
              state.enabled ? "translate-x-[18px]" : "translate-x-0.5"
            }`}
          />
        </button>
      </div>

      {/* Sliders. Vertical, because that is the shape of an equaliser curve
          and reading one horizontally means reading it sideways. */}
      <div
        className={`flex items-end justify-between gap-1 transition-opacity ${
          state.enabled ? "" : "opacity-40"
        }`}
      >
        {state.bands.map((gain, i) => (
          <div key={i} className="flex flex-1 flex-col items-center gap-1">
            <span className="text-[9px] tabular-nums text-white/30">
              {gain > 0 ? `+${gain}` : gain}
            </span>
            <input
              type="range"
              min={-max}
              max={max}
              step={0.5}
              value={gain}
              onChange={(e) => dragBand(i, Number(e.target.value))}
              onDoubleClick={() => dragBand(i, 0)}
              title={`${state.frequencies[i]} Hz — double-click to reset`}
              className="eq-slider h-24"
              style={{ writingMode: "vertical-lr", direction: "rtl" }}
            />
            <span className="text-[9px] tabular-nums text-white/25">
              {formatFreq(state.frequencies[i])}
            </span>
          </div>
        ))}
      </div>

      <div className="mx-1 my-3 border-t border-white/6" />

      <div className="space-y-2.5">
        <div className="flex items-center gap-2">
          <span className="w-16 text-[11px] text-white/50">Preset</span>
          <select
            value=""
            onChange={async (e) => {
              const name = e.target.value;
              if (!name) return;
              try {
                setState(await invoke<AudioState>("equalizer_preset", { name }));
                setError(null);
              } catch (err) {
                setError(String(err));
              }
            }}
            className="flex-1 rounded-lg bg-white/5 px-2 py-1 text-[11px] text-white/80 outline-none ring-1 ring-white/8"
          >
            <option value="">Choose…</option>
            {presets.map((p) => (
              <option key={p.name} value={p.name} title={p.description}>
                {p.name} — {p.description}
              </option>
            ))}
          </select>
        </div>

        <div className="flex items-center gap-2">
          <span className="w-16 text-[11px] text-white/50" title="Headroom for boosted bands">
            Preamp
          </span>
          <input
            type="range"
            min={-20}
            max={12}
            step={0.5}
            value={state.preamp}
            onChange={(e) => {
              const preamp = Number(e.target.value);
              setState((prev) => (prev ? { ...prev, preamp } : prev));
              if (timer.current) window.clearTimeout(timer.current);
              timer.current = window.setTimeout(() => void apply({ preamp }), APPLY_DEBOUNCE_MS);
            }}
            className="flex-1"
          />
          <span className="w-12 text-right text-[10px] tabular-nums text-white/40">
            {state.preamp > 0 ? `+${state.preamp}` : state.preamp} dB
          </span>
        </div>

        <label className="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            checked={state.normalize}
            onChange={(e) => apply({ normalize: e.target.checked })}
            className="accent-brand-purple"
          />
          <span className="text-[11px] text-white/60">Normalize loudness</span>
          <span className="ml-auto text-[9px] text-white/20">quiet dialogue vs. loud action</span>
        </label>

        <label className="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            checked={state.pitch_correction}
            onChange={async (e) => {
              const enabled = e.target.checked;
              try {
                await invoke("pitch_correction", { enabled });
                setState((prev) => (prev ? { ...prev, pitch_correction: enabled } : prev));
              } catch (err) {
                setError(String(err));
              }
            }}
            className="accent-brand-purple"
          />
          <span className="text-[11px] text-white/60">Keep pitch when speeding up</span>
        </label>
      </div>

      {error && (
        <p className="mt-2 rounded-lg bg-red-500/10 px-2 py-1.5 text-[10px] leading-relaxed text-red-300/90">
          {error}
        </p>
      )}

      <button
        className="mt-3 w-full rounded-lg py-1.5 text-[11px] text-white/40 transition-colors hover:bg-white/6 hover:text-white/70"
        onClick={async () => {
          try {
            setState(await invoke<AudioState>("equalizer_reset"));
            setError(null);
          } catch (e) {
            setError(String(e));
          }
        }}
      >
        Reset all
      </button>
    </motion.div>
  );
}

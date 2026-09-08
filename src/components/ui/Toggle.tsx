// One switch, drawn one way.
//
// The knob is absolutely positioned inside the track, and it needs an
// explicit `left` to sit where it looks like it should. Without one the
// browser falls back to the knob's *static* position — and because a
// `<button>` centres its content, that put the knob halfway across the
// track when off and completely outside it when on. Anchoring at
// `left-0.5` and moving with a transform keeps the maths honest: the
// track is 36 px, the knob 16 px, so 2 px + 16 px of travel lands it
// 2 px from the right edge.

type TrackProps = {
  checked: boolean;
  /** Extra classes for the track — spacing, mostly. */
  className?: string;
};

/** The switch itself, with no click target of its own. Use this when the
 *  surrounding row is already a button (a whole-row setting toggle). */
export function ToggleTrack({ checked, className = "" }: TrackProps) {
  return (
    <span
      className={`relative block h-5 w-9 flex-shrink-0 rounded-full transition-colors ${
        checked ? "bg-brand-purple" : "bg-white/10"
      } ${className}`}
    >
      <span
        className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
          checked ? "translate-x-4" : "translate-x-0"
        }`}
      />
    </span>
  );
}

type ToggleProps = {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** Accessible name. The visible label usually sits beside the switch. */
  label?: string;
  title?: string;
  className?: string;
};

/** A switch that *is* the button. */
export default function Toggle({
  checked,
  onChange,
  label,
  title,
  className = "",
}: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={title}
      onClick={() => onChange(!checked)}
      className={`flex-shrink-0 ${className}`}
    >
      <ToggleTrack checked={checked} />
    </button>
  );
}

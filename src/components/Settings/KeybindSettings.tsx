import { useEffect, useRef, useState } from "react";
import { useKeybindStore, type Binding } from "../../stores/keybindStore";
import { useStrings } from "../../i18n/utils";
import { eventToKey, formatKey } from "../../lib/keys";

/**
 * The shortcut editor.
 *
 * Clicking a row arms capture: the next chord typed becomes the binding.
 * Capture is modal on purpose — offering a text field to type `Mod+Shift+p`
 * into would ask people to know a syntax, when pressing the keys is the
 * thing they already know how to do.
 */
export default function KeybindSettings() {
  const { bindings, loaded, load, setBinding, reset } = useKeybindStore();
  const t = useStrings();

  /** Action currently listening for a chord, if any. */
  const [capturing, setCapturing] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const capturingRef = useRef<string | null>(null);
  capturingRef.current = capturing;

  useEffect(() => {
    if (!loaded) void load();
  }, [loaded, load]);

  // Capture runs on the window during the capture phase so it beats the
  // app's own shortcut handler — otherwise binding `f` would toggle
  // fullscreen on the way to being recorded.
  useEffect(() => {
    if (!capturing) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        setCapturing(null);
        setConflict(null);
        return;
      }

      const chord = eventToKey(e);
      if (!chord) return; // a bare modifier — keep waiting for the real key

      const action = capturingRef.current;
      if (!action) return;
      setCapturing(null);

      void setBinding(action, chord).then((error) => setConflict(error));
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [capturing, setBinding]);

  // Preserve the catalogue's order within each group — it's arranged by
  // how related the actions are, which alphabetising would destroy.
  const groups: { id: string; rows: Binding[] }[] = [];
  for (const b of bindings) {
    const existing = groups.find((g) => g.id === b.group);
    if (existing) existing.rows.push(b);
    else groups.push({ id: b.group, rows: [b] });
  }

  const groupLabel = (id: string) =>
    (t.keybinds.groups as Record<string, string>)[id] ?? id;

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
          {t.keybinds.section}
        </p>
        <button
          className="rounded-lg px-2 py-1 text-[10px] font-medium text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
          onClick={() => {
            setConflict(null);
            void reset();
          }}
        >
          {t.keybinds.resetAll}
        </button>
      </div>

      <p className="mb-3 text-[11px] leading-relaxed text-white/45">{t.keybinds.hint}</p>

      {conflict && (
        <p className="mb-3 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] leading-relaxed text-red-300">
          {conflict}
        </p>
      )}

      <div className="space-y-4">
        {groups.map((group) => (
          <div key={group.id}>
            <p className="mb-1.5 text-[10px] font-medium uppercase tracking-wider text-white/20">
              {groupLabel(group.id)}
            </p>
            <div className="overflow-hidden rounded-lg border border-white/6">
              {group.rows.map((b, i) => (
                <div
                  key={b.id}
                  className={`flex items-center gap-3 px-3 py-1.5 ${
                    i > 0 ? "border-t border-white/4" : ""
                  }`}
                >
                  <span className="flex-1 truncate text-[11px] text-white/70">
                    {(t.keybinds as Record<string, unknown>)[b.id] as string | undefined ?? b.label}
                  </span>

                  {b.customized && (
                    <button
                      className="rounded px-1.5 py-0.5 text-[9px] text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
                      title={`${t.keybinds.reset} → ${formatKey(b.default)}`}
                      onClick={() => {
                        setConflict(null);
                        void reset(b.id);
                      }}
                    >
                      {t.keybinds.changed}
                    </button>
                  )}

                  <button
                    className={`min-w-[5.5rem] rounded-md border px-2 py-1 text-center text-[11px] font-medium tabular-nums transition-all ${
                      capturing === b.id
                        ? "animate-pulse border-brand-purple/60 bg-brand-purple/15 text-white"
                        : "border-white/10 bg-white/4 text-white/70 hover:border-white/25 hover:text-white"
                    }`}
                    onClick={() => {
                      setConflict(null);
                      setCapturing(capturing === b.id ? null : b.id);
                    }}
                  >
                    {capturing === b.id ? t.keybinds.press : formatKey(b.key)}
                  </button>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

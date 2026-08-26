import { useEffect } from "react";
import { useMousebindStore } from "../../stores/mousebindStore";
import { useKeybindStore } from "../../stores/keybindStore";
import { useStrings } from "../../i18n/utils";

/**
 * Mouse binding editor.
 *
 * A dropdown per trigger rather than the keyboard panel's press-to-capture:
 * there's no way to "press" a drag gesture at a settings row, and the
 * trigger set is fixed anyway. Choosing from the action list is both
 * simpler and more discoverable — it doubles as a list of what the player
 * can do.
 */
export default function MouseSettings() {
  const { bindings, loaded, load, setBinding, reset } = useMousebindStore();
  // The action catalogue comes from the keyboard store: one list of
  // actions, two ways to trigger them.
  const { bindings: actions, loaded: actionsLoaded, load: loadActions } =
    useKeybindStore();
  const t = useStrings();

  useEffect(() => {
    if (!loaded) void load();
    if (!actionsLoaded) void loadActions();
  }, [loaded, load, actionsLoaded, loadActions]);

  const actionLabel = (id: string) =>
    (t.keybinds as Record<string, unknown>)[id] as string | undefined ??
    actions.find((a) => a.id === id)?.label ??
    id;

  const triggerLabel = (id: string) =>
    (t.mouse.triggers as Record<string, string>)[id] ?? id;

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
          {t.mouse.section}
        </p>
        <button
          className="rounded-lg px-2 py-1 text-[10px] font-medium text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
          onClick={() => void reset()}
        >
          {t.keybinds.resetAll}
        </button>
      </div>

      <p className="mb-3 text-[11px] leading-relaxed text-white/45">{t.mouse.hint}</p>

      <div className="overflow-hidden rounded-lg border border-white/6">
        {bindings.map((b, i) => (
          <div
            key={b.id}
            className={`flex items-center gap-3 px-3 py-1.5 ${
              i > 0 ? "border-t border-white/4" : ""
            }`}
          >
            <span className="flex-1 truncate text-[11px] text-white/70">
              {triggerLabel(b.id)}
            </span>

            {b.customized && (
              <button
                className="rounded px-1.5 py-0.5 text-[9px] text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
                title={t.keybinds.reset}
                onClick={() => void reset(b.id)}
              >
                {t.keybinds.changed}
              </button>
            )}

            <select
              value={b.action}
              onChange={(e) => void setBinding(b.id, e.target.value)}
              className="w-[9.5rem] rounded-md border border-white/10 bg-[#1c1c26] px-2 py-1 text-[11px] text-white/80 outline-none transition-colors focus:border-brand-purple/40"
            >
              {/* WebView2 doesn't inherit Tailwind colours into <option>,
                  so each one sets them explicitly or the list renders
                  white-on-white. */}
              <option value="none" style={{ background: "#1c1c26", color: "#ffffff" }}>
                {t.mouse.none}
              </option>
              {actions.map((a) => (
                <option key={a.id} value={a.id} style={{ background: "#1c1c26", color: "#ffffff" }}>
                  {actionLabel(a.id)}
                </option>
              ))}
            </select>
          </div>
        ))}
      </div>
    </div>
  );
}

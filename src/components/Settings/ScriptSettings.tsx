import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStrings } from "../../i18n/utils";
import Toggle from "../ui/Toggle";

interface UserScript {
  name: string;
  path: string;
  kind: string;
  enabled: boolean;
  size: number;
  description: string | null;
}

export default function ScriptSettings() {
  const t = useStrings();
  const [scripts, setScripts] = useState<UserScript[]>([]);

  const refresh = useCallback(async () => {
    try {
      const res = await invoke<UserScript[] | { scripts?: UserScript[] }>("script_list");
      setScripts(Array.isArray(res) ? res : (res?.scripts ?? []));
    } catch {
      setScripts([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleAdd = async () => {
    try {
      const picked = await invoke<{ path: string | null }>("open_script_dialog");
      if (!picked.path) return;
      await invoke<UserScript>("script_add", { path: picked.path });
      await refresh();
    } catch {
      // Ignored; refresh reflects current disk state.
    }
  };

  const handleToggle = async (name: string, enabled: boolean) => {
    try {
      await invoke<UserScript>("script_toggle", { name, enabled });
      await refresh();
    } catch {
      // Ignored.
    }
  };

  const handleRemove = async (name: string) => {
    try {
      await invoke("script_remove", { name });
      await refresh();
    } catch {
      // Ignored.
    }
  };

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-white/25">
          {t.scripts.section}
        </p>
        <button
          onClick={handleAdd}
          className="rounded-lg border border-white/10 bg-white/4 px-2.5 py-1 text-[10px] font-medium text-white/70 transition hover:border-white/20 hover:text-white"
        >
          {t.scripts.add}
        </button>
      </div>

      {scripts.length === 0 ? (
        <p className="rounded-xl border border-white/6 bg-white/3 px-3 py-3 text-[11px] text-white/30">
          {t.scripts.empty}
        </p>
      ) : (
        <div className="space-y-1.5">
          {scripts.map((s) => (
            <div
              key={s.name}
              className="flex items-center justify-between gap-3 rounded-xl border border-white/6 bg-white/3 px-3 py-2"
            >
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-[11px] font-medium text-white/75">
                  {s.name}
                </p>
                {s.description && (
                  <p className="truncate text-[10px] text-white/35">
                    {s.description}
                  </p>
                )}
              </div>
              <Toggle
                checked={s.enabled}
                onChange={(next) => void handleToggle(s.name, next)}
                label={s.name}
              />
              <button
                onClick={() => void handleRemove(s.name)}
                title={t.scripts.remove}
                aria-label={t.scripts.remove}
                className="rounded p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-red-300"
              >
                <svg
                  width="13"
                  height="13"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>
          ))}
        </div>
      )}

      <p className="mt-2 text-[10px] leading-relaxed text-white/30">
        {t.scripts.hint}
      </p>
    </div>
  );
}

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useIncognitoStore } from "../stores/incognitoStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";

interface RecentEntry {
  path: string;
  title: string;
  duration: number | null;
  last_played: string | null;
  play_count: number;
}

/**
 * Recently played files, shown on the drop zone.
 *
 * This is where someone is already looking when they want to reopen
 * something, so it belongs here rather than behind a menu. Hidden while
 * incognito is on: surfacing a history in the mode whose whole point is
 * not leaving one would defeat it.
 */
export default function RecentFiles({ limit = 6 }: { limit?: number }) {
  const [entries, setEntries] = useState<RecentEntry[]>([]);
  const play = usePlayerStore((s) => s.play);
  const incognito = useIncognitoStore((s) => s.enabled);
  const t = useStrings();

  useEffect(() => {
    if (incognito) {
      setEntries([]);
      return;
    }
    invoke<RecentEntry[]>("recent_list", { limit })
      .then(setEntries)
      .catch(() => setEntries([]));
  }, [limit, incognito]);

  if (incognito || entries.length === 0) return null;

  return (
    <div className="idle-fade-in-delay-2 mt-8 w-full max-w-md">
      <div className="mb-2 flex items-center justify-between px-1">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
          {t.recent.title}
        </p>
        <button
          className="rounded px-1.5 py-0.5 text-[10px] text-white/15 transition-colors hover:bg-white/6 hover:text-white/40"
          onClick={() => {
            void invoke("recent_clear").then(() => setEntries([]));
          }}
        >
          {t.recent.clear}
        </button>
      </div>

      <div className="flex flex-col gap-0.5">
        {entries.map((e) => (
          <button
            key={e.path}
            onClick={() => play(e.path)}
            title={e.path}
            className="group flex items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors hover:bg-white/6"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="currentColor"
              className="flex-shrink-0 text-white/15 group-hover:text-brand-purple"
            >
              <path d="M8 5v14l11-7z" />
            </svg>
            <span className="min-w-0 flex-1 truncate text-[12px] text-white/45 group-hover:text-white/80">
              {e.title}
            </span>
            {e.duration != null && e.duration > 0 && (
              <span className="flex-shrink-0 text-[10px] tabular-nums text-white/15">
                {formatTime(e.duration)}
              </span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

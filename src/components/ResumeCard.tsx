import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useIncognitoStore } from "../stores/incognitoStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";

interface Session {
  path: string;
  position: number;
  duration: number;
  updated_at: string;
}

/**
 * "Pick up where you left off", on the drop zone.
 *
 * The recent list below it can already reopen the file, and reopening
 * applies the resume point — so this is not a second way to do the same
 * thing. What it adds is the part the recent list cannot say: that this
 * one is unfinished, and how far in. Without it, someone who closed the
 * window forty minutes into a film comes back to a filename that looks
 * exactly like the six above it.
 *
 * Only ever one entry. A list of half-watched things is a different
 * feature, and a worse home screen.
 */
export default function ResumeCard() {
  const [session, setSession] = useState<Session | null>(null);
  const play = usePlayerStore((s) => s.play);
  const incognito = useIncognitoStore((s) => s.enabled);
  const t = useStrings();

  useEffect(() => {
    if (incognito) {
      setSession(null);
      return;
    }
    invoke<Session | null>("session_get")
      .then(setSession)
      .catch(() => setSession(null));
  }, [incognito]);

  if (incognito || !session) return null;

  const name = session.path.split(/[\\/]/).pop() ?? session.path;
  const title = name.replace(/\.[^.]+$/, "");
  const pct =
    session.duration > 0
      ? Math.min(100, Math.max(0, (session.position / session.duration) * 100))
      : 0;

  return (
    <div className="idle-fade-in-delay-1 mt-8 w-full max-w-md">
      <p className="mb-2 px-1 text-[10px] font-semibold uppercase tracking-widest text-white/20">
        {t.resume.title}
      </p>
      <div className="group rounded-lg px-3 py-2 transition-colors hover:bg-white/6">
        <div className="flex items-center gap-3">
          <button
            onClick={() => play(session.path)}
            title={session.path}
            className="flex min-w-0 flex-1 items-center gap-3 text-left"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="currentColor"
              className="flex-shrink-0 text-brand-purple"
            >
              <path d="M8 5v14l11-7z" />
            </svg>
            <span className="min-w-0 flex-1 truncate text-[12px] text-white/60 group-hover:text-white/90">
              {title}
            </span>
            <span className="flex-shrink-0 text-[10px] tabular-nums text-white/25">
              {formatTime(session.position)}
            </span>
          </button>
        {/* Dismiss, because the offer should not outlive wanting it. Only
            clears the session — the resume point stays, so opening the file
            any other way still lands in the right place. */}
          <button
            onClick={() => {
              void invoke("session_clear").then(() => setSession(null));
            }}
            title={t.resume.dismiss}
            aria-label={t.resume.dismiss}
            className="flex-shrink-0 rounded px-1.5 py-0.5 text-[11px] leading-none text-white/15 opacity-0 transition-all hover:bg-white/6 hover:text-white/50 focus-visible:opacity-100 group-hover:opacity-100"
          >
            ✕
          </button>
        </div>
        {/* The track has to be visible or the fill reads as an underline on
            the title rather than as progress — which is exactly how the
            first version looked on screen. */}
        {session.duration > 0 && (
          <span className="mt-2 block h-[3px] w-full overflow-hidden rounded-full bg-white/15">
            <span
              className="block h-full rounded-full bg-brand-purple"
              style={{ width: `${pct}%` }}
            />
          </span>
        )}
      </div>
    </div>
  );
}

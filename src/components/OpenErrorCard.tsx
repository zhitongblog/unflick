import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import {
  classifyOpenError,
  detectPlatform,
  hintKeyFor,
  titleKeyFor,
} from "../lib/openError";

/**
 * A failed open, said out loud and left on screen.
 *
 * The toast that already fires for this auto-dismisses; a path that could
 * not open was gone in two and a half seconds and could be neither re-read
 * nor copied. This is the durable half: what happened, what to do about it,
 * and the backend's own words behind a disclosure for when the answer is
 * "send this to someone".
 */
export default function OpenErrorCard({ onOpenFile }: { onOpenFile: () => void }) {
  const openError = usePlayerStore((s) => s.openError);
  const target = usePlayerStore((s) => s.openErrorTarget);
  const clearOpenError = usePlayerStore((s) => s.clearOpenError);
  const t = useStrings();

  if (!openError) return null;

  const classified = classifyOpenError(target, openError);
  const platform = detectPlatform(
    typeof navigator === "undefined" ? undefined : navigator.userAgent,
  );
  const strings = t.openError as unknown as Record<string, string>;
  const title = (strings[titleKeyFor(classified)] ?? t.openError.titleUnreadable).replace(
    "{scheme}",
    classified.scheme ?? "",
  );
  const hint = strings[hintKeyFor(classified, platform)] ?? t.openError.hintUnreadable;

  return (
    <div className="idle-fade-in mt-8 w-full max-w-md">
      <div className="rounded-xl border border-red-500/20 bg-red-500/6 px-4 py-3 text-left">
        <div className="flex items-start gap-2">
          <p className="min-w-0 flex-1 text-[12px] font-semibold text-red-200/90">{title}</p>
          <button
            onClick={clearOpenError}
            title={t.openError.dismiss}
            aria-label={t.openError.dismiss}
            className="flex-shrink-0 rounded px-1.5 py-0.5 text-[11px] leading-none text-white/25 transition-colors hover:bg-white/6 hover:text-white/60"
          >
            ✕
          </button>
        </div>

        {target && (
          <p className="mt-1 truncate font-mono text-[10.5px] text-white/30" title={target}>
            {target}
          </p>
        )}

        <p className="mt-2 text-[11px] leading-relaxed text-white/50">{hint}</p>

        <div className="mt-3 flex items-center gap-3">
          <button
            onClick={() => {
              clearOpenError();
              onOpenFile();
            }}
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[11px] font-medium text-white/70 transition-colors hover:border-white/20 hover:bg-white/10 hover:text-white"
          >
            {t.openError.openAnother}
          </button>
          <details className="group min-w-0 flex-1">
            <summary className="cursor-pointer select-none list-none text-[10.5px] text-white/25 transition-colors hover:text-white/50">
              {t.openError.details}
            </summary>
            <p className="mt-1.5 select-text break-words rounded-lg border border-white/6 bg-black/30 px-2.5 py-2 font-mono text-[10px] leading-relaxed text-white/40">
              {classified.detail}
            </p>
          </details>
        </div>
      </div>
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { useKeybindStore } from "../../stores/keybindStore";
import { useStrings } from "../../i18n/utils";
import { formatKey } from "../../lib/keys";

/**
 * The one screen a fresh install gets.
 *
 * Everything here is chosen against a single question: what does someone
 * need in the first ten seconds that they would otherwise never find? Two
 * things. That the window takes a dropped file (obvious once said, invisible
 * until then), and that a CLI and an MCP server ship with the player — the
 * one thing about unflick nobody discovers on their own, and the reason the
 * plugin one-liner is on this screen rather than in a README.
 *
 * Shown once. Every way out of it marks the flag, and nothing brings it back
 * except asking for it in Settings or `unflick settings set onboarding_seen
 * false`.
 */

/** Kept out of i18n on purpose: a translator cannot improve a command, only break it. */
const CLAUDE_LINES = ["/plugin marketplace add zhitongblog/unflick", "/plugin install unflick"];
const CLI_SAMPLE = "unflick play <file>";

/**
 * The logo mark, inlined.
 *
 * There is no `public/` directory in this project, so `logo/unflick-logo.svg`
 * has no URL the WebView can fetch — the favicon link in index.html is a 404
 * for the same reason. The wordmark from the file is dropped: it is a
 * `<text>` element in a font that may not exist, and at 44px it is a smudge.
 */
function Mark() {
  return (
    <svg width="44" height="44" viewBox="0 0 512 512" aria-hidden="true">
      <defs>
        {/* Namespaced: a bare id="bg" would collide with any other inline SVG. */}
        <linearGradient id="unflick-mark-grad" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#7C3AED" />
          <stop offset="100%" stopColor="#DB2777" />
        </linearGradient>
      </defs>
      <rect x="36" y="36" width="440" height="440" rx="96" ry="96" fill="url(#unflick-mark-grad)" />
      {[80, 118, 156, 194, 232, 270].map((y) => (
        <g key={y}>
          <rect x="68" y={y} width="24" height="16" rx="4" fill="rgba(255,255,255,0.18)" />
          <rect x="420" y={y} width="24" height="16" rx="4" fill="rgba(255,255,255,0.18)" />
        </g>
      ))}
      <polygon points="210,92 210,296 368,194" fill="white" opacity="0.95" />
    </svg>
  );
}

function Keycap({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="rounded-md border border-white/12 bg-white/6 px-2 py-1 text-[11px] font-semibold text-white/70">
      {children}
    </kbd>
  );
}

export default function WelcomeCard({
  onOpenFile,
  onDismiss,
}: {
  /** Opens the file picker, then closes this. */
  onOpenFile: () => void;
  /** Marks the flag and closes. Every exit goes through it. */
  onDismiss: () => void;
}) {
  const t = useStrings();
  const reduceMotion = useReducedMotion();
  const bindings = useKeybindStore((s) => s.bindings);
  const overlayRef = useRef<HTMLDivElement>(null);
  const codeRef = useRef<HTMLPreElement>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "manual">("idle");

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onDismiss();
      }
    };
    // Capture, so the app's global key handler doesn't act on Esc as well.
    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [onDismiss]);

  /**
   * The live binding for an action, or the shipped default while the table
   * is still loading. Hardcoding Space / → / F would be a lie to anyone who
   * has already rebound them, and `formatKey` is what puts ⌘ on macOS.
   */
  const keyFor = (id: string, fallback: string) =>
    formatKey(bindings.find((b) => b.id === id)?.key ?? fallback);

  /**
   * Copy the two lines, or say plainly that we could not.
   *
   * No clipboard plugin ships with this build and adding one is a change to
   * src-tauri, so this is `navigator.clipboard` or nothing. Two things it
   * must never do: claim "Copied" for a write that did not happen, and sit
   * there doing nothing. The second one is not hypothetical — in a WebView
   * where the page is not the active clipboard owner, `writeText` returns a
   * promise that neither resolves nor rejects, and the button is dead. So
   * the write is raced against a short deadline, and anything other than a
   * resolved write falls back to selecting the block and saying so.
   */
  const copy = async () => {
    const text = CLAUDE_LINES.join("\n");
    const wrote = await Promise.race([
      navigator.clipboard
        ? navigator.clipboard.writeText(text).then(
            () => true,
            () => false,
          )
        : Promise.resolve(false),
      new Promise<boolean>((resolve) => setTimeout(() => resolve(false), 400)),
    ]);

    if (wrote) {
      setCopyState("copied");
      setTimeout(() => setCopyState("idle"), 1600);
      return;
    }

    const node = codeRef.current;
    if (node) {
      const range = document.createRange();
      range.selectNodeContents(node);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
    setCopyState("manual");
  };

  const duration = reduceMotion ? 0 : 0.18;

  return (
    <motion.div
      ref={overlayRef}
      className="fixed inset-0 z-[120] flex items-center justify-center bg-black/75 p-6 backdrop-blur-sm"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration }}
      onClick={(e) => {
        if (e.target === overlayRef.current) onDismiss();
      }}
    >
      <motion.div
        role="dialog"
        aria-modal="true"
        aria-label={t.onboarding.tagline}
        className="gradient-border max-h-full w-[520px] overflow-y-auto rounded-2xl p-6 shadow-2xl"
        style={{ background: "var(--bg-secondary, #111827)" }}
        initial={{ opacity: 0, scale: reduceMotion ? 1 : 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration, ease: "easeOut" }}
      >
        <div className="flex items-center gap-3">
          <Mark />
          <div className="min-w-0">
            <h1 className="idle-title text-2xl font-extrabold leading-none">unflick</h1>
            <p className="mt-1 text-[11px] text-white/35">{t.onboarding.tagline}</p>
          </div>
        </div>

        <p className="mt-4 text-[12px] leading-relaxed text-white/60">{t.onboarding.what}</p>

        <div className="mt-5 flex flex-col items-start gap-2">
          <button
            onClick={onOpenFile}
            className="idle-open-btn rounded-xl px-6 py-2.5 text-[12px] font-semibold text-white transition-all duration-200 active:scale-95"
            style={{ background: "linear-gradient(135deg, #7C3AED, #9333EA, #DB2777)" }}
          >
            {t.onboarding.openFile}
          </button>
          <p className="text-[11px] text-white/30">{t.onboarding.orDrop}</p>
        </div>

        <div className="mt-5">
          <p className="mb-2 text-[10px] font-semibold uppercase tracking-widest text-white/25">
            {t.onboarding.keysTitle}
          </p>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] text-white/45">
            <span className="flex items-center gap-2">
              <Keycap>{keyFor("play_pause", "Space")}</Keycap>
              {t.onboarding.keyPlayPause}
            </span>
            <span className="flex items-center gap-2">
              <Keycap>{keyFor("seek_forward", "ArrowRight")}</Keycap>
              {t.onboarding.keySeek}
            </span>
            <span className="flex items-center gap-2">
              <Keycap>{keyFor("fullscreen", "f")}</Keycap>
              {t.onboarding.keyFullscreen}
            </span>
          </div>
        </div>

        {/* The differentiator. Set apart because it is the part nobody
            discovers, not because it is the part most people need first. */}
        <div className="gradient-border mt-5 rounded-xl bg-white/3 p-4">
          <p className="text-[11px] font-semibold text-white/75">{t.onboarding.agentTitle}</p>
          <p className="mt-1.5 text-[11px] leading-relaxed text-white/45">
            {t.onboarding.agentBody}
          </p>
          <code className="mt-2 block select-text font-mono text-[11px] text-brand-purple/90">
            {CLI_SAMPLE}
          </code>

          <p className="mt-3 text-[11px] text-white/45">{t.onboarding.agentClaude}</p>
          <div className="mt-1.5 flex items-start gap-2">
            <pre
              ref={codeRef}
              className="min-w-0 flex-1 select-text overflow-x-auto rounded-lg border border-white/8 bg-black/35 px-3 py-2 font-mono text-[11px] leading-relaxed text-white/70"
            >
              {CLAUDE_LINES.join("\n")}
            </pre>
            <button
              onClick={copy}
              className="flex-shrink-0 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-[10.5px] font-medium text-white/60 transition-colors hover:border-white/20 hover:bg-white/10 hover:text-white/85"
            >
              {copyState === "copied" ? t.onboarding.copied : t.onboarding.copy}
            </button>
          </div>
          {copyState === "manual" && (
            <p className="mt-1.5 text-[10px] text-amber-300/70">{t.onboarding.copyManual}</p>
          )}
        </div>

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            onClick={onDismiss}
            className="rounded-lg px-3 py-2 text-[11px] font-medium text-white/35 transition-colors hover:bg-white/6 hover:text-white/60"
          >
            {t.onboarding.skip}
          </button>
          <button
            onClick={onDismiss}
            className="rounded-lg border border-white/10 bg-white/5 px-4 py-2 text-[11px] font-semibold text-white/80 transition-colors hover:border-white/20 hover:bg-white/10 hover:text-white"
          >
            {t.onboarding.start}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

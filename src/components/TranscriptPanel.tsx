import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";

export interface TranscriptCue {
  index: number;
  start: number;
  end: number;
  speaker?: string | null;
  text: string;
}

export interface SpeakerStat {
  speaker: string;
  cue_count: number;
  word_count: number;
  speaking_seconds: number;
  first_seen: number;
  last_seen: number;
}

interface TranscriptPayload {
  source: string;
  origin: string;
  cues: TranscriptCue[];
  speakers: SpeakerStat[];
}

export default function TranscriptPanel({ onClose }: { onClose: () => void }) {
  const t = useStrings();
  const file = usePlayerStore((s) => s.file);
  const position = usePlayerStore((s) => s.position);
  const subtitles = usePlayerStore((s) => s.subtitles);
  const seek = usePlayerStore((s) => s.seek);

  const [cues, setCues] = useState<TranscriptCue[]>([]);
  const [speakers, setSpeakers] = useState<SpeakerStat[]>([]);
  const [loading, setLoading] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedSpeaker, setSelectedSpeaker] = useState<string | null>(null);
  const [editingSpeaker, setEditingSpeaker] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);

  const activeRowRef = useRef<HTMLButtonElement | null>(null);

  const loadTranscript = useCallback(async () => {
    if (!file) {
      setCues([]);
      setSpeakers([]);
      return;
    }
    setLoading(true);
    try {
      const res = await invoke<TranscriptPayload>("transcript_get", {});
      setCues(res.cues ?? []);
      setSpeakers(res.speakers ?? []);
    } catch {
      setCues([]);
      setSpeakers([]);
    } finally {
      setLoading(false);
    }
  }, [file]);

  useEffect(() => {
    void loadTranscript();
  }, [loadTranscript, subtitles.length]);

  const filteredCues = useMemo(() => {
    const q = query.trim().toLowerCase();
    const spk = selectedSpeaker?.toLowerCase() ?? null;
    return cues.filter((c) => {
      if (spk && (c.speaker ?? "").toLowerCase() !== spk) {
        return false;
      }
      if (q && !c.text.toLowerCase().includes(q)) {
        return false;
      }
      return true;
    });
  }, [cues, query, selectedSpeaker]);

  const activeIndex = useMemo(() => {
    for (let i = filteredCues.length - 1; i >= 0; i--) {
      const c = filteredCues[i];
      if (position >= c.start - 0.15 && position <= c.end + 0.25) {
        return c.index;
      }
    }
    return -1;
  }, [filteredCues, position]);

  useEffect(() => {
    if (autoScroll && !query.trim() && activeIndex >= 0 && activeRowRef.current) {
      activeRowRef.current.scrollIntoView({
        block: "nearest",
        behavior: "smooth",
      });
    }
  }, [activeIndex, autoScroll, query]);

  const handleDetectSpeakers = async () => {
    if (!file || detecting) return;
    setDetecting(true);
    try {
      const res = await invoke<TranscriptPayload>("transcript_speakers", {
        detect: true,
      });
      setCues(res.cues ?? []);
      setSpeakers(res.speakers ?? []);
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "error",
            message: typeof e === "string" ? e : t.transcript.empty,
          },
        }),
      );
    } finally {
      setDetecting(false);
    }
  };

  const handleRenameSubmit = async (from: string) => {
    const to = renameDraft.trim();
    setEditingSpeaker(null);
    if (!to || to === from) return;
    try {
      const res = await invoke<{
        cues: TranscriptCue[];
        speakers: SpeakerStat[];
      }>("transcript_rename_speaker", { from, to });
      setCues(res.cues ?? []);
      setSpeakers(res.speakers ?? []);
      if (selectedSpeaker === from) {
        setSelectedSpeaker(to);
      }
    } catch {
      /* ignore */
    }
  };

  const handleExport = async () => {
    try {
      const dialog = await invoke<{ path: string | null }>("save_transcript_dialog", {
        defaultName: "transcript.txt",
      });
      if (!dialog.path) return;
      const ext = dialog.path.split(".").pop()?.toLowerCase();
      const format =
        ext === "srt" || ext === "vtt" || ext === "json" ? ext : "txt";
      await invoke("transcript_export", {
        output: dialog.path,
        format,
        speaker: selectedSpeaker ?? undefined,
      });
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "success",
            message: t.transcript.exported,
          },
        }),
      );
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent("unflick:toast", {
          detail: {
            kind: "error",
            message: typeof e === "string" ? e : t.transcript.exportFailed,
          },
        }),
      );
    }
  };

  return (
    <motion.div
      data-transcript-panel
      className="absolute bottom-0 right-0 top-0 z-30 flex w-80 flex-col"
      style={{
        background: "var(--bg-primary)",
        borderLeft: "1px solid var(--border-subtle)",
      }}
      initial={{ x: 320 }}
      animate={{ x: 0 }}
      exit={{ x: 320 }}
      transition={{ type: "spring", damping: 30, stiffness: 300 }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between px-4 py-3"
        style={{ borderBottom: "1px solid var(--border-subtle)" }}
      >
        <h2 className="idle-title text-[12px] font-bold uppercase tracking-wider">
          {t.transcript.title}
        </h2>
        <div className="flex items-center gap-1">
          {cues.length > 0 && (
            <>
              <button
                type="button"
                className="rounded-lg px-2 py-1 text-[10px] font-medium text-white/35 transition-colors hover:bg-white/6 hover:text-white/75 disabled:opacity-40"
                onClick={handleDetectSpeakers}
                disabled={detecting}
                title={t.transcript.detectSpeakers}
              >
                {detecting ? t.transcript.detectingSpeakers : t.transcript.detectSpeakers}
              </button>
              <button
                type="button"
                className="rounded-lg p-1.5 text-white/30 transition-colors hover:bg-white/6 hover:text-white/60"
                onClick={handleExport}
                title={t.transcript.export}
                aria-label={t.transcript.export}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                  <polyline points="7 10 12 15 17 10" />
                  <line x1="12" y1="15" x2="12" y2="3" />
                </svg>
              </button>
            </>
          )}
          <button
            type="button"
            className="rounded-lg p-1 text-white/25 transition-colors hover:bg-white/6 hover:text-white/50"
            onClick={onClose}
            title={t.transcript.close}
            aria-label={t.transcript.close}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>

      {/* Search & speaker filters */}
      {cues.length > 0 && (
        <div
          className="flex flex-col gap-2 px-3 py-2"
          style={{ borderBottom: "1px solid var(--border-subtle)" }}
        >
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t.transcript.searchPlaceholder}
            className="w-full rounded-lg border border-white/8 bg-white/4 px-2.5 py-1.5 text-[11px] text-white/80 placeholder-white/25 outline-none focus:border-brand-purple/50"
          />

          {speakers.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <button
                type="button"
                onClick={() => setSelectedSpeaker(null)}
                className={`rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors ${
                  selectedSpeaker === null
                    ? "bg-brand-purple/25 text-brand-purple border border-brand-purple/40"
                    : "bg-white/5 text-white/40 hover:bg-white/10 hover:text-white/70"
                }`}
              >
                {t.transcript.allSpeakers}
              </button>
              {speakers.map((s) =>
                editingSpeaker === s.speaker ? (
                  <input
                    key={s.speaker}
                    type="text"
                    autoFocus
                    value={renameDraft}
                    onChange={(e) => setRenameDraft(e.target.value)}
                    placeholder={t.transcript.renamePlaceholder}
                    onBlur={() => void handleRenameSubmit(s.speaker)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void handleRenameSubmit(s.speaker);
                      if (e.key === "Escape") setEditingSpeaker(null);
                    }}
                    className="w-24 rounded-full border border-brand-purple/50 bg-white/8 px-2 py-0.5 text-[10px] text-white outline-none"
                  />
                ) : (
                  <div key={s.speaker} className="flex items-center gap-0.5">
                    <button
                      type="button"
                      onClick={() =>
                        setSelectedSpeaker(
                          selectedSpeaker === s.speaker ? null : s.speaker,
                        )
                      }
                      onDoubleClick={() => {
                        setEditingSpeaker(s.speaker);
                        setRenameDraft(s.speaker);
                      }}
                      title={t.transcript.renameSpeaker}
                      className={`rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors ${
                        selectedSpeaker === s.speaker
                          ? "bg-brand-purple/25 text-brand-purple border border-brand-purple/40"
                          : "bg-white/5 text-white/45 hover:bg-white/10 hover:text-white/75"
                      }`}
                    >
                      {s.speaker}
                    </button>
                  </div>
                ),
              )}
            </div>
          )}
        </div>
      )}

      {/* Cue list */}
      <div className="flex-1 overflow-y-auto px-2 py-1.5">
        {loading && (
          <div className="flex items-center justify-center py-12">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-brand-purple border-t-transparent" />
          </div>
        )}

        {!loading && cues.length === 0 && (
          <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
            <p className="text-[12px] text-white/35">{t.transcript.empty}</p>
          </div>
        )}

        {!loading && cues.length > 0 && filteredCues.length === 0 && (
          <div className="flex flex-col items-center justify-center px-6 py-12 text-center">
            <p className="text-[12px] text-white/35">{t.transcript.noMatch}</p>
          </div>
        )}

        {!loading &&
          filteredCues.map((c) => {
            const isActive = c.index === activeIndex;
            return (
              <button
                key={c.index}
                ref={isActive ? activeRowRef : null}
                type="button"
                onClick={() => void seek(c.start)}
                className={`group flex w-full items-start gap-2 rounded-xl px-2.5 py-1.5 text-left transition-colors ${
                  isActive
                    ? "bg-brand-purple/15 border border-brand-purple/25"
                    : "hover:bg-white/4"
                }`}
              >
                <span
                  className={`mt-0.5 flex-shrink-0 font-mono text-[10px] tabular-nums ${
                    isActive
                      ? "text-brand-purple font-semibold"
                      : "text-white/25 group-hover:text-white/45"
                  }`}
                >
                  {formatTime(c.start)}
                </span>
                <div className="min-w-0 flex-1">
                  {c.speaker && (
                    <span className="mr-1.5 inline-block rounded bg-white/8 px-1.5 py-0.2 text-[9px] font-semibold text-white/60">
                      {c.speaker}
                    </span>
                  )}
                  <span
                    className={`text-[11.5px] leading-relaxed ${
                      isActive
                        ? "font-medium text-white/95"
                        : "text-white/60 group-hover:text-white/80"
                    }`}
                  >
                    {c.text}
                  </span>
                </div>
              </button>
            );
          })}
      </div>

      {/* Footer */}
      {!loading && cues.length > 0 && (
        <div
          className="flex items-center justify-between px-4 py-2"
          style={{ borderTop: "1px solid var(--border-subtle)" }}
        >
          <p className="text-[10px] font-medium text-white/20">
            {t.transcript.cues.replace("{count}", String(filteredCues.length))}
          </p>
          <button
            type="button"
            onClick={() => setAutoScroll((v) => !v)}
            className={`rounded-lg px-2 py-0.5 text-[10px] font-medium transition-colors ${
              autoScroll
                ? "text-brand-purple bg-brand-purple/10"
                : "text-white/25 hover:text-white/50"
            }`}
          >
            {t.transcript.autoScroll}
          </button>
        </div>
      )}
    </motion.div>
  );
}

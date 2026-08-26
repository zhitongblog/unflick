import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { usePlayerStore } from "../stores/playerStore";
import { useStrings } from "../i18n/utils";
import { formatTime } from "../lib/format";

/**
 * Bookmark popover: the list for the open file, plus the one button that
 * makes a new one.
 *
 * Naming happens here rather than in a dialog. A bookmark is worth making
 * in the second you decide you want it, so the keyboard shortcut saves
 * one unnamed and this list lets you label it afterwards — the label is
 * never in the way of saving the spot.
 */
export default function BookmarkMenu({ onClose }: { onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const { bookmarks, addBookmark, gotoBookmark, renameBookmark, removeBookmark, file } =
    usePlayerStore();
  const t = useStrings();

  /** Id of the bookmark being renamed, with its draft label. */
  const [editing, setEditing] = useState<{ id: number; value: string } | null>(null);
  const editInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  // Slide the mpv overlay window out of the way while this is mounted,
  // otherwise the popover renders behind the video on Windows.
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("unflick:popover-open"));
    return () => {
      window.dispatchEvent(new CustomEvent("unflick:popover-close"));
    };
  }, []);

  // The list is a snapshot of a table the CLI and MCP write to as well, so
  // re-read it whenever the popover is opened rather than trusting what
  // was there last time.
  useEffect(() => {
    void usePlayerStore.getState().refreshBookmarks();
  }, []);

  useEffect(() => {
    if (editing) editInput.current?.select();
  }, [editing]);

  const commitRename = () => {
    if (!editing) return;
    const name = editing.value.trim();
    void renameBookmark(editing.id, name.length > 0 ? name : null);
    setEditing(null);
  };

  return (
    <motion.div
      ref={menuRef}
      initial={{ opacity: 0, y: 8, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.95 }}
      transition={{ duration: 0.12 }}
      className="glass-elevated absolute bottom-full right-0 mb-2 max-h-80 w-72 overflow-y-auto rounded-xl py-1.5 shadow-2xl"
    >
      <p className="px-3 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-widest text-white/25">
        {t.bookmarks.title}
      </p>

      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] text-white/60 transition-colors hover:bg-white/6 disabled:opacity-30"
        disabled={!file}
        onClick={() => {
          void addBookmark();
        }}
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor">
          <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6z" />
        </svg>
        <span className="flex-1">{t.bookmarks.add}</span>
      </button>

      {bookmarks.length > 0 && <div className="my-1 h-px bg-white/8" />}

      {bookmarks.length === 0 && (
        <p className="px-3 py-2 text-[11px] text-white/25">{t.bookmarks.none}</p>
      )}

      {bookmarks.map((b) => (
        <div
          key={b.id}
          className="group flex w-full items-center gap-1 px-3 py-1.5 text-[11px] transition-colors hover:bg-white/6"
        >
          {editing?.id === b.id ? (
            <input
              ref={editInput}
              className="min-w-0 flex-1 rounded bg-white/10 px-1 py-0.5 text-[11px] text-white/80 outline-none"
              value={editing.value}
              placeholder={t.bookmarks.namePlaceholder}
              onChange={(e) => setEditing({ id: b.id, value: e.target.value })}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") commitRename();
                if (e.key === "Escape") setEditing(null);
              }}
              onBlur={commitRename}
            />
          ) : (
            <button
              className="min-w-0 flex-1 truncate text-left text-white/60 transition-colors hover:text-white/90"
              title={b.name ?? undefined}
              onClick={() => {
                void gotoBookmark(b);
                onClose();
              }}
            >
              {b.name ?? formatTime(b.position)}
            </button>
          )}

          <span className="flex-shrink-0 text-[10px] tabular-nums text-white/25">
            {formatTime(b.position)}
          </span>

          <button
            className="flex-shrink-0 p-0.5 text-white/0 transition-colors group-hover:text-white/30 hover:!text-white/70"
            title={t.bookmarks.rename}
            onClick={() => setEditing({ id: b.id, value: b.name ?? "" })}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor">
              <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 000-1.41l-2.34-2.34a1 1 0 00-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z" />
            </svg>
          </button>

          <button
            className="flex-shrink-0 p-0.5 text-white/0 transition-colors group-hover:text-white/30 hover:!text-white/70"
            title={t.bookmarks.remove}
            onClick={() => {
              void removeBookmark(b.id);
            }}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 19a2 2 0 002 2h8a2 2 0 002-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z" />
            </svg>
          </button>
        </div>
      ))}
    </motion.div>
  );
}

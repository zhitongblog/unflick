import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useIncognitoStore } from "../stores/incognitoStore";
import { useStrings } from "../i18n/utils";

const appWindow = getCurrentWebviewWindow();

export default function TitleBar() {
  const incognito = useIncognitoStore((s) => s.enabled);
  const t = useStrings();
  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 items-center justify-between px-3 select-none"
      style={{
        background: "linear-gradient(to bottom, rgba(0,0,0,0.4), rgba(0,0,0,0.15))",
      }}
    >
      {/* Left: brand name + optional incognito badge */}
      <span
        data-tauri-drag-region
        className="idle-title flex items-center gap-2 text-[11px] font-bold tracking-wide pointer-events-none uppercase"
      >
        unflick
        {incognito && (
          <span
            className="flex items-center gap-1 rounded-full border border-violet-400/30 bg-violet-500/15 px-2 py-0.5 text-[9px] font-semibold tracking-wider text-violet-200 normal-case"
            title={t.context.incognito}
          >
            <svg width="9" height="9" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M12 4a4 4 0 0 1 4 4v2H8V8a4 4 0 0 1 4-4Zm-6 8h12a2 2 0 0 1 2 2v3a4 4 0 0 1-4 4 4 4 0 0 1-4-2.5A4 4 0 0 1 8 21a4 4 0 0 1-4-4v-3a2 2 0 0 1 2-2Z"/>
            </svg>
            {t.context.incognito}
          </span>
        )}
      </span>

      {/* Right: window controls */}
      <div className="flex items-center">
        {/* Minimize */}
        <button
          onClick={() => appWindow.minimize()}
          className="flex h-8 w-10 items-center justify-center text-white/40 transition-all duration-150 hover:bg-white/8 hover:text-white/70"
          title="Minimize"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>

        {/* Maximize / Restore */}
        <button
          onClick={() => appWindow.toggleMaximize()}
          className="flex h-8 w-10 items-center justify-center text-white/40 transition-all duration-150 hover:bg-white/8 hover:text-white/70"
          title="Maximize"
        >
          <svg width="9" height="9" viewBox="0 0 9 9" fill="none" stroke="currentColor" strokeWidth="1.2">
            <rect x="0.5" y="0.5" width="8" height="8" rx="1" />
          </svg>
        </button>

        {/* Close */}
        <button
          onClick={() => appWindow.close()}
          className="flex h-8 w-10 items-center justify-center text-white/40 transition-all duration-150 hover:bg-red-500/80 hover:text-white"
          title="Close"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
            <line x1="1.5" y1="1.5" x2="8.5" y2="8.5" />
            <line x1="8.5" y1="1.5" x2="1.5" y2="8.5" />
          </svg>
        </button>
      </div>
    </div>
  );
}

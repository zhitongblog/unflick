import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

const appWindow = getCurrentWebviewWindow();

export default function TitleBar() {
  return (
    <div
      data-tauri-drag-region
      className="flex h-8 shrink-0 items-center justify-between bg-black/30 backdrop-blur-md px-3 select-none"
    >
      {/* Left: brand name */}
      <span
        data-tauri-drag-region
        className="bg-gradient-to-r from-brand-purple to-brand-pink bg-clip-text text-xs font-semibold text-transparent pointer-events-none"
      >
        unflick
      </span>

      {/* Right: window controls */}
      <div className="flex items-center gap-1">
        {/* Minimize */}
        <button
          onClick={() => appWindow.minimize()}
          className="flex h-6 w-6 items-center justify-center rounded text-gray-400 transition-colors hover:bg-white/10 hover:text-white"
          title="Minimize"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>

        {/* Maximize / Restore */}
        <button
          onClick={() => appWindow.toggleMaximize()}
          className="flex h-6 w-6 items-center justify-center rounded text-gray-400 transition-colors hover:bg-white/10 hover:text-white"
          title="Maximize"
        >
          <svg width="9" height="9" viewBox="0 0 9 9" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="0.5" width="8" height="8" />
          </svg>
        </button>

        {/* Close */}
        <button
          onClick={() => appWindow.close()}
          className="flex h-6 w-6 items-center justify-center rounded text-gray-400 transition-colors hover:bg-red-600 hover:text-white"
          title="Close"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round">
            <line x1="1" y1="1" x2="9" y2="9" />
            <line x1="9" y1="1" x2="1" y2="9" />
          </svg>
        </button>
      </div>
    </div>
  );
}

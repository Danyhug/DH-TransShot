import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Props {
  onScreenshot?: () => void;
  onOcrTranslate?: () => void;
  onDebugLog?: () => void;
  onSettings?: () => void;
}

export function TitleBar({ onScreenshot, onOcrTranslate, onDebugLog, onSettings }: Props) {
  const appWindow = getCurrentWindow();
  const [pinned, setPinned] = useState(false);

  const togglePin = async () => {
    const next = !pinned;
    setPinned(next);
    await appWindow.setAlwaysOnTop(next);
  };

  const btnClass =
    "w-7 h-7 flex items-center justify-center rounded-md hover:bg-black/5 active:bg-black/10 transition-colors";

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-10 px-4 select-none shrink-0"
    >
      {/* Left: Pin button */}
      <button
        onClick={togglePin}
        className={btnClass}
        style={{ color: pinned ? "var(--color-primary)" : "var(--color-text-secondary)" }}
        title={pinned ? "取消置顶" : "窗口置顶"}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill={pinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 17v5" />
          <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76z" />
        </svg>
      </button>

      {/* Right: Action icons */}
      <div className="flex items-center gap-0.5">
        {/* Camera - Region Screenshot */}
        <button
          onClick={onScreenshot}
          className={btnClass}
          style={{ color: "var(--color-text-secondary)" }}
          title="区域截图 (⌥A)"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z" />
            <circle cx="12" cy="13" r="3" />
          </svg>
        </button>

        {/* Crop - Region OCR Translate */}
        <button
          onClick={onOcrTranslate}
          className={btnClass}
          style={{ color: "var(--color-text-secondary)" }}
          title="区域翻译 (⌥S)"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 2v4H2" />
            <path d="M18 22v-4h4" />
            <path d="M6 6h10a2 2 0 0 1 2 2v10" />
            <path d="M18 18H8a2 2 0 0 1-2-2V6" />
          </svg>
        </button>

        {/* FileText - Debug Log */}
        <button
          onClick={onDebugLog}
          className={btnClass}
          style={{ color: "var(--color-text-secondary)" }}
          title="调试日志"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
            <path d="M14 2v4a2 2 0 0 0 2 2h4" />
            <path d="M10 13H8" />
            <path d="M16 13h-2" />
            <path d="M10 17H8" />
            <path d="M16 17h-2" />
          </svg>
        </button>

        {/* Toggle - Settings */}
        <button
          onClick={onSettings}
          className={btnClass}
          style={{ color: "var(--color-text-secondary)" }}
          title="设置"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="1" y="5" width="22" height="14" rx="7" ry="7" />
            <circle cx="16" cy="12" r="3" />
          </svg>
        </button>
      </div>
    </div>
  );
}

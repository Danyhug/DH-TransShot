import { useState, useEffect, useRef } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readClipboard } from "../../lib/invoke";
import { MAX_LOGS } from "../../stores/logStore";
import type { LogEntry } from "../../stores/logStore";

export function LogPanel() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [clipboard, setClipboard] = useState<string>("");
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Read clipboard on mount
    readClipboard().then(setClipboard).catch(() => setClipboard("(无法读取剪贴板)"));

    // Request full log history from main window
    emit("debug-log-ready");

    // Listen for initial full log dump
    const unlistenInit = listen<LogEntry[]>("debug-log-init", (event) => {
      setLogs(event.payload);
    });

    // Listen for incremental log entries
    const unlistenEntry = listen<LogEntry>("debug-log-entry", (event) => {
      setLogs((prev) => {
        const next = [...prev, event.payload];
        return next.length > MAX_LOGS ? next.slice(-MAX_LOGS) : next;
      });
    });

    // Listen for clear events
    const unlistenClear = listen("debug-log-clear", () => {
      setLogs([]);
    });

    return () => {
      unlistenInit.then((fn) => fn());
      unlistenEntry.then((fn) => fn());
      unlistenClear.then((fn) => fn());
    };
  }, []);

  // Auto-scroll to bottom when logs change
  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [logs]);

  const levelColor = (level: LogEntry["level"]): string => {
    switch (level) {
      case "error":
        return "#ef4444";
      case "warn":
        return "#eab308";
      default:
        return "var(--color-text-secondary)";
    }
  };

  const copyAll = () => {
    const text = logs.map((l) => `[${l.time}] [${l.level.toUpperCase()}] ${l.message}`).join("\n");
    navigator.clipboard.writeText(text).catch(() => {});
  };

  const clearLogs = () => {
    setLogs([]);
    // Notify main window to clear log store as well
    emit("debug-log-clear-request");
  };

  const close = () => {
    getCurrentWindow().close();
  };

  return (
    <div
      className="flex flex-col h-screen rounded-xl overflow-hidden"
      style={{ backgroundColor: "var(--color-bg)" }}
    >
      {/* Draggable title bar */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between h-10 px-4 select-none shrink-0"
      >
        <span
          className="text-sm font-semibold"
          style={{ color: "var(--color-text)" }}
        >
          调试日志
        </span>
        <button
          onClick={close}
          className="w-7 h-7 flex items-center justify-center rounded-md hover:bg-black/5 active:bg-black/10 transition-colors"
          style={{ color: "var(--color-text-secondary)" }}
          title="关闭"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </div>

      {/* Clipboard section (top) */}
      <div className="shrink-0 px-4 pb-3">
        <h3
          className="text-xs font-medium"
          style={{ color: "var(--color-text-secondary)", marginBottom: "6px" }}
        >
          剪贴板内容
        </h3>
        <div
          className="text-xs font-mono overflow-auto"
          style={{
            backgroundColor: "var(--color-surface)",
            color: "var(--color-text)",
            borderRadius: "8px",
            padding: "8px 10px",
            maxHeight: "80px",
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
          }}
        >
          {clipboard || "(空)"}
        </div>
      </div>

      {/* Log list (fills remaining space) */}
      <div
        ref={listRef}
        className="flex-1 overflow-y-auto px-4"
        style={{ minHeight: 0 }}
      >
        {logs.length === 0 ? (
          <div className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
            暂无日志
          </div>
        ) : (
          <div className="space-y-1">
            {logs.map((entry) => (
              <div
                key={entry.id}
                className="text-xs font-mono leading-relaxed"
                style={{ color: "var(--color-text)" }}
              >
                <span style={{ color: "var(--color-text-secondary)" }}>{entry.time}</span>{" "}
                <span style={{ color: levelColor(entry.level), fontWeight: 600 }}>
                  [{entry.level.toUpperCase()}]
                </span>{" "}
                {entry.message}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Action bar (bottom) */}
      <div className="flex items-center gap-2 shrink-0 px-4 py-3">
        <button
          onClick={clearLogs}
          className="text-xs transition-colors hover:opacity-80"
          style={{
            color: "var(--color-text-secondary)",
            padding: "4px 10px",
            borderRadius: "6px",
            backgroundColor: "var(--color-surface)",
            border: "none",
          }}
        >
          清除
        </button>
        <button
          onClick={copyAll}
          className="text-xs transition-colors hover:opacity-80"
          style={{
            color: "var(--color-text-secondary)",
            padding: "4px 10px",
            borderRadius: "6px",
            backgroundColor: "var(--color-surface)",
            border: "none",
          }}
        >
          复制全部
        </button>
      </div>
    </div>
  );
}

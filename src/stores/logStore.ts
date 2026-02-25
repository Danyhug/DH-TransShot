import { create } from "zustand";
import { emitTo, listen } from "@tauri-apps/api/event";
import { openDockedWindow } from "../lib/windowUtils";
import { info as logInfo, warn as logWarn, error as logError } from "@tauri-apps/plugin-log";

export interface LogEntry {
  id: number;
  time: string;
  level: "info" | "warn" | "error";
  message: string;
}

interface LogState {
  logs: LogEntry[];
  addLog: (level: LogEntry["level"], message: string) => void;
  clear: () => void;
}

export const MAX_LOGS = 200;
let nextId = 1;

function now(): string {
  const d = new Date();
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}

export const useLogStore = create<LogState>((set) => ({
  logs: [],
  addLog: (level, message) =>
    set((state) => {
      const entry: LogEntry = { id: nextId++, time: now(), level, message };
      const logs = [...state.logs, entry];
      // Push to debug window in real time
      emitTo("debug-log", "debug-log-entry", entry).catch(() => {});
      return { logs: logs.length > MAX_LOGS ? logs.slice(-MAX_LOGS) : logs };
    }),
  clear: () => {
    set({ logs: [] });
    emitTo("debug-log", "debug-log-clear", {}).catch(() => {});
  },
}));

/** Open (or focus) the debug-log window, docked to the right of main window */
export async function openDebugWindow() {
  await openDockedWindow({
    label: "debug-log",
    url: "debug.html",
    title: "调试日志",
    width: 360,
    side: "right",
  });
}

/**
 * Set up event listeners on the main window side.
 * Call once in App.tsx useEffect; returns cleanup function.
 */
export function setupMainWindowLogListeners(): () => void {
  // When debug window signals ready, send full log history
  const unlistenReady = listen("debug-log-ready", () => {
    const { logs } = useLogStore.getState();
    emitTo("debug-log", "debug-log-init", logs).catch(() => {});
  });

  // When debug window requests clear, clear the main store
  const unlistenClearReq = listen("debug-log-clear-request", () => {
    useLogStore.getState().clear();
  });

  return () => {
    unlistenReady.then((fn) => fn());
    unlistenClearReq.then((fn) => fn());
  };
}

export const appLog = {
  info: (msg: string) => {
    console.log(msg);
    useLogStore.getState().addLog("info", msg);
    logInfo(msg).catch(() => {});
  },
  warn: (msg: string) => {
    console.warn(msg);
    useLogStore.getState().addLog("warn", msg);
    logWarn(msg).catch(() => {});
  },
  error: (msg: string) => {
    console.error(msg);
    useLogStore.getState().addLog("error", msg);
    logError(msg).catch(() => {});
  },
};

import { useEffect, useRef, useState } from "react";
import { appLog } from "../../stores/logStore";

interface HotkeyInputProps {
  value: string;
  onChange: (value: string) => void;
}

const MOD_LABEL_MAC: Record<string, string> = {
  Alt: "⌥",
  Ctrl: "⌃",
  Shift: "⇧",
  Cmd: "⌘",
};

const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/i.test(navigator.platform);

/** Render "Alt+A" as "⌥A" on macOS, "Alt+A" on Windows/Linux. */
export function formatShortcut(value: string): string {
  if (!value) return "";
  if (!isMac) return value;
  const parts = value.split("+").map((p) => p.trim()).filter(Boolean);
  return parts.map((p) => MOD_LABEL_MAC[p] ?? p).join("");
}

/** Map browser KeyboardEvent.code into the token format the Rust parser expects. */
function codeToToken(code: string, key: string): string | null {
  if (code.startsWith("Key") && code.length === 4) return code.slice(3); // KeyA -> A
  if (code.startsWith("Digit") && code.length === 6) return code.slice(5); // Digit1 -> 1
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code; // F1..F24
  switch (code) {
    case "Space": return "Space";
    case "Enter": return "Enter";
    case "Tab": return "Tab";
    case "Backspace": return "Backspace";
    case "Delete": return "Delete";
    case "Escape": return "Escape";
    case "ArrowUp": return "ArrowUp";
    case "ArrowDown": return "ArrowDown";
    case "ArrowLeft": return "ArrowLeft";
    case "ArrowRight": return "ArrowRight";
    case "Home": return "Home";
    case "End": return "End";
    case "PageUp": return "PageUp";
    case "PageDown": return "PageDown";
    case "Minus": return "-";
    case "Equal": return "=";
    case "BracketLeft": return "[";
    case "BracketRight": return "]";
    case "Backslash": return "\\";
    case "Semicolon": return ";";
    case "Quote": return "'";
    case "Comma": return ",";
    case "Period": return ".";
    case "Slash": return "/";
    case "Backquote": return "`";
    default:
      // Fallback: single-char keys like punctuation
      if (key && key.length === 1) return key.toUpperCase();
      return null;
  }
}

export function HotkeyInput({ value, onChange }: HotkeyInputProps) {
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      // Modifier-only press: ignore until a non-modifier key is added
      const mods: string[] = [];
      if (e.altKey) mods.push("Alt");
      if (e.ctrlKey) mods.push("Ctrl");
      if (e.shiftKey) mods.push("Shift");
      if (e.metaKey) mods.push("Cmd");

      // Cancel on Escape (with no modifiers)
      if (e.code === "Escape" && mods.length === 0) {
        setRecording(false);
        setError(null);
        return;
      }

      // Skip pure modifier keydown
      if (["AltLeft", "AltRight", "ControlLeft", "ControlRight",
           "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight"].includes(e.code)) {
        return;
      }

      const token = codeToToken(e.code, e.key);
      if (!token) {
        setError(`不支持的按键: ${e.code}`);
        return;
      }
      if (mods.length === 0) {
        setError("至少需要一个修饰键 (Alt/Ctrl/Shift/Cmd)");
        return;
      }
      const combo = [...mods, token].join("+");
      appLog.info(`[Settings] 录入快捷键: ${combo}`);
      onChange(combo);
      setRecording(false);
      setError(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording, onChange]);

  const startRecording = () => {
    setRecording(true);
    setError(null);
    inputRef.current?.focus();
  };

  return (
    <div>
      <button
        ref={inputRef as unknown as React.RefObject<HTMLButtonElement>}
        type="button"
        onClick={startRecording}
        onBlur={() => { if (recording) { setRecording(false); setError(null); } }}
        className="text-xs font-mono outline-none transition-colors"
        style={{
          padding: "4px 10px",
          borderRadius: "6px",
          border: recording ? "1px dashed var(--color-primary)" : "1px solid transparent",
          backgroundColor: recording ? "transparent" : "var(--color-surface)",
          color: recording ? "var(--color-primary)" : "var(--color-text)",
          minWidth: "84px",
          cursor: "pointer",
        }}
        title={recording ? "按下组合键，Esc 取消" : "点击录入新快捷键"}
      >
        {recording ? "按下组合键..." : (formatShortcut(value) || "未设置")}
      </button>
      {error && (
        <div className="text-xs mt-1" style={{ color: "var(--color-error, #e53935)" }}>
          {error}
        </div>
      )}
    </div>
  );
}

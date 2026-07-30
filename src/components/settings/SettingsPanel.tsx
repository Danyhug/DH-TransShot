import { useState, useEffect, useCallback, type ReactNode } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, saveSettings, suspendHotkeys, resumeHotkeys } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import { defaultSettings } from "../../stores/settingsStore";
import { ServiceSettings, type ServiceName } from "./ServiceSettings";
import { HotkeySettings } from "./HotkeySettings";
import { SpeechSettings } from "./SpeechSettings";
import type { Settings, ExtraProvider, HotkeyConfig } from "../../types";

type SectionKey = "service" | "hotkey" | "speech";

const ICON_PROPS = {
  width: 15,
  height: 15,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 2,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

const navItems: { key: SectionKey; label: string; icon: ReactNode }[] = [
  {
    key: "service",
    label: "服务",
    icon: (
      <svg {...ICON_PROPS}>
        <rect width="20" height="8" x="2" y="2" rx="2" />
        <rect width="20" height="8" x="2" y="14" rx="2" />
        <path d="M6 6h.01" />
        <path d="M6 18h.01" />
      </svg>
    ),
  },
  {
    key: "hotkey",
    label: "快捷键",
    icon: (
      <svg {...ICON_PROPS}>
        <rect width="20" height="16" x="2" y="4" rx="2" />
        <path d="M6 8h.01" />
        <path d="M10 8h.01" />
        <path d="M14 8h.01" />
        <path d="M18 8h.01" />
        <path d="M8 12h.01" />
        <path d="M12 12h.01" />
        <path d="M16 12h.01" />
        <path d="M7 16h10" />
      </svg>
    ),
  },
  {
    key: "speech",
    label: "朗读",
    icon: (
      <svg {...ICON_PROPS}>
        <path d="M11 5 6 9H2v6h4l5 4V5z" />
        <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
        <path d="M19.07 4.93a7 7 0 0 1 0 14.14" />
      </svg>
    ),
  },
];

export function SettingsPanel() {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [section, setSection] = useState<SectionKey>("service");
  const [activeService, setActiveService] = useState<ServiceName>("translation");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    appLog.info("[Settings] 设置窗口: 加载配置...");
    getSettings()
      .then((s) => {
        appLog.info("[Settings] 设置窗口: 配置加载成功, translation.model=" + s.translation.model);
        setSettings(s);
      })
      .catch((e) => appLog.error("[Settings] 设置窗口: 配置加载失败: " + String(e)));
  }, []);

  // 设置面板期间挂起全局快捷键，避免录入新组合时被系统拦截
  useEffect(() => {
    suspendHotkeys().catch((e) => appLog.warn("[Settings] suspend_hotkeys 失败: " + String(e)));
    return () => {
      resumeHotkeys().catch((e) => appLog.warn("[Settings] resume_hotkeys 失败: " + String(e)));
    };
  }, []);

  const updateGlobal = useCallback((key: "base_url" | "api_key", value: string) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  }, []);

  const updateService = useCallback(
    (service: ServiceName, key: "model" | "extra", value: string) => {
      setSettings((prev) => ({ ...prev, [service]: { ...prev[service], [key]: value } }));
    },
    []
  );

  const updateProviders = useCallback((service: ServiceName, providers: ExtraProvider[]) => {
    setSettings((prev) => ({ ...prev, [service]: { ...prev[service], providers } }));
  }, []);

  const updateActiveProvider = useCallback((service: ServiceName, active: number) => {
    setSettings((prev) => ({ ...prev, [service]: { ...prev[service], active } }));
  }, []);

  const updateHotkey = useCallback((key: keyof HotkeyConfig, value: string) => {
    setError(null);
    setSettings((prev) => ({ ...prev, hotkeys: { ...prev.hotkeys, [key]: value } }));
  }, []);

  const updateSpeech = useCallback((key: keyof Settings["speech"], value: boolean) => {
    setSettings((prev) => ({ ...prev, speech: { ...prev.speech, [key]: value } }));
  }, []);

  const save = useCallback(async () => {
    const hk = settings.hotkeys;
    if (!hk?.screenshot?.trim() || !hk?.ocr_translate?.trim() || !hk?.clipboard_translate?.trim()) {
      appLog.warn("[Settings] 快捷键不能为空");
      // 跳到快捷键分区并就地标红，比 alert 更容易定位到底哪一项没填
      setSection("hotkey");
      setError("三个动作都需要设置快捷键");
      return;
    }
    setError(null);
    try {
      appLog.info(
        "[Settings] 保存配置, translation.model=" +
          settings.translation.model +
          ", ocr.model=" +
          settings.ocr.model
      );
      await saveSettings(settings);
      appLog.info("[Settings] 配置保存成功");
      await emit("settings-saved");
    } catch (e) {
      appLog.error("[Settings] 配置保存失败: " + String(e));
      setError("保存失败：" + String(e));
      return;
    }
    // Tauri may destroy the webview without running React's effect cleanup.
    // Resume explicitly before closing; the Rust window-destroyed handler is
    // an additional fallback for the title-bar close button or a crash.
    try {
      await resumeHotkeys();
    } catch (e) {
      appLog.warn("[Settings] 保存后恢复快捷键失败: " + String(e));
    }
    await getCurrentWindow().close();
  }, [settings]);

  const close = useCallback(async () => {
    try {
      await resumeHotkeys();
    } catch (e) {
      appLog.warn("[Settings] 关闭前恢复快捷键失败: " + String(e));
    } finally {
      await getCurrentWindow().close();
    }
  }, []);

  return (
    <div
      className="flex flex-col h-screen rounded-xl overflow-hidden"
      style={{ backgroundColor: "var(--color-bg)" }}
    >
      {/* Draggable title bar */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between h-11 px-4 select-none shrink-0"
      >
        <span className="text-sm font-semibold" style={{ color: "var(--color-text)" }}>
          设置
        </span>
        <button
          onClick={close}
          className="w-7 h-7 flex items-center justify-center rounded-md hover:bg-black/5 active:bg-black/10 transition-colors"
          style={{ color: "var(--color-text-secondary)" }}
          title="关闭"
        >
          <svg {...ICON_PROPS} width="14" height="14">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </div>

      {/* Sidebar + content */}
      <div className="flex flex-1" style={{ minHeight: 0 }}>
        <nav
          className="shrink-0 flex flex-col gap-0.5 py-2 px-2"
          style={{ width: "148px", borderRight: "1px solid var(--color-border)" }}
        >
          {navItems.map((item) => {
            const active = section === item.key;
            return (
              <button
                key={item.key}
                onClick={() => setSection(item.key)}
                className="relative flex items-center gap-2.5 text-xs font-medium transition-colors text-left"
                style={{
                  padding: "8px 10px",
                  borderRadius: "8px",
                  border: "none",
                  cursor: "pointer",
                  backgroundColor: active ? "var(--color-surface)" : "transparent",
                  color: active ? "var(--color-primary)" : "var(--color-text-secondary)",
                }}
              >
                {active && (
                  <span
                    className="absolute"
                    style={{
                      left: 0,
                      top: "8px",
                      bottom: "8px",
                      width: "2px",
                      borderRadius: "9999px",
                      backgroundColor: "var(--color-primary)",
                    }}
                  />
                )}
                {item.icon}
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="flex-1 overflow-y-auto" style={{ minWidth: 0, padding: "18px 22px 24px" }}>
          {section === "service" && (
            <ServiceSettings
              settings={settings}
              activeService={activeService}
              onActiveServiceChange={setActiveService}
              onGlobalChange={updateGlobal}
              onServiceChange={updateService}
              onProvidersChange={updateProviders}
              onActiveProviderChange={updateActiveProvider}
            />
          )}
          {section === "hotkey" && (
            <HotkeySettings
              hotkeys={settings.hotkeys}
              invalid={error !== null}
              onChange={updateHotkey}
            />
          )}
          {section === "speech" && (
            <SpeechSettings speech={settings.speech} onChange={updateSpeech} />
          )}
        </div>
      </div>

      {/* Actions */}
      <div
        className="flex items-center justify-between gap-3 shrink-0"
        style={{ padding: "12px 22px", borderTop: "1px solid var(--color-border)" }}
      >
        <span className="text-xs truncate" style={{ color: error ? "#ef4444" : "transparent" }}>
          {error ?? ""}
        </span>
        <div className="flex gap-2 shrink-0">
          <button
            onClick={close}
            className="text-sm transition-colors hover:opacity-80"
            style={{
              color: "var(--color-text-secondary)",
              padding: "6px 16px",
              borderRadius: "8px",
              backgroundColor: "var(--color-surface)",
              border: "none",
              cursor: "pointer",
            }}
          >
            取消
          </button>
          <button
            onClick={save}
            className="text-sm font-medium text-white transition-colors hover:opacity-90"
            style={{
              backgroundColor: "var(--color-primary)",
              padding: "6px 16px",
              borderRadius: "8px",
              border: "none",
              cursor: "pointer",
            }}
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

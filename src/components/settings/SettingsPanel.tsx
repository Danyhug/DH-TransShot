import { useState, useEffect, useCallback } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, saveSettings } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import { defaultSettings } from "../../stores/settingsStore";
import type { Settings, ServiceConfig } from "../../types";

type TabName = "translation" | "ocr" | "tts";

const tabs: { key: TabName; label: string }[] = [
  { key: "translation", label: "翻译" },
  { key: "ocr", label: "OCR" },
  { key: "tts", label: "TTS" },
];

function ServiceFields({
  config,
  onChange,
}: {
  config: ServiceConfig;
  onChange: (key: keyof ServiceConfig, value: string) => void;
}) {
  const inputStyle = {
    backgroundColor: "var(--color-surface)",
    color: "var(--color-text)",
    borderRadius: "8px",
    padding: "8px 10px",
    marginTop: "4px",
    border: "none",
  };

  return (
    <div className="space-y-2">
      <label className="block">
        <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
          模型
        </span>
        <input
          type="text"
          value={config.model}
          onChange={(e) => onChange("model", e.target.value)}
          className="w-full text-sm outline-none"
          style={inputStyle}
          placeholder="gpt-4o-mini"
        />
      </label>
      <label className="block">
        <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
          自定义参数
        </span>
        <textarea
          value={config.extra}
          onChange={(e) => onChange("extra", e.target.value)}
          className="w-full text-sm outline-none resize-none"
          style={{ ...inputStyle, minHeight: "56px" }}
          placeholder='{"temperature": 0.3}'
          rows={2}
        />
      </label>
    </div>
  );
}

export function SettingsPanel() {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [activeTab, setActiveTab] = useState<TabName>("translation");

  useEffect(() => {
    appLog.info("[Settings] 设置窗口: 加载配置...");
    getSettings()
      .then((s) => {
        appLog.info("[Settings] 设置窗口: 配置加载成功, translation.model=" + s.translation.model);
        setSettings(s);
      })
      .catch((e) => appLog.error("[Settings] 设置窗口: 配置加载失败: " + String(e)));
  }, []);

  const updateService = useCallback((service: TabName, key: keyof ServiceConfig, value: string) => {
    setSettings((prev) => ({
      ...prev,
      [service]: { ...prev[service], [key]: value },
    }));
  }, []);

  const save = useCallback(async () => {
    try {
      appLog.info("[Settings] 保存配置, translation.model=" + settings.translation.model + ", ocr.model=" + settings.ocr.model);
      await saveSettings(settings);
      appLog.info("[Settings] 配置保存成功");
      await emit("settings-saved");
      await getCurrentWindow().close();
    } catch (e) {
      appLog.error("[Settings] 配置保存失败: " + String(e));
    }
  }, [settings]);

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
          设置
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

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-5 pb-4" style={{ minHeight: 0 }}>
        {/* Global API fields */}
        <div className="space-y-2" style={{ marginBottom: "14px" }}>
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              API 地址
            </span>
            <input
              type="text"
              value={settings.base_url}
              onChange={(e) =>
                setSettings((prev) => ({ ...prev, base_url: e.target.value }))
              }
              className="w-full text-sm outline-none"
              style={{
                backgroundColor: "var(--color-surface)",
                color: "var(--color-text)",
                borderRadius: "8px",
                padding: "8px 10px",
                marginTop: "4px",
                border: "none",
              }}
              placeholder="https://api.openai.com"
            />
          </label>
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              API 密钥
            </span>
            <input
              type="password"
              value={settings.api_key}
              onChange={(e) =>
                setSettings((prev) => ({ ...prev, api_key: e.target.value }))
              }
              className="w-full text-sm outline-none"
              style={{
                backgroundColor: "var(--color-surface)",
                color: "var(--color-text)",
                borderRadius: "8px",
                padding: "8px 10px",
                marginTop: "4px",
                border: "none",
              }}
              placeholder="sk-..."
            />
          </label>
        </div>

        {/* Tabs */}
        <div className="flex gap-1" style={{ marginBottom: "10px" }}>
          {tabs.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setActiveTab(tab.key)}
              className="text-xs font-medium transition-colors"
              style={{
                padding: "5px 12px",
                borderRadius: "6px",
                border: "none",
                cursor: "pointer",
                backgroundColor:
                  activeTab === tab.key ? "var(--color-primary)" : "var(--color-surface)",
                color: activeTab === tab.key ? "#fff" : "var(--color-text-secondary)",
              }}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Service config for active tab */}
        <ServiceFields
          config={settings[activeTab]}
          onChange={(key, value) => updateService(activeTab, key, value)}
        />

        {/* Hide on capture toggle */}
        <div style={{ marginTop: "14px" }}>
          <label className="flex items-center justify-between">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              截图时隐藏主界面
            </span>
            <input
              type="checkbox"
              checked={settings.hide_on_capture}
              onChange={(e) =>
                setSettings((prev) => ({ ...prev, hide_on_capture: e.target.checked }))
              }
            />
          </label>
        </div>

        {/* Hotkeys info */}
        <div style={{ marginTop: "14px" }}>
          <h3 className="text-xs font-medium" style={{ color: "var(--color-text-secondary)", marginBottom: "6px" }}>
            快捷键
          </h3>
          <div className="space-y-0.5 text-xs" style={{ color: "var(--color-text-secondary)" }}>
            <div className="flex justify-between">
              <span>区域截图</span>
              <span style={{ color: "var(--color-text)" }}>⌥A</span>
            </div>
            <div className="flex justify-between">
              <span>区域翻译</span>
              <span style={{ color: "var(--color-text)" }}>⌥S</span>
            </div>
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="flex justify-end gap-2 shrink-0" style={{ padding: "14px 20px 16px" }}>
        <button
          onClick={close}
          className="text-sm transition-colors hover:opacity-80"
          style={{
            color: "var(--color-text-secondary)",
            padding: "6px 14px",
            borderRadius: "8px",
            backgroundColor: "var(--color-surface)",
            border: "none",
          }}
        >
          取消
        </button>
        <button
          onClick={save}
          className="text-sm font-medium text-white transition-colors hover:opacity-90"
          style={{
            backgroundColor: "var(--color-primary)",
            padding: "6px 14px",
            borderRadius: "8px",
            border: "none",
          }}
        >
          保存
        </button>
      </div>
    </div>
  );
}

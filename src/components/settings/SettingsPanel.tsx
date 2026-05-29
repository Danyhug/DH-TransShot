import { useState, useEffect, useCallback } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, saveSettings, suspendHotkeys, resumeHotkeys } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import { defaultSettings } from "../../stores/settingsStore";
import { HotkeyInput } from "./HotkeyInput";
import type { Settings, ServiceConfig, ExtraProvider, HotkeyConfig } from "../../types";

type TabName = "translation" | "ocr" | "tts";

const tabs: { key: TabName; label: string }[] = [
  { key: "translation", label: "翻译" },
  { key: "ocr", label: "OCR" },
  { key: "tts", label: "TTS" },
];

const extraParamPresets: Record<TabName, { key: string; label: string; defaultValue: string; tooltip: string }[]> = {
  translation: [
    { key: "temperature", label: "temperature", defaultValue: "0.3", tooltip: "平衡创造性与可靠性，越低越稳定精确，越高越发散多样 (0~2)" },
    { key: "top_p", label: "top_p", defaultValue: "0.9", tooltip: "核采样，只从概率累计前 90% 的词中选，越低回复越固定 (0~1)" },
    { key: "max_tokens", label: "max_tokens", defaultValue: "4096", tooltip: "单次回复最大长度，太小会被截断，建议留足输入空间" },
    { key: "frequency_penalty", label: "frequency_penalty", defaultValue: "0", tooltip: "抑制重复用词，越高越不容易来回说同一个词 (-2.0~2.0)" },
    { key: "presence_penalty", label: "presence_penalty", defaultValue: "0", tooltip: "鼓励新话题，越高越倾向引入新内容而不是反复提旧的 (-2.0~2.0)" },
  ],
  ocr: [
    { key: "temperature", label: "temperature", defaultValue: "0.1", tooltip: "平衡创造性与可靠性，OCR 识别建议设低以保证准确 (0~2)" },
    { key: "top_p", label: "top_p", defaultValue: "0.9", tooltip: "核采样，只从概率累计前 90% 的词中选，越低回复越固定 (0~1)" },
    { key: "max_tokens", label: "max_tokens", defaultValue: "4096", tooltip: "单次回复最大长度，太小会被截断，建议留足输入空间" },
    { key: "frequency_penalty", label: "frequency_penalty", defaultValue: "0", tooltip: "抑制重复用词，越高越不容易来回说同一个词 (-2.0~2.0)" },
    { key: "presence_penalty", label: "presence_penalty", defaultValue: "0", tooltip: "鼓励新话题，越高越倾向引入新内容而不是反复提旧的 (-2.0~2.0)" },
  ],
  tts: [
    { key: "voice", label: "voice", defaultValue: "", tooltip: "发音人音色，格式为「模型名:音色名」，如 FunAudioLLM/CosyVoice2-0.5B:alex" },
    { key: "speed", label: "speed", defaultValue: "1.0", tooltip: "语速，1.0 为正常，2.0 倍速，最小 0.25，最大 4.0" },
    { key: "gain", label: "gain", defaultValue: "0.0", tooltip: "音量增益 (dB)，0 为原始音量，正数加大，负数减小 (-10~10)" },
    { key: "response_format", label: "format", defaultValue: "mp3", tooltip: "音频输出格式，mp3 体积小，wav 无损，opus 适合流式" },
    { key: "sample_rate", label: "sample_rate", defaultValue: "48000", tooltip: "采样率 (Hz)，越高音质越好，opus 格式仅支持 48000" },
  ],
};

function ServiceFields({
  config,
  activeTab,
  onChange,
  onProvidersChange,
  onActiveChange,
}: {
  config: ServiceConfig;
  activeTab: TabName;
  onChange: (key: "model" | "extra", value: string) => void;
  onProvidersChange: (providers: ExtraProvider[]) => void;
  onActiveChange: (active: number) => void;
}) {
  const inputStyle = {
    backgroundColor: "var(--color-surface)",
    color: "var(--color-text)",
    borderRadius: "8px",
    padding: "8px 10px",
    marginTop: "4px",
    border: "none",
  };

  const isDefault = config.active < 0;
  const activeProvider =
    !isDefault && config.providers[config.active] ? config.providers[config.active] : null;

  const updateActiveProvider = (key: keyof ExtraProvider, value: string) => {
    if (isDefault) return;
    const idx = config.active;
    const next = config.providers.map((p, i) => (i === idx ? { ...p, [key]: value } : p));
    onProvidersChange(next);
  };

  const addProvider = () => {
    const next: ExtraProvider[] = [
      ...config.providers,
      { name: `提供商 ${config.providers.length + 1}`, base_url: "", api_key: "", model: "" },
    ];
    onProvidersChange(next);
    onActiveChange(next.length - 1);
  };

  const removeActiveProvider = () => {
    if (isDefault) return;
    const idx = config.active;
    const next = config.providers.filter((_, i) => i !== idx);
    onProvidersChange(next);
    onActiveChange(-1);
  };

  return (
    <div className="space-y-2">
      {/* Provider chips */}
      <div>
        <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
          模型提供商
        </span>
        <div className="flex flex-wrap items-center gap-1.5 mt-1">
          <button
            onClick={() => onActiveChange(-1)}
            className="text-xs transition-colors"
            style={{
              padding: "3px 10px",
              borderRadius: "9999px",
              border: "none",
              cursor: "pointer",
              backgroundColor: isDefault ? "var(--color-primary)" : "var(--color-surface)",
              color: isDefault ? "#fff" : "var(--color-text-secondary)",
            }}
          >
            默认
          </button>
          {config.providers.map((p, i) => (
            <button
              key={i}
              onClick={() => onActiveChange(i)}
              className="text-xs transition-colors"
              style={{
                padding: "3px 10px",
                borderRadius: "9999px",
                border: "none",
                cursor: "pointer",
                backgroundColor: config.active === i ? "var(--color-primary)" : "var(--color-surface)",
                color: config.active === i ? "#fff" : "var(--color-text-secondary)",
              }}
            >
              {p.name?.trim() || `提供商 ${i + 1}`}
            </button>
          ))}
          <button
            onClick={addProvider}
            title="添加提供商"
            className="text-xs transition-colors"
            style={{
              padding: "3px 10px",
              borderRadius: "9999px",
              border: "1px dashed var(--color-text-secondary)",
              cursor: "pointer",
              backgroundColor: "transparent",
              color: "var(--color-text-secondary)",
              opacity: 0.7,
            }}
          >
            + 新增
          </button>
        </div>
      </div>

      {/* Provider-specific fields */}
      {isDefault ? (
        <label className="block">
          <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
            模型（使用顶部 API 地址 / API 密钥）
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
      ) : activeProvider ? (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              名称
            </span>
            <input
              type="text"
              value={activeProvider.name}
              onChange={(e) => updateActiveProvider("name", e.target.value)}
              className="w-full text-sm outline-none"
              style={inputStyle}
              placeholder="OpenAI"
            />
          </label>
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              API 地址（留空则用顶部全局）
            </span>
            <input
              type="text"
              value={activeProvider.base_url}
              onChange={(e) => updateActiveProvider("base_url", e.target.value)}
              className="w-full text-sm outline-none"
              style={inputStyle}
              placeholder="https://api.openai.com"
            />
          </label>
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              API 密钥（留空则用顶部全局）
            </span>
            <input
              type="password"
              value={activeProvider.api_key}
              onChange={(e) => updateActiveProvider("api_key", e.target.value)}
              className="w-full text-sm outline-none"
              style={inputStyle}
              placeholder="sk-..."
            />
          </label>
          <label className="block">
            <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
              模型
            </span>
            <input
              type="text"
              value={activeProvider.model}
              onChange={(e) => updateActiveProvider("model", e.target.value)}
              className="w-full text-sm outline-none"
              style={inputStyle}
              placeholder="gpt-4o-mini"
            />
          </label>
          <button
            onClick={removeActiveProvider}
            className="text-xs transition-colors"
            style={{
              padding: "4px 10px",
              borderRadius: "8px",
              border: "none",
              cursor: "pointer",
              backgroundColor: "var(--color-surface)",
              color: "#ef4444",
            }}
          >
            删除此提供商
          </button>
        </div>
      ) : null}

      {/* Shared extra params */}
      <label className="block">
        <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
          自定义参数（所有提供商共享）
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
      <div className="flex flex-wrap gap-1.5 mt-1">
        {extraParamPresets[activeTab].map((preset) => {
          let existingKeys: string[] = [];
          try {
            const parsed = JSON.parse(config.extra);
            if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
              existingKeys = Object.keys(parsed);
            }
          } catch { /* ignore */ }
          const alreadyAdded = existingKeys.includes(preset.key);
          return (
            <button
              key={preset.key}
              disabled={alreadyAdded}
              title={preset.tooltip}
              onClick={() => {
                let obj: Record<string, unknown> = {};
                try {
                  const parsed = JSON.parse(config.extra);
                  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
                    obj = parsed;
                  }
                } catch { /* start fresh */ }
                if (preset.key in obj) return;
                let val: string | number = preset.defaultValue;
                if (preset.key === "voice" && !val) {
                  val = config.model ? `${config.model}:` : "";
                }
                const num = Number(val);
                obj[preset.key] = val !== "" && !isNaN(num) ? num : val;
                onChange("extra", JSON.stringify(obj, null, 2));
              }}
              className="text-xs transition-colors"
              style={{
                padding: "2px 8px",
                borderRadius: "9999px",
                border: "none",
                cursor: alreadyAdded ? "not-allowed" : "pointer",
                backgroundColor: "var(--color-surface)",
                color: "var(--color-text-secondary)",
                opacity: alreadyAdded ? 0.4 : 1,
              }}
            >
              {preset.label}
            </button>
          );
        })}
      </div>
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

  // 设置面板期间挂起全局快捷键，避免录入新组合时被系统拦截
  useEffect(() => {
    suspendHotkeys().catch((e) => appLog.warn("[Settings] suspend_hotkeys 失败: " + String(e)));
    return () => {
      resumeHotkeys().catch((e) => appLog.warn("[Settings] resume_hotkeys 失败: " + String(e)));
    };
  }, []);

  const updateService = useCallback((service: TabName, key: "model" | "extra", value: string) => {
    setSettings((prev) => ({
      ...prev,
      [service]: { ...prev[service], [key]: value },
    }));
  }, []);

  const updateProviders = useCallback((service: TabName, providers: ExtraProvider[]) => {
    setSettings((prev) => ({
      ...prev,
      [service]: { ...prev[service], providers },
    }));
  }, []);

  const updateActive = useCallback((service: TabName, active: number) => {
    setSettings((prev) => ({
      ...prev,
      [service]: { ...prev[service], active },
    }));
  }, []);

  const save = useCallback(async () => {
    const hk = settings.hotkeys;
    if (!hk?.screenshot?.trim() || !hk?.ocr_translate?.trim() || !hk?.clipboard_translate?.trim()) {
      appLog.warn("[Settings] 快捷键不能为空");
      alert("快捷键不能为空，请为三个动作都设置快捷键");
      return;
    }
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
          activeTab={activeTab}
          onChange={(key, value) => updateService(activeTab, key, value)}
          onProvidersChange={(providers) => updateProviders(activeTab, providers)}
          onActiveChange={(active) => updateActive(activeTab, active)}
        />

        {/* Hotkeys */}
        <div style={{ marginTop: "14px" }}>
          <h3 className="text-xs font-medium" style={{ color: "var(--color-text-secondary)", marginBottom: "6px" }}>
            快捷键
          </h3>
          <div className="space-y-1.5">
            {([
              { key: "screenshot", label: "区域截图" },
              { key: "ocr_translate", label: "区域翻译" },
              { key: "clipboard_translate", label: "翻译选中文本" },
            ] as { key: keyof HotkeyConfig; label: string }[]).map(({ key, label }) => (
              <div key={key} className="flex items-center justify-between gap-2">
                <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
                  {label}
                </span>
                <HotkeyInput
                  value={settings.hotkeys?.[key] ?? ""}
                  onChange={(v) =>
                    setSettings((prev) => ({
                      ...prev,
                      hotkeys: { ...prev.hotkeys, [key]: v },
                    }))
                  }
                />
              </div>
            ))}
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

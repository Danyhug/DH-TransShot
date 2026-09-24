import { create } from "zustand";
import { openCenteredWindow } from "../lib/windowUtils";
import type { Settings, ServiceConfig } from "../types";

type ServiceName = "translation" | "ocr" | "tts";

interface SettingsState {
  settings: Settings;
  setSettings: (settings: Settings) => void;
  updateService: (service: ServiceName, key: keyof ServiceConfig, value: string) => void;
}

/**
 * 与后端 `Settings::default()` 一一对应，改一边必须同步另一边。
 *
 * extra 的取值依据（硅基流动官方 API 文档，2026-08 核对）：`top_p` / `max_tokens` /
 * TTS 那几个跟随平台默认；`temperature`（平台 0.7）与 `enable_thinking`（平台 true）
 * 是刻意偏离——翻译/OCR 要的是稳定输出，不需要思维链。
 */
export const defaultSettings: Settings = {
  base_url: "",
  api_key: "",
  translation: {
    model: "tencent/Hunyuan-MT-7B",
    extra: `{
  "temperature": 0.3,
  "top_p": 0.7,
  "max_tokens": 4096,
  "enable_thinking": false
}`,
    providers: [],
    active: -1,
  },
  ocr: {
    model: "Qwen/Qwen3.5-4B",
    extra: `{
  "temperature": 0.1,
  "top_p": 0.7,
  "max_tokens": 4096,
  "enable_thinking": false
}`,
    providers: [],
    active: -1,
  },
  tts: {
    model: "FunAudioLLM/CosyVoice2-0.5B",
    extra: `{
  "voice": "FunAudioLLM/CosyVoice2-0.5B:alex",
  "speed": 1.0,
  "response_format": "mp3",
  "sample_rate": 44100
}`,
    providers: [],
    active: -1,
  },
  hotkeys: {
    screenshot: "Alt+A",
    ocr_translate: "Alt+S",
    clipboard_translate: "Alt+Q",
  },
  speech: {
    auto_read_source: false,
    auto_read_target: false,
    auto_read_max_units: 0,
    stream_playback: true,
  },
  translation_prompt: {
    expand_abbreviations: false,
    domains: [],
    custom_prompt: "",
  },
};

/**
 * Resolve the currently-active (base_url, api_key, model, extra) for a given service.
 * `active < 0` or out-of-range falls back to the default (global creds + svc.model/extra).
 * For extra providers, blank fields fall back to the shared ones.
 * 与后端 `ServiceConfig::resolved` 保持一致（前端 TTS 缓存键依赖它）。
 */
export function resolveActiveProvider(
  settings: Settings,
  service: ServiceName,
): { base_url: string; api_key: string; model: string; extra: string } {
  const svc = settings[service];
  const fallback = {
    base_url: settings.base_url,
    api_key: settings.api_key,
    model: svc.model,
    extra: svc.extra,
  };
  if (svc.active < 0) return fallback;
  const p = svc.providers[svc.active];
  if (!p) return fallback;
  return {
    base_url: p.base_url.trim() ? p.base_url : settings.base_url,
    api_key: p.api_key.trim() ? p.api_key : settings.api_key,
    model: p.model.trim() ? p.model : svc.model,
    extra: p.extra?.trim() ? p.extra : svc.extra,
  };
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: defaultSettings,
  setSettings: (settings) => set({ settings }),
  updateService: (service, key, value) => {
    const { settings } = get();
    set({
      settings: {
        ...settings,
        [service]: { ...settings[service], [key]: value },
      },
    });
  },
}));

/** Open (or focus) the settings window (standalone, centered) */
export async function openSettingsWindow() {
  await openCenteredWindow({
    label: "settings",
    url: "settings.html",
    title: "设置",
    width: 720,
    height: 540,
    minWidth: 640,
    minHeight: 440,
  });
}

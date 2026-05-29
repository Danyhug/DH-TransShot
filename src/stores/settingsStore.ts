import { create } from "zustand";
import { openDockedWindow } from "../lib/windowUtils";
import type { Settings, ServiceConfig } from "../types";

type ServiceName = "translation" | "ocr" | "tts";

interface SettingsState {
  settings: Settings;
  setSettings: (settings: Settings) => void;
  updateService: (service: ServiceName, key: keyof ServiceConfig, value: string) => void;
}

export const defaultSettings: Settings = {
  base_url: "",
  api_key: "",
  translation: {
    model: "tencent/Hunyuan-MT-7B",
    extra: `{
  "temperature": 0.3,
  "top_p": 0.9,
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
  "top_p": 0.9,
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
  "sample_rate": 44100,
  "enable_thinking": false
}`,
    providers: [],
    active: -1,
  },
  hotkeys: {
    screenshot: "Alt+A",
    ocr_translate: "Alt+S",
    clipboard_translate: "Alt+Q",
  },
};

/**
 * Resolve the currently-active (base_url, api_key, model) for a given service.
 * `active < 0` or out-of-range falls back to the default (global creds + svc.model).
 * For extra providers, blank fields fall back to the global ones.
 */
export function resolveActiveProvider(
  settings: Settings,
  service: ServiceName,
): { base_url: string; api_key: string; model: string } {
  const svc = settings[service];
  const fallback = {
    base_url: settings.base_url,
    api_key: settings.api_key,
    model: svc.model,
  };
  if (svc.active < 0) return fallback;
  const p = svc.providers[svc.active];
  if (!p) return fallback;
  return {
    base_url: p.base_url.trim() ? p.base_url : settings.base_url,
    api_key: p.api_key.trim() ? p.api_key : settings.api_key,
    model: p.model.trim() ? p.model : svc.model,
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

/** Open (or focus) the settings window, docked to the left of main window */
export async function openSettingsWindow() {
  await openDockedWindow({
    label: "settings",
    url: "settings.html",
    title: "设置",
    width: 340,
    side: "left",
  });
}

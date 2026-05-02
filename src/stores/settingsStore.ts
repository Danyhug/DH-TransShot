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
  "max_tokens": 4096
}`,
  },
  ocr: {
    model: "PaddlePaddle/PaddleOCR-VL-1.5",
    extra: `{
  "temperature": 0.1,
  "top_p": 0.9,
  "max_tokens": 4096
}`,
  },
  tts: {
    model: "FunAudioLLM/CosyVoice2-0.5B",
    extra: `{
  "voice": "FunAudioLLM/CosyVoice2-0.5B:alex",
  "speed": 1.0,
  "response_format": "mp3",
  "sample_rate": 44100
}`,
  },
  source_language: "auto",
  target_language: "zh-CN",
  hotkey_screenshot: "Alt+A",
  hotkey_region: "Alt+S",
};

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

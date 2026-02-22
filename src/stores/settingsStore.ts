import { create } from "zustand";
import { openDockedWindow } from "../lib/windowUtils";
import type { Settings, ServiceConfig } from "../types";

type ServiceName = "translation" | "ocr" | "tts";

interface SettingsState {
  settings: Settings;
  setSettings: (settings: Settings) => void;
  updateService: (service: ServiceName, key: keyof ServiceConfig, value: string) => void;
}

export const emptyService: ServiceConfig = {
  model: "",
  extra: "",
};

export const defaultSettings: Settings = {
  base_url: "",
  api_key: "",
  translation: { ...emptyService },
  ocr: { ...emptyService },
  tts: { ...emptyService },
  source_language: "auto",
  target_language: "zh-CN",
  hotkey_screenshot: "Alt+A",
  hotkey_region: "Alt+S",
  hide_on_capture: true,
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

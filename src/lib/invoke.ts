import { invoke } from "@tauri-apps/api/core";
import type { Settings, ScreenshotInitEvent } from "../types";

export async function startRegionSelect(mode: string): Promise<void> {
  return invoke("start_region_select", { mode });
}

export async function captureRegion(
  x: number,
  y: number,
  width: number,
  height: number
): Promise<string> {
  return invoke("capture_region", { x, y, width, height });
}

export async function getFrozenScreenshot(): Promise<ScreenshotInitEvent> {
  return invoke("get_frozen_screenshot");
}

export async function recognizeText(
  imageBase64: string,
  language: string
): Promise<string> {
  return invoke("recognize_text", { imageBase64, language });
}

export async function translateText(
  text: string,
  sourceLang: string,
  targetLang: string
): Promise<string> {
  return invoke("translate_text", { text, sourceLang, targetLang });
}

export async function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export async function saveSettings(settings: Settings): Promise<void> {
  return invoke("save_settings", { settings });
}

export async function readClipboard(): Promise<string> {
  return invoke("read_clipboard");
}

export async function copyImageToClipboard(imageBase64: string): Promise<void> {
  return invoke("copy_image_to_clipboard", { imageBase64 });
}

export async function synthesizeSpeech(text: string): Promise<string> {
  return invoke("synthesize_speech", { text });
}

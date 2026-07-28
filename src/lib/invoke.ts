import { invoke } from "@tauri-apps/api/core";
import type { Settings, ScreenshotInitEvent } from "../types";

export async function startRegionSelect(mode: string): Promise<void> {
  return invoke("start_region_select", { mode });
}

export async function captureRegion(
  monitorIndex: number,
  x: number,
  y: number,
  width: number,
  height: number
): Promise<string> {
  return invoke("capture_region", { monitorIndex, x, y, width, height });
}

export async function getFrozenScreenshot(monitorIndex: number): Promise<ScreenshotInitEvent> {
  return invoke("get_frozen_screenshot", { monitorIndex });
}

export async function captureAndOcr(
  monitorIndex: number,
  x: number,
  y: number,
  width: number,
  height: number,
  language: string
): Promise<string> {
  return invoke("capture_and_ocr", { monitorIndex, x, y, width, height, language });
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

export async function readSelectedText(): Promise<string> {
  return invoke("read_selected_text");
}

export async function copyImageToClipboard(imageBase64: string): Promise<void> {
  return invoke("copy_image_to_clipboard", { imageBase64 });
}

export async function saveFile(path: string, base64Data: string): Promise<void> {
  return invoke("save_file", { path, base64Data });
}

export async function synthesizeSpeech(text: string): Promise<string> {
  return invoke("synthesize_speech", { text });
}

export interface SpeechResponse {
  /** 完整音频 base64（流式为拼接后的 WAV） */
  audio: string;
  /** 本次推送的流式分块数量；0 表示直接播放 audio */
  chunkCount: number;
  /** 流式分块采样率 (Hz)，非流式为 0 */
  sampleRate: number;
}

/** 边收边播版本：分块通过 `tts-chunk` 事件推送（按 sessionId 过滤）。 */
export async function synthesizeSpeechStream(
  text: string,
  sessionId: string
): Promise<SpeechResponse> {
  return invoke("synthesize_speech_stream", { text, sessionId });
}

export async function suspendHotkeys(): Promise<void> {
  return invoke("suspend_hotkeys");
}

export async function resumeHotkeys(): Promise<void> {
  return invoke("resume_hotkeys");
}

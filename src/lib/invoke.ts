import { Channel, invoke } from "@tauri-apps/api/core";
import type { Settings, ScreenshotInitEvent } from "../types";

export async function startRegionSelect(mode: string): Promise<void> {
  return invoke("start_region_select", { mode });
}

/**
 * 关闭窗口（后端先 `hide()` 让帧，再 `close()`）。
 *
 * 不要改用 `getCurrentWindow().close()`：在 display link 刷新过程中直接销毁还活着的
 * webview，会让 WebKit 访问已经释放的滚动树，偶发把整个 App 打崩（`EXC_BAD_ACCESS`，
 * 崩溃栈里没有本项目的帧）。详见后端 `src-tauri/src/window_lifecycle.rs`。
 */
export async function closeWindowDeferred(label: string): Promise<void> {
  return invoke("close_window_deferred", { label });
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

/** 内置翻译规则原文（后端 `translation::prompt::DEFAULT_RULES`），设置里展示和「恢复默认」用 */
export async function getDefaultTranslationPrompt(): Promise<string> {
  return invoke("get_default_translation_prompt");
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

/**
 * 播放状态消息。音频本身**不过 IPC**——由 Rust 侧直接送进输出设备，
 * 这里只剩「已经出声了」这一个信号（前端据此熄灭加载态）。
 */
export type TtsPlaybackMessage = { event: "start" };

/**
 * 朗读一段文本：后端合成 + 本地播放，**播完才 resolve**。
 *
 * 调用即抢占上一段朗读，不需要先调 `stopSpeech()`——两个 invoke 谁先到达没有保证，
 * 先停后播反而可能把新的这段停掉。
 */
export async function speakText(
  text: string,
  onEvent: Channel<TtsPlaybackMessage>
): Promise<void> {
  return invoke("speak_text", { text, onEvent });
}

/** 停止当前朗读。 */
export async function stopSpeech(): Promise<void> {
  return invoke("stop_speech");
}

export async function suspendHotkeys(): Promise<void> {
  return invoke("suspend_hotkeys");
}

export async function resumeHotkeys(): Promise<void> {
  return invoke("resume_hotkeys");
}

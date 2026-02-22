export interface Settings {
  base_url: string;
  api_key: string;
  translation: ServiceConfig;
  ocr: ServiceConfig;
  tts: ServiceConfig;
  source_language: string;
  target_language: string;
  hotkey_screenshot: string;
  hotkey_region: string;
  hide_on_capture: boolean;
}

export interface ServiceConfig {
  model: string;
  extra: string;
}

export interface RegionSelectEvent {
  x: number;
  y: number;
  width: number;
  height: number;
  mode: string;
}

export interface WindowRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ScreenshotInitEvent {
  image: string;
  mode: string;
  window_rects: WindowRect[];
}

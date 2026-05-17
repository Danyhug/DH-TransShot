export interface Settings {
  base_url: string;
  api_key: string;
  translation: ServiceConfig;
  ocr: ServiceConfig;
  tts: ServiceConfig;
  hotkeys: HotkeyConfig;
}

export interface ServiceConfig {
  model: string;
  extra: string;
}

export interface HotkeyConfig {
  screenshot: string;
  ocr_translate: string;
  clipboard_translate: string;
}

export interface RegionSelectEvent {
  x: number;
  y: number;
  width: number;
  height: number;
  mode: string;
  monitor_index: number;
  annotatedImage?: string;
}

export interface WindowRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface MonitorInfo {
  name: string;
  x: number;      // physical pixel position
  y: number;       // physical pixel position
  width: number;   // physical pixel size
  height: number;  // physical pixel size
  scale_factor: number;
}

export interface ScreenshotInitEvent {
  image: string;
  mode: string;
  window_rects: WindowRect[];
  monitors: MonitorInfo[];
}

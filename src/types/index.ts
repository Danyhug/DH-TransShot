export interface Settings {
  base_url: string;
  api_key: string;
  translation: ServiceConfig;
  ocr: ServiceConfig;
  tts: ServiceConfig;
  hotkeys: HotkeyConfig;
  speech: SpeechConfig;
  translation_prompt: TranslationPromptConfig;
}

/** 翻译提示词偏好（对应后端 `config::TranslationPromptConfig`，由 `translation::prompt` 拼进 system prompt） */
export interface TranslationPromptConfig {
  /** 解释缩写与标识符：单独输入缩写时输出「全称 + 含义」候选；单独输入标识符/路径（如 stores/settingsStore）时按词段译出含义；句中缩写按上下文译出含义 */
  expand_abbreviations: boolean;
  /** 行业偏向（多选，key 见 TranslationSettings 的 DOMAINS）；空 = 通用 */
  domains: string[];
  /** 自定义附加翻译指令，原样追加到 system prompt */
  custom_prompt: string;
  /** 编辑过的内置翻译规则；空串 = 使用后端内置默认规则 */
  base_prompt: string;
}

export interface SpeechConfig {
  /** 翻译完成后自动朗读原文 */
  auto_read_source: boolean;
  /** 翻译完成后自动朗读译文 */
  auto_read_target: boolean;
  /** 自动朗读的长度上限（中文按字、西文按单词计数）；0 表示不限制，手动朗读不受限 */
  auto_read_max_units: number;
  /** 边收边播：chat+audio 流式分块实时播放（关闭则等整段合成完再播） */
  stream_playback: boolean;
}

export interface ExtraProvider {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  /** 该提供商专用的自定义参数（JSON 字符串）；留空则继承 ServiceConfig.extra，填写则整体覆盖 */
  extra: string;
}

export interface ServiceConfig {
  model: string;
  /** 共享自定义参数：默认提供商 + 所有未单独填写 extra 的提供商都用它 */
  extra: string;
  providers: ExtraProvider[];
  /** -1 = 默认（使用顶层 base_url/api_key + model）；0+ = providers 索引 */
  active: number;
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

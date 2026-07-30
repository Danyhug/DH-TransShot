import {
  Chip,
  CodeArea,
  Divider,
  Field,
  Group,
  InfoNote,
  SectionHeader,
  SegmentedControl,
  TextInput,
} from "./controls";
import type { ExtraProvider, ServiceConfig, Settings } from "../../types";

export type ServiceName = "translation" | "ocr" | "tts";

const serviceTabs: { key: ServiceName; label: string }[] = [
  { key: "translation", label: "翻译" },
  { key: "ocr", label: "OCR" },
  { key: "tts", label: "TTS" },
];

interface ExtraParamPreset {
  key: string;
  label: string;
  defaultValue: string;
  tooltip: string;
}

const chatModelPresets = (temperatureDefault: string, temperatureTip: string): ExtraParamPreset[] => [
  { key: "temperature", label: "temperature", defaultValue: temperatureDefault, tooltip: temperatureTip },
  { key: "top_p", label: "top_p", defaultValue: "0.9", tooltip: "核采样，只从概率累计前 90% 的词中选，越低回复越固定 (0~1)" },
  { key: "max_tokens", label: "max_tokens", defaultValue: "4096", tooltip: "单次回复最大长度，太小会被截断，建议留足输入空间" },
  { key: "frequency_penalty", label: "frequency_penalty", defaultValue: "0", tooltip: "抑制重复用词，越高越不容易来回说同一个词 (-2.0~2.0)" },
  { key: "presence_penalty", label: "presence_penalty", defaultValue: "0", tooltip: "鼓励新话题，越高越倾向引入新内容而不是反复提旧的 (-2.0~2.0)" },
];

const extraParamPresets: Record<ServiceName, ExtraParamPreset[]> = {
  translation: chatModelPresets("0.3", "平衡创造性与可靠性，越低越稳定精确，越高越发散多样 (0~2)"),
  ocr: chatModelPresets("0.1", "平衡创造性与可靠性，OCR 识别建议设低以保证准确 (0~2)"),
  tts: [
    { key: "voice", label: "voice", defaultValue: "", tooltip: "音色。audio/speech 协议格式为「模型名:音色名」（如 FunAudioLLM/CosyVoice2-0.5B:alex）；小米 MiMo chat+audio 填裸名字（如 Milo、冰糖）" },
    { key: "speed", label: "speed", defaultValue: "1.0", tooltip: "语速（audio/speech 协议），1.0 为正常，2.0 倍速，最小 0.25，最大 4.0" },
    { key: "gain", label: "gain", defaultValue: "0.0", tooltip: "音量增益 dB（audio/speech 协议），0 为原始音量 (-10~10)" },
    { key: "response_format", label: "format", defaultValue: "mp3", tooltip: "audio/speech 输出格式，mp3 体积小，wav 无损，opus 适合流式" },
    { key: "sample_rate", label: "sample_rate", defaultValue: "48000", tooltip: "采样率 Hz（audio/speech 协议），越高音质越好，opus 仅支持 48000" },
    { key: "style", label: "style", defaultValue: "用自然、平稳、清晰的语气朗读。", tooltip: "小米 MiMo chat+audio 风格指令（user 消息），可描述语气/情感/角色；置空则不发送" },
    { key: "stream", label: "stream", defaultValue: "true", tooltip: "小米 MiMo chat+audio 是否流式合成（边收边播依赖它），默认 true" },
  ],
};

/** 解析 extra JSON，失败时按空对象处理（用户可能正在编辑到一半）。 */
function parseExtra(extra: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(extra);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    /* 编辑中的非法 JSON，当作空对象 */
  }
  return {};
}

const BASE_URL_TITLE =
  "填根地址或以 /v1 结尾自动补全端点；已是完整端点原样；结尾加 # 按填写内容原样请求";

function ProviderEditor({
  config,
  onChange,
  onProvidersChange,
  onActiveChange,
}: {
  config: ServiceConfig;
  onChange: (key: "model" | "extra", value: string) => void;
  onProvidersChange: (providers: ExtraProvider[]) => void;
  onActiveChange: (active: number) => void;
}) {
  const isDefault = config.active < 0;
  const activeProvider =
    !isDefault && config.providers[config.active] ? config.providers[config.active] : null;

  const updateActiveProvider = (key: keyof ExtraProvider, value: string) => {
    if (isDefault) return;
    const idx = config.active;
    onProvidersChange(config.providers.map((p, i) => (i === idx ? { ...p, [key]: value } : p)));
  };

  const addProvider = () => {
    const next: ExtraProvider[] = [
      ...config.providers,
      { name: `提供商 ${config.providers.length + 1}`, base_url: "", api_key: "", model: "" },
    ];
    onProvidersChange(next);
    onActiveChange(next.length - 1);
  };

  const removeActiveProvider = () => {
    if (isDefault) return;
    const idx = config.active;
    onProvidersChange(config.providers.filter((_, i) => i !== idx));
    onActiveChange(-1);
  };

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center gap-1.5">
        <span
          className="text-xs font-medium"
          style={{ color: "var(--color-text-secondary)", marginRight: "4px" }}
        >
          提供商
        </span>
        <Chip active={isDefault} onClick={() => onActiveChange(-1)}>
          默认
        </Chip>
        {config.providers.map((p, i) => (
          <Chip key={i} active={config.active === i} onClick={() => onActiveChange(i)}>
            {p.name?.trim() || `提供商 ${i + 1}`}
          </Chip>
        ))}
        <Chip dashed title="添加提供商" onClick={addProvider}>
          ＋ 新增
        </Chip>
      </div>

      {isDefault ? (
        <Group>
          <Field label="模型" hint="使用上方「全局凭据」的 API 地址与密钥">
            <TextInput
              type="text"
              value={config.model}
              onChange={(e) => onChange("model", e.target.value)}
              placeholder="gpt-4o-mini"
            />
          </Field>
        </Group>
      ) : activeProvider ? (
        <Group>
          <div className="grid grid-cols-2 gap-3">
            <Field label="名称">
              <TextInput
                type="text"
                value={activeProvider.name}
                onChange={(e) => updateActiveProvider("name", e.target.value)}
                placeholder="OpenAI"
              />
            </Field>
            <Field label="模型">
              <TextInput
                type="text"
                value={activeProvider.model}
                onChange={(e) => updateActiveProvider("model", e.target.value)}
                placeholder="gpt-4o-mini"
              />
            </Field>
            <Field label="API 地址">
              <TextInput
                type="text"
                value={activeProvider.base_url}
                onChange={(e) => updateActiveProvider("base_url", e.target.value)}
                placeholder="留空则用全局"
                title={BASE_URL_TITLE}
              />
            </Field>
            <Field label="API 密钥">
              <TextInput
                type="password"
                value={activeProvider.api_key}
                onChange={(e) => updateActiveProvider("api_key", e.target.value)}
                placeholder="留空则用全局"
              />
            </Field>
          </div>
          <div className="flex justify-end" style={{ marginTop: "10px" }}>
            <button
              type="button"
              onClick={removeActiveProvider}
              className="text-xs transition-opacity hover:opacity-80"
              style={{
                padding: "4px 10px",
                borderRadius: "8px",
                border: "none",
                cursor: "pointer",
                backgroundColor: "var(--color-surface)",
                color: "#ef4444",
              }}
            >
              删除此提供商
            </button>
          </div>
        </Group>
      ) : null}
    </div>
  );
}

function ExtraParams({
  config,
  service,
  onChange,
}: {
  config: ServiceConfig;
  service: ServiceName;
  onChange: (key: "model" | "extra", value: string) => void;
}) {
  const existing = parseExtra(config.extra);

  const addPreset = (preset: ExtraParamPreset) => {
    const obj = parseExtra(config.extra);
    if (preset.key in obj) return;
    let val: string = preset.defaultValue;
    if (preset.key === "voice" && !val) {
      val = config.model ? `${config.model}:` : "";
    }
    if (val === "true" || val === "false") {
      obj[preset.key] = val === "true";
    } else {
      const num = Number(val);
      obj[preset.key] = val !== "" && !isNaN(num) ? num : val;
    }
    onChange("extra", JSON.stringify(obj, null, 2));
  };

  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-xs font-medium" style={{ color: "var(--color-text-secondary)" }}>
          自定义参数
        </span>
        <span className="text-xs" style={{ color: "var(--color-text-secondary)", opacity: 0.8 }}>
          所有提供商共享，随请求原样下发
        </span>
      </div>
      <div className="flex flex-wrap gap-1.5" style={{ margin: "8px 0" }}>
        {extraParamPresets[service].map((preset) => {
          const alreadyAdded = preset.key in existing;
          return (
            <Chip
              key={preset.key}
              disabled={alreadyAdded}
              title={preset.tooltip}
              onClick={() => addPreset(preset)}
            >
              ＋ {preset.label}
            </Chip>
          );
        })}
      </div>
      <CodeArea
        value={config.extra}
        onChange={(e) => onChange("extra", e.target.value)}
        placeholder='{"temperature": 0.3}'
        rows={5}
        style={{ minHeight: "104px" }}
      />
    </div>
  );
}

/** 「服务」分区：全局凭据 + 按服务切换的提供商 / 模型 / 自定义参数。 */
export function ServiceSettings({
  settings,
  activeService,
  onActiveServiceChange,
  onGlobalChange,
  onServiceChange,
  onProvidersChange,
  onActiveProviderChange,
}: {
  settings: Settings;
  activeService: ServiceName;
  onActiveServiceChange: (service: ServiceName) => void;
  onGlobalChange: (key: "base_url" | "api_key", value: string) => void;
  onServiceChange: (service: ServiceName, key: "model" | "extra", value: string) => void;
  onProvidersChange: (service: ServiceName, providers: ExtraProvider[]) => void;
  onActiveProviderChange: (service: ServiceName, active: number) => void;
}) {
  return (
    <div>
      <SectionHeader
        title="全局凭据"
        description="翻译 / OCR / TTS 默认都用这组地址和密钥，可在下方为单个服务单独覆盖。"
      />
      <div className="grid grid-cols-2 gap-3">
        <Field label="API 地址">
          <TextInput
            type="text"
            value={settings.base_url}
            onChange={(e) => onGlobalChange("base_url", e.target.value)}
            placeholder="https://api.openai.com"
            title={BASE_URL_TITLE}
          />
        </Field>
        <Field label="API 密钥">
          <TextInput
            type="password"
            value={settings.api_key}
            onChange={(e) => onGlobalChange("api_key", e.target.value)}
            placeholder="sk-..."
          />
        </Field>
      </div>
      <div style={{ marginTop: "10px" }}>
        <InfoNote>
          <span style={{ color: "var(--color-text)" }}>地址填写规则：</span>
          根地址或 <code>/v1</code> 结尾 → 自动补全端点（如 …/v1/chat/completions）；已是完整端点 →
          原样使用；结尾加 <code>#</code> → 完全按填写内容请求。
        </InfoNote>
      </div>

      <Divider />

      <SectionHeader title="服务配置" description="每个服务可以挂多个提供商，切换后立即生效。" />
      <SegmentedControl
        value={activeService}
        options={serviceTabs}
        onChange={onActiveServiceChange}
      />

      <div style={{ marginTop: "14px" }} className="flex flex-col gap-5">
        <ProviderEditor
          config={settings[activeService]}
          onChange={(key, value) => onServiceChange(activeService, key, value)}
          onProvidersChange={(providers) => onProvidersChange(activeService, providers)}
          onActiveChange={(active) => onActiveProviderChange(activeService, active)}
        />
        <ExtraParams
          config={settings[activeService]}
          service={activeService}
          onChange={(key, value) => onServiceChange(activeService, key, value)}
        />
      </div>
    </div>
  );
}

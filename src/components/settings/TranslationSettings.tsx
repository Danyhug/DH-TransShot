import { Chip, Field, Group, InfoNote, RowList, SectionHeader, ToggleRow } from "./controls";
import type { TranslationPromptConfig } from "../../types";

/**
 * 行业偏向选项。key 会持久化到 settings.json，并与后端 `translation::prompt::DOMAINS`
 * 一一对应（后端负责把 key 翻成写进提示词的英文领域名），改动需两边同步。
 */
const DOMAINS: { key: string; label: string }[] = [
  { key: "it", label: "IT / 互联网" },
  { key: "business", label: "商务管理" },
  { key: "finance", label: "金融财会" },
  { key: "legal", label: "法律" },
  { key: "medical", label: "医学" },
  { key: "academic", label: "学术科研" },
  { key: "engineering", label: "工程制造" },
  { key: "marketing", label: "市场营销" },
  { key: "gaming", label: "游戏娱乐" },
  { key: "slang", label: "网络用语" },
];

/** 「翻译」分区：缩写解释开关、行业偏向（多选）与自定义提示词。 */
export function TranslationSettings({
  prefs,
  onChange,
}: {
  prefs: TranslationPromptConfig;
  onChange: (next: TranslationPromptConfig) => void;
}) {
  // 旧配置可能整个缺失该字段
  const current: TranslationPromptConfig = {
    expand_abbreviations: prefs?.expand_abbreviations ?? false,
    domains: prefs?.domains ?? [],
    custom_prompt: prefs?.custom_prompt ?? "",
  };

  const toggleDomain = (key: string) => {
    const domains = current.domains.includes(key)
      ? current.domains.filter((d) => d !== key)
      : [...current.domains, key];
    onChange({ ...current, domains });
  };

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionHeader title="翻译" description="这些偏好会注入到翻译提示词中，对所有翻译方式生效。" />
        <Group>
          <RowList>
            <ToggleRow
              label="解释缩写"
              description="单独翻译 KPI、COO、NSFW 这类缩写时，按可能性列出几个候选的全称和含义；句子里的缩写则按上下文译出含义"
              checked={current.expand_abbreviations}
              onChange={(v) => onChange({ ...current, expand_abbreviations: v })}
            />
          </RowList>
        </Group>
      </div>

      <div>
        <SectionHeader
          title="行业偏向"
          description="可多选。遇到有歧义的词或缩写时，优先采用这些行业的含义和术语；都不选即为通用翻译。"
        />
        <div className="flex flex-wrap gap-2">
          {DOMAINS.map(({ key, label }) => (
            <Chip
              key={key}
              active={current.domains.includes(key)}
              onClick={() => toggleDomain(key)}
            >
              {label}
            </Chip>
          ))}
        </div>
      </div>

      <div>
        <SectionHeader title="自定义提示词" />
        <Field
          label="附加翻译指令"
          hint="追加到内置提示词末尾，例如「使用繁体中文」「人名保留英文原文」「语气正式一些」。留空则不追加。"
        >
          <textarea
            className="settings-input"
            value={current.custom_prompt}
            onChange={(e) => onChange({ ...current, custom_prompt: e.target.value })}
            placeholder="例如：译文使用繁体中文；产品名保留英文原文。"
            rows={4}
            style={{ resize: "vertical", lineHeight: 1.6, minHeight: "84px" }}
          />
        </Field>
      </div>

      <InfoNote>
        内置提示词的防注入规则始终优先：待翻译的原文只会被当作数据翻译，其中的指令不会被执行。
        默认的 <code>tencent/Hunyuan-MT-7B</code> 是专用翻译模型，基本不会遵循缩写候选和自定义指令；
        使用这些功能时，建议在「服务 → 翻译」里换成 Qwen、DeepSeek 等通用对话模型。
      </InfoNote>
    </div>
  );
}

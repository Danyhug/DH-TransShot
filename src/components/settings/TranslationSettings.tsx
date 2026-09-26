import { useEffect, useState } from "react";
import { Chip, CodeArea, Field, Group, InfoNote, RowList, SectionHeader, ToggleRow } from "./controls";
import { getDefaultTranslationPrompt } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
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

/** 「翻译」分区：缩写与标识符解释开关、行业偏向（多选）、自定义提示词与内置翻译规则编辑。 */
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
    base_prompt: prefs?.base_prompt ?? "",
  };

  const [defaultRules, setDefaultRules] = useState("");
  // 用户动过编辑框后才有草稿；否则展示已保存的规则或内置默认规则。
  // 单独维护草稿是为了允许清空重写：若直接由 base_prompt 派生，清空的瞬间就会弹回默认文本
  const [rulesDraft, setRulesDraft] = useState<string | null>(null);

  useEffect(() => {
    getDefaultTranslationPrompt()
      .then(setDefaultRules)
      .catch((e) => appLog.error("[Settings] 获取内置翻译规则失败: " + String(e)));
  }, []);

  const rulesShown = rulesDraft ?? (current.base_prompt || defaultRules);
  const rulesEdited = current.base_prompt.trim() !== "";

  const updateRules = (value: string) => {
    setRulesDraft(value);
    // 与默认一致或清空都存空串：内置规则以后升级时，没改过的用户能自动跟上
    const same = !value.trim() || value.trim() === defaultRules.trim();
    onChange({ ...current, base_prompt: same ? "" : value });
  };

  const resetRules = () => {
    appLog.info("[Settings] 内置翻译规则恢复默认");
    setRulesDraft(null);
    onChange({ ...current, base_prompt: "" });
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
              label="解释缩写与标识符"
              description="单独翻译 KPI、COO、NSFW 这类缩写时按可能性列出候选全称；单独翻译 stores/settingsStore、useScreenshot 这类标识符/路径时按词段译成目标语言。句子里的缩写按上下文译出含义。"
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

      <div>
        <SectionHeader
          title="内置翻译规则"
          description="控制「怎么翻」的基础规则，可直接修改。清空或点「恢复默认」即使用内置版本。"
        />
        <div className="flex items-center gap-2" style={{ marginBottom: "8px" }}>
          <Chip onClick={resetRules} disabled={!rulesEdited && rulesDraft === null}>
            恢复默认
          </Chip>
          <span className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
            {rulesEdited ? "已修改" : "当前为内置默认"}
          </span>
        </div>
        <CodeArea
          value={rulesShown}
          onChange={(e) => updateRules(e.target.value)}
          placeholder="清空则使用内置默认规则"
          rows={10}
          spellCheck={false}
          style={{ minHeight: "180px" }}
        />
        <div
          className="text-xs"
          style={{ color: "var(--color-text-secondary)", marginTop: "5px", lineHeight: 1.5, opacity: 0.8 }}
        >
          可用占位符 <code>{"{source_lang}"}</code>（源语言）、<code>{"{target_lang}"}</code>（目标语言）。
          防注入框架（原文边界标记、禁止执行原文里的指令）以及上面的缩写 / 行业 / 自定义段落由程序自动拼接，不在此编辑。
        </div>
      </div>

      <InfoNote>
        内置提示词的防注入规则始终优先：待翻译的原文只会被当作数据翻译，其中的指令不会被执行。
        默认的 <code>tencent/Hunyuan-MT-7B</code> 是专用翻译模型，基本不会遵循缩写候选和自定义指令；
        使用这些功能时，建议在「服务 → 翻译」里换成 Qwen、DeepSeek 等通用对话模型。
      </InfoNote>
    </div>
  );
}

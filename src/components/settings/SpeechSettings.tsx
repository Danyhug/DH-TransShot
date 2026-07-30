import { Group, InfoNote, NumberRow, RowList, SectionHeader, ToggleRow } from "./controls";
import type { Settings } from "../../types";

type SpeechKey = keyof Settings["speech"];
type SpeechToggleKey = "auto_read_source" | "auto_read_target" | "stream_playback";

/** 「朗读」分区：自动朗读、自动朗读长度上限与流式播放开关。 */
export function SpeechSettings({
  speech,
  onChange,
}: {
  speech: Settings["speech"];
  onChange: (key: SpeechKey, value: boolean | number) => void;
}) {
  // 旧配置可能缺字段：除 stream_playback 外都默认关
  const flag = (key: SpeechToggleKey) => speech?.[key] ?? key === "stream_playback";

  return (
    <div>
      <SectionHeader title="朗读" description="控制翻译完成后的自动朗读行为与音频播放方式。" />
      <Group>
        <RowList>
          <ToggleRow
            label="翻译后自动朗读原文"
            description="翻译完成后先朗读原文"
            checked={flag("auto_read_source")}
            onChange={(v) => onChange("auto_read_source", v)}
          />
          <ToggleRow
            label="翻译后自动朗读译文"
            description="与「朗读原文」可同时开启，此时先读原文再读译文"
            checked={flag("auto_read_target")}
            onChange={(v) => onChange("auto_read_target", v)}
          />
          <NumberRow
            label="自动朗读长度上限"
            description="超过该长度的文本不自动朗读（中文按字、其它语种按单词计数）；填 0 表示不限制。手动点喇叭始终朗读，不受此限制"
            value={speech?.auto_read_max_units ?? 0}
            max={100000}
            unit="字 / 词"
            onChange={(v) => onChange("auto_read_max_units", v)}
          />
          <ToggleRow
            label="流式边收边播"
            description="音频分块到达即开始播放，显著降低长文本的等待时间；仅对小米 MiMo（chat+audio）流式协议生效"
            checked={flag("stream_playback")}
            onChange={(v) => onChange("stream_playback", v)}
          />
        </RowList>
      </Group>
      <div style={{ marginTop: "12px" }}>
        <InfoNote>
          朗读使用「服务 → TTS」里配置的模型；手动朗读可点击翻译面板中原文 / 译文卡片下方的喇叭按钮。
        </InfoNote>
      </div>
    </div>
  );
}

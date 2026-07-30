import { Group, InfoNote, RowList, SectionHeader, ToggleRow } from "./controls";
import type { Settings } from "../../types";

type SpeechKey = keyof Settings["speech"];

const toggles: { key: SpeechKey; label: string; description: string }[] = [
  {
    key: "auto_read_source",
    label: "翻译后自动朗读原文",
    description: "翻译完成后先朗读原文",
  },
  {
    key: "auto_read_target",
    label: "翻译后自动朗读译文",
    description: "与「朗读原文」可同时开启，此时先读原文再读译文",
  },
  {
    key: "stream_playback",
    label: "流式边收边播",
    description: "音频分块到达即开始播放，显著降低长文本的等待时间；仅对小米 MiMo（chat+audio）流式协议生效",
  },
];

/** 「朗读」分区：自动朗读与流式播放开关。 */
export function SpeechSettings({
  speech,
  onChange,
}: {
  speech: Settings["speech"];
  onChange: (key: SpeechKey, value: boolean) => void;
}) {
  return (
    <div>
      <SectionHeader title="朗读" description="控制翻译完成后的自动朗读行为与音频播放方式。" />
      <Group>
        <RowList>
          {toggles.map(({ key, label, description }) => (
            <ToggleRow
              key={key}
              label={label}
              description={description}
              checked={speech?.[key] ?? key === "stream_playback"}
              onChange={(v) => onChange(key, v)}
            />
          ))}
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

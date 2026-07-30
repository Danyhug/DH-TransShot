import { HotkeyInput } from "./HotkeyInput";
import { Group, InfoNote, RowList, SectionHeader } from "./controls";
import type { HotkeyConfig } from "../../types";

const hotkeyRows: { key: keyof HotkeyConfig; label: string; description: string }[] = [
  { key: "screenshot", label: "区域截图", description: "框选后裁切并复制到剪贴板" },
  { key: "ocr_translate", label: "区域翻译", description: "框选 → OCR 识别 → 翻译并显示" },
  {
    key: "clipboard_translate",
    label: "翻译选中文本",
    description: "读取当前选中的文字并翻译；失败时回退到剪贴板",
  },
];

/** 「快捷键」分区：三个全局动作的组合键录入。 */
export function HotkeySettings({
  hotkeys,
  invalid,
  onChange,
}: {
  hotkeys: HotkeyConfig;
  /** 保存校验未通过时，把没填的项就地标红 */
  invalid: boolean;
  onChange: (key: keyof HotkeyConfig, value: string) => void;
}) {
  return (
    <div>
      <SectionHeader
        title="快捷键"
        description="点击右侧按钮后直接按下组合键录入，Esc 取消。设置窗口打开期间全局快捷键会暂时挂起。"
      />
      <Group>
        <RowList>
          {hotkeyRows.map(({ key, label, description }) => {
            const value = hotkeys?.[key] ?? "";
            const isEmpty = invalid && !value.trim();
            return (
              <div key={key} className="flex items-center justify-between gap-4">
                <span className="min-w-0">
                  <span className="block text-xs font-medium" style={{ color: "var(--color-text)" }}>
                    {label}
                  </span>
                  <span
                    className="block text-xs"
                    style={{
                      color: isEmpty ? "#ef4444" : "var(--color-text-secondary)",
                      marginTop: "2px",
                      lineHeight: 1.5,
                    }}
                  >
                    {isEmpty ? "未设置，保存前必须填写" : description}
                  </span>
                </span>
                <HotkeyInput value={value} onChange={(v) => onChange(key, v)} />
              </div>
            );
          })}
        </RowList>
      </Group>
      <div style={{ marginTop: "12px" }}>
        <InfoNote>
          支持 <code>Alt</code> / <code>Option</code> / <code>Ctrl</code> / <code>Shift</code> /{" "}
          <code>Cmd</code> / <code>Super</code> / <code>CmdOrCtrl</code> 作为修饰键，至少需要一个；
          主键支持字母、数字、F1~F24、方向键与常用符号。
        </InfoNote>
      </div>
    </div>
  );
}

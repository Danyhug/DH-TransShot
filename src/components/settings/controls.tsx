import { Children, type InputHTMLAttributes, type ReactNode, type TextareaHTMLAttributes } from "react";

/**
 * 设置窗口的共享基础控件。
 *
 * 设置页的各个分区（服务 / 快捷键 / 朗读）用的是同一套标签、输入框、胶囊按钮和开关，
 * 集中在这里定义，避免每个分区各写一份内联样式。视觉规则见 docs/theme.md。
 *
 * 注意：本文件（及各分区）一律用 flex + gap 做间距，不要用 Tailwind 的 `space-y-*`。
 * globals.css 里的 `* { margin: 0 }` 是无层级（unlayered）规则，会盖过 Tailwind
 * `@layer utilities` 里的 margin-top，`space-y-*` 在本项目中不生效。
 */

/** 分区大标题 + 可选说明文字。 */
export function SectionHeader({ title, description }: { title: string; description?: string }) {
  return (
    <div style={{ marginBottom: "12px" }}>
      <h2 className="text-sm font-semibold" style={{ color: "var(--color-text)" }}>
        {title}
      </h2>
      {description && (
        <p
          className="text-xs"
          style={{ color: "var(--color-text-secondary)", marginTop: "3px", lineHeight: 1.5 }}
        >
          {description}
        </p>
      )}
    </div>
  );
}

/** 表单字段：标签 + 控件 + 可选补充说明。 */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="text-xs font-medium" style={{ color: "var(--color-text-secondary)" }}>
        {label}
      </span>
      <div style={{ marginTop: "5px" }}>{children}</div>
      {hint && (
        <div
          className="text-xs"
          style={{
            color: "var(--color-text-secondary)",
            marginTop: "5px",
            lineHeight: 1.5,
            opacity: 0.8,
          }}
        >
          {hint}
        </div>
      )}
    </label>
  );
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  const { className = "", ...rest } = props;
  return <input {...rest} className={`settings-input ${className}`.trim()} />;
}

export function CodeArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const { className = "", ...rest } = props;
  return (
    <textarea {...rest} className={`settings-input settings-input--code ${className}`.trim()} />
  );
}

/** 透明底 + 细边框的分组容器（输入框本身是 surface 色，同色嵌套会糊成一片）。 */
export function Group({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`settings-group ${className}`.trim()}>{children}</div>;
}

/** 说明性提示块，用于放规则说明这类不需要操作的信息。 */
export function InfoNote({ children }: { children: ReactNode }) {
  return (
    <div
      className="text-xs"
      style={{
        color: "var(--color-text-secondary)",
        backgroundColor: "var(--color-surface)",
        borderRadius: "8px",
        padding: "8px 10px",
        lineHeight: 1.7,
      }}
    >
      {children}
    </div>
  );
}

/** 胶囊按钮：提供商切换、参数预设都用它。 */
export function Chip({
  children,
  active = false,
  dashed = false,
  disabled = false,
  title,
  onClick,
}: {
  children: ReactNode;
  active?: boolean;
  dashed?: boolean;
  disabled?: boolean;
  title?: string;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="text-xs transition-colors"
      style={{
        padding: "4px 11px",
        borderRadius: "9999px",
        border: dashed ? "1px dashed var(--color-border)" : "1px solid transparent",
        cursor: disabled ? "not-allowed" : "pointer",
        backgroundColor: active
          ? "var(--color-primary)"
          : dashed
            ? "transparent"
            : "var(--color-surface)",
        color: active ? "#fff" : "var(--color-text-secondary)",
        opacity: disabled ? 0.4 : 1,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </button>
  );
}

/** 分段控件（翻译 / OCR / TTS 这类互斥切换）。 */
export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { key: T; label: string }[];
  onChange: (key: T) => void;
}) {
  return (
    <div
      className="inline-flex"
      style={{
        backgroundColor: "var(--color-surface)",
        borderRadius: "9px",
        padding: "2px",
        gap: "2px",
      }}
    >
      {options.map((option) => {
        const active = option.key === value;
        return (
          <button
            key={option.key}
            type="button"
            onClick={() => onChange(option.key)}
            className="text-xs font-medium transition-colors"
            style={{
              padding: "5px 16px",
              borderRadius: "7px",
              border: "none",
              cursor: "pointer",
              backgroundColor: active ? "var(--color-primary)" : "transparent",
              color: active ? "#fff" : "var(--color-text-secondary)",
            }}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

/** 开关行：左侧标题 + 可选说明，右侧开关。 */
export function ToggleRow({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-start justify-between gap-4 cursor-pointer">
      <span className="min-w-0">
        <span className="block text-xs font-medium" style={{ color: "var(--color-text)" }}>
          {label}
        </span>
        {description && (
          <span
            className="block text-xs"
            style={{ color: "var(--color-text-secondary)", marginTop: "2px", lineHeight: 1.5 }}
          >
            {description}
          </span>
        )}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className="relative transition-colors shrink-0"
        style={{
          width: "34px",
          height: "18px",
          marginTop: "1px",
          borderRadius: "9999px",
          border: "none",
          cursor: "pointer",
          backgroundColor: checked ? "var(--color-primary)" : "var(--color-surface)",
        }}
      >
        <span
          className="absolute transition-all"
          style={{
            top: "2px",
            left: checked ? "18px" : "2px",
            width: "14px",
            height: "14px",
            borderRadius: "9999px",
            backgroundColor: "#fff",
          }}
        />
      </button>
    </label>
  );
}

/** 分区之间的水平分隔线。 */
export function Divider() {
  return (
    <div
      style={{
        height: "1px",
        backgroundColor: "var(--color-border)",
        margin: "20px 0",
      }}
    />
  );
}

/**
 * 带分隔线的设置行列表：快捷键行与朗读开关共用同一种行距和分隔线，
 * 保证两个分区看起来是同一套东西。
 */
export function RowList({ children }: { children: ReactNode }) {
  const items = Children.toArray(children);
  return (
    <div>
      {items.map((child, i) => (
        <div
          key={i}
          style={{
            paddingTop: i === 0 ? 0 : "11px",
            paddingBottom: i === items.length - 1 ? 0 : "11px",
            borderTop: i === 0 ? undefined : "1px solid var(--color-border)",
          }}
        >
          {child}
        </div>
      ))}
    </div>
  );
}

interface Props {
  value: string;
  onChange?: (value: string) => void;
  placeholder?: string;
  readOnly?: boolean;
}

export function TextArea({ value, onChange, placeholder, readOnly = false }: Props) {
  return (
    <textarea
      value={value}
      onChange={onChange ? (e) => onChange(e.target.value) : undefined}
      placeholder={placeholder}
      readOnly={readOnly}
      className="w-full flex-1 resize-none text-sm leading-relaxed outline-none"
      style={{
        backgroundColor: "transparent",
        color: "var(--color-text)",
        padding: "8px 12px",
      }}
    />
  );
}

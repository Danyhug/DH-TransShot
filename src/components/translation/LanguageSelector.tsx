import { languages, targetLanguages } from "../../lib/languages";

interface Props {
  value: string;
  onChange: (value: string) => void;
  includeAuto?: boolean;
}

export function LanguageSelector({ value, onChange, includeAuto = false }: Props) {
  const list = includeAuto ? languages : targetLanguages;

  return (
    <div className="relative inline-flex items-center">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="pl-2 pr-5 py-1 rounded-lg text-sm outline-none cursor-pointer appearance-none"
        style={{
          backgroundColor: "transparent",
          color: "var(--color-text)",
        }}
      >
        {list.map((lang) => (
          <option key={lang.code} value={lang.code}>
            {lang.name}
          </option>
        ))}
      </select>
      {/* Down arrow */}
      <svg
        className="absolute right-0.5 pointer-events-none"
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ color: "var(--color-text-secondary)" }}
      >
        <path d="m6 9 6 6 6-6" />
      </svg>
    </div>
  );
}

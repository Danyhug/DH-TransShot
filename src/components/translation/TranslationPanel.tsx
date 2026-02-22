import { LanguageSelector } from "./LanguageSelector";
import { SwapButton } from "./SwapButton";
import { TextArea } from "./TextArea";
import { ActionButtons } from "./ActionButtons";
import { useTranslation } from "../../hooks/useTranslation";
import { useTranslationStore } from "../../stores/translationStore";

export function TranslationPanel() {
  const {
    sourceText,
    translatedText,
    isTranslating,
    error,
    setSourceText,
    translate,
  } = useTranslation();

  const { sourceLang, targetLang, setSourceLang, setTargetLang, swapLanguages } =
    useTranslationStore();

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      translate();
    }
  };

  return (
    <div className="flex flex-col flex-1 px-4 pb-4 pt-1 gap-3 overflow-hidden" onKeyDown={handleKeyDown}>
      {/* Source text card */}
      <div
        className="flex-1 flex flex-col min-h-0 rounded-xl overflow-hidden"
        style={{ backgroundColor: "var(--color-surface)" }}
      >
        <TextArea
          value={sourceText}
          onChange={setSourceText}
          placeholder="输入要翻译的文本... (Ctrl+Enter)"
        />
        <ActionButtons text={sourceText} />
      </div>

      {/* Language selection bar */}
      <div className="flex items-center justify-center px-1 py-0.5">
        <div className="flex items-center gap-3">
          <LanguageSelector value={sourceLang} onChange={setSourceLang} includeAuto />
          <SwapButton onClick={swapLanguages} disabled={sourceLang === "auto"} />
          <LanguageSelector value={targetLang} onChange={setTargetLang} />
        </div>
      </div>

      {/* Translation result card */}
      <div
        className="flex-1 flex flex-col min-h-0 rounded-xl overflow-hidden"
        style={{ backgroundColor: "var(--color-surface)" }}
      >
        <TextArea
          value={isTranslating ? "翻译中..." : translatedText}
          readOnly
          placeholder="翻译结果将显示在这里..."
        />
        <ActionButtons text={translatedText} />
      </div>

      {/* Error */}
      {error && (
        <div className="text-xs text-red-500 px-1">{error}</div>
      )}
    </div>
  );
}

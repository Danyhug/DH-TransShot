import { useCallback } from "react";
import { useTranslationStore } from "../stores/translationStore";
import { useSettingsStore } from "../stores/settingsStore";
import { translateText } from "../lib/invoke";
import { appLog } from "../stores/logStore";

// Generation counter: incremented on each translate call or explicit cancel.
// Stale calls (whose captured generation no longer matches) silently discard results.
let translateGeneration = 0;

/** Invalidate any in-flight translate call so its result will be discarded. */
export function cancelPendingTranslation() {
  translateGeneration++;
}

export function useTranslation() {
  const {
    sourceText,
    translatedText,
    sourceLang,
    targetLang,
    isTranslating,
    isOcrProcessing,
    error,
    setSourceText,
    setTranslatedText,
    setIsTranslating,
    setError,
  } = useTranslationStore();

  const translate = useCallback(
    async (text?: string) => {
      const input = text ?? sourceText;
      if (!input.trim()) {
        appLog.warn("[Translate] 输入文本为空，跳过翻译");
        return;
      }

      // Read latest settings from store to avoid stale closure
      const { settings } = useSettingsStore.getState();

      if (!settings.api_key && !settings.base_url.includes("localhost")) {
        appLog.warn("[Translate] 未配置 API Key，且非本地服务");
        setError("Please configure your API key in settings");
        return;
      }

      appLog.info("[Translate] 手动翻译: " + sourceLang + " → " + targetLang + ", 文本长度=" + input.length);
      const generation = ++translateGeneration;
      setIsTranslating(true);
      setError(null);

      try {
        const result = await translateText(input, sourceLang, targetLang);
        if (generation !== translateGeneration) {
          appLog.info("[Translate] 翻译结果已过期, 丢弃");
          return;
        }
        appLog.info("[Translate] 翻译完成, 结果长度=" + result.length);
        setTranslatedText(result);
      } catch (e) {
        if (generation !== translateGeneration) {
          appLog.info("[Translate] 错误已过期, 忽略");
          return;
        }
        appLog.error("[Translate] 翻译失败: " + String(e));
        setError(String(e));
      } finally {
        if (generation === translateGeneration) {
          setIsTranslating(false);
        }
      }
    },
    [sourceText, sourceLang, targetLang, setTranslatedText, setIsTranslating, setError]
  );

  return {
    sourceText,
    translatedText,
    sourceLang,
    targetLang,
    isTranslating,
    isOcrProcessing,
    error,
    setSourceText,
    translate,
  };
}

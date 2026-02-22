import { useCallback } from "react";
import { useTranslationStore } from "../stores/translationStore";
import { useSettingsStore } from "../stores/settingsStore";
import { translateText } from "../lib/invoke";
import { appLog } from "../stores/logStore";

export function useTranslation() {
  const {
    sourceText,
    translatedText,
    sourceLang,
    targetLang,
    isTranslating,
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
      setIsTranslating(true);
      setError(null);

      try {
        const result = await translateText(input, sourceLang, targetLang);
        appLog.info("[Translate] 翻译完成, 结果长度=" + result.length);
        setTranslatedText(result);
      } catch (e) {
        appLog.error("[Translate] 翻译失败: " + String(e));
        setError(String(e));
      } finally {
        setIsTranslating(false);
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
    error,
    setSourceText,
    translate,
  };
}

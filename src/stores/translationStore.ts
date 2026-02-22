import { create } from "zustand";

interface TranslationState {
  sourceText: string;
  translatedText: string;
  sourceLang: string;
  targetLang: string;
  isTranslating: boolean;
  error: string | null;
  setSourceText: (text: string) => void;
  setTranslatedText: (text: string) => void;
  setSourceLang: (lang: string) => void;
  setTargetLang: (lang: string) => void;
  setIsTranslating: (v: boolean) => void;
  setError: (error: string | null) => void;
  swapLanguages: () => void;
}

export const useTranslationStore = create<TranslationState>((set, get) => ({
  sourceText: "",
  translatedText: "",
  sourceLang: "auto",
  targetLang: "zh-CN",
  isTranslating: false,
  error: null,
  setSourceText: (text) => set({ sourceText: text }),
  setTranslatedText: (text) => set({ translatedText: text }),
  setSourceLang: (lang) => set({ sourceLang: lang }),
  setTargetLang: (lang) => set({ targetLang: lang }),
  setIsTranslating: (v) => set({ isTranslating: v }),
  setError: (error) => set({ error }),
  swapLanguages: () => {
    const { sourceLang, targetLang, sourceText, translatedText } = get();
    if (sourceLang === "auto") return;
    set({
      sourceLang: targetLang,
      targetLang: sourceLang,
      sourceText: translatedText,
      translatedText: sourceText,
    });
  },
}));

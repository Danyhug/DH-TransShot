export interface Language {
  code: string;
  name: string;
}

export const languages: Language[] = [
  { code: "auto", name: "自动检测" },
  { code: "zh-CN", name: "中文简体" },
  { code: "zh-TW", name: "中文繁體" },
  { code: "en", name: "英语" },
  { code: "ja", name: "日语" },
  { code: "ko", name: "韩语" },
  { code: "fr", name: "法语" },
  { code: "de", name: "德语" },
  { code: "es", name: "西班牙语" },
  { code: "pt", name: "葡萄牙语" },
  { code: "ru", name: "俄语" },
  { code: "ar", name: "阿拉伯语" },
  { code: "it", name: "意大利语" },
  { code: "th", name: "泰语" },
  { code: "vi", name: "越南语" },
];

export const targetLanguages = languages.filter((l) => l.code !== "auto");

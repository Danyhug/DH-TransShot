import { useState, useRef } from "react";
import { synthesizeSpeech } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";

interface Props {
  text: string;
}

export function ActionButtons({ text }: Props) {
  const [isSpeaking, setIsSpeaking] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const copyToClipboard = () => {
    navigator.clipboard.writeText(text).catch(console.error);
  };

  const speak = async () => {
    if (!text || isSpeaking) return;

    // Stop any currently playing audio
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
    }

    setIsSpeaking(true);
    appLog.info("[TTS] 开始语音合成, 文本长度=" + text.length);

    try {
      const base64Audio = await synthesizeSpeech(text);
      const audio = new Audio(`data:audio/mp3;base64,${base64Audio}`);
      audioRef.current = audio;

      audio.onended = () => {
        setIsSpeaking(false);
        audioRef.current = null;
      };
      audio.onerror = () => {
        appLog.error("[TTS] 音频播放失败");
        setIsSpeaking(false);
        audioRef.current = null;
      };

      await audio.play();
      appLog.info("[TTS] 音频播放开始");
    } catch (e) {
      appLog.error("[TTS] 语音合成失败: " + String(e));
      setIsSpeaking(false);
    }
  };

  return (
    <div className="flex items-center gap-1.5" style={{ padding: "0 12px 8px" }}>
      <button
        onClick={speak}
        disabled={!text || isSpeaking}
        className="p-1.5 rounded-md transition-colors hover:bg-black/5 disabled:opacity-25"
        style={{ color: "var(--color-text-secondary)" }}
        title={isSpeaking ? "朗读中..." : "朗读"}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
          <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
          <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
        </svg>
      </button>
      <button
        onClick={copyToClipboard}
        disabled={!text}
        className="p-1.5 rounded-md transition-colors hover:bg-black/5 disabled:opacity-25"
        style={{ color: "var(--color-text-secondary)" }}
        title="复制"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      </button>
    </div>
  );
}

import { speak, stopSpeaking } from "../../lib/tts";
import { useTtsStore } from "../../stores/ttsStore";
import { appLog } from "../../stores/logStore";

interface Props {
  text: string;
  /** 朗读会话标识，用于区分原文/译文按钮的「朗读中」高亮 */
  speakId: string;
}

export function ActionButtons({ text, speakId }: Props) {
  const speakingId = useTtsStore((s) => s.speakingId);
  const isSpeaking = speakingId === speakId;

  const copyToClipboard = () => {
    navigator.clipboard.writeText(text).catch(console.error);
  };

  const onSpeakClick = () => {
    if (!text) return;
    if (isSpeaking) {
      appLog.info("[TTS] 手动停止朗读 (" + speakId + ")");
      stopSpeaking();
      return;
    }
    appLog.info("[TTS] 准备朗读, 文本长度=" + text.length + " (" + speakId + ")");
    speak(text, speakId);
  };

  return (
    <div className="flex items-center gap-1.5" style={{ padding: "0 12px 8px" }}>
      <button
        onClick={onSpeakClick}
        disabled={!text}
        className="p-1.5 rounded-md transition-colors hover:bg-black/5 disabled:opacity-25"
        style={{ color: isSpeaking ? "var(--color-primary)" : "var(--color-text-secondary)" }}
        title={isSpeaking ? "停止朗读" : "朗读"}
      >
        {isSpeaking ? (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="6" y="5" width="4" height="14" rx="1" />
            <rect x="14" y="5" width="4" height="14" rx="1" />
          </svg>
        ) : (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
            <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
            <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
          </svg>
        )}
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

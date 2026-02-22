interface Props {
  text: string;
}

export function ActionButtons({ text }: Props) {
  const copyToClipboard = () => {
    navigator.clipboard.writeText(text).catch(console.error);
  };

  const speak = () => {
    if (!text) return;
    const utterance = new SpeechSynthesisUtterance(text);
    window.speechSynthesis.speak(utterance);
  };

  return (
    <div className="flex items-center gap-1.5" style={{ padding: "0 12px 8px" }}>
      <button
        onClick={speak}
        disabled={!text}
        className="p-1.5 rounded-md transition-colors hover:bg-black/5 disabled:opacity-25"
        style={{ color: "var(--color-text-secondary)" }}
        title="朗读"
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

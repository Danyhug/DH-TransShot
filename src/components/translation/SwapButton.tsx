interface Props {
  onClick: () => void;
  disabled?: boolean;
}

export function SwapButton({ onClick, disabled }: Props) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="p-1.5 rounded-md transition-colors hover:bg-black/5 active:bg-black/10 disabled:opacity-30"
      style={{
        color: "var(--color-text-secondary)",
      }}
      title="交换语言"
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M7 16l-4-4 4-4" />
        <path d="M3 12h18" />
        <path d="M17 8l4 4-4 4" />
      </svg>
    </button>
  );
}

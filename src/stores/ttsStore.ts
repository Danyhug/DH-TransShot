import { create } from "zustand";

interface TtsState {
  /** 当前正在朗读的会话标识（对应 ActionButtons 的 role，如 "source"/"target"）；null 表示未在朗读 */
  speakingId: string | null;
  setSpeakingId: (id: string | null) => void;
}

/**
 * 全局朗读状态。因为播放器是单例（同一时刻只播一段），用一个全局标识让对应按钮显示「朗读中」，
 * 切换朗读时自动熄灭上一个按钮。
 */
export const useTtsStore = create<TtsState>((set) => ({
  speakingId: null,
  setSpeakingId: (id) => set({ speakingId: id }),
}));

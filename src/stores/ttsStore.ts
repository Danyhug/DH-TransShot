import { create } from "zustand";

interface TtsState {
  /** 当前正在朗读的会话标识（对应 ActionButtons 的 role，如 "source"/"target"）；null 表示未在朗读 */
  speakingId: string | null;
  /** 已发起合成、但还没收到第一段音频数据的会话标识；null 表示没有等待中的请求 */
  loadingId: string | null;
  setSpeakingId: (id: string | null) => void;
  setLoadingId: (id: string | null) => void;
}

/**
 * 全局朗读状态。因为播放器是单例（同一时刻只播一段），用一个全局标识让对应按钮显示「朗读中」，
 * 切换朗读时自动熄灭上一个按钮。
 *
 * `loadingId` 与 `speakingId` 分开：点击后到第一段音频到达之间可能有数秒（合成 + 网络），
 * 此时按钮显示加载图标，收到数据后才切成「停止」。
 */
export const useTtsStore = create<TtsState>((set) => ({
  speakingId: null,
  loadingId: null,
  setSpeakingId: (id) => set({ speakingId: id }),
  setLoadingId: (id) => set({ loadingId: id }),
}));

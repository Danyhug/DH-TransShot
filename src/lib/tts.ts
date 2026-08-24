import { Channel } from "@tauri-apps/api/core";
import { speakText, stopSpeech } from "./invoke";
import type { TtsPlaybackMessage } from "./invoke";
import { appLog } from "../stores/logStore";
import { useTtsStore } from "../stores/ttsStore";

/**
 * 朗读的编排层：抢占、按钮状态、长度统计。
 *
 * **音频的合成、缓存与播放全在 Rust 侧**（`src-tauri/src/audio/`、`src-tauri/src/tts/`），
 * 这里只负责发起 / 停止和 UI 状态。
 *
 * 为什么不在前端播：主窗口失焦会自动隐藏，WKWebView 一被标记为遮挡，WebKit 就停掉
 * `AudioContext` 背后的音频单元，却仍用定时器时钟继续「空转渲染」——`state` 是 `running`、
 * `currentTime` 正常推进、`onended` 照常触发，样本却没送到输出设备，JS 侧查不出任何异常。
 * 而快捷键翻译的典型流程恰恰是「窗口弹出 → 焦点回到原 App → 窗口隐藏」，于是表现为
 * 「日志一切正常但一声不响」。预热输出设备、每次朗读重建 context、比对墙上时钟与音频时长
 * 做哑火兜底都试过，挡不住，最后把播放整体挪到了 Rust 侧。
 */

// 中日韩文字（CJK 扩展 A + 基本区 + 假名 + 谚文）——这些语言不按空格分词，逐字计数
const CJK_PATTERN = /[㐀-䶿一-鿿぀-ヿ가-힯]/g;
// 其余语种按「字母/数字串」计一个单词
const WORD_PATTERN = /[\p{L}\p{N}]+/gu;

/**
 * 统计文本长度：中文/日文/韩文按字计，其余语种按单词计，两者相加。
 * 用于「自动朗读长度上限」判断（`settings.speech.auto_read_max_units`）。
 */
export function countSpeechUnits(text: string): number {
  const cjk = text.match(CJK_PATTERN)?.length ?? 0;
  const words = text.replace(CJK_PATTERN, " ").match(WORD_PATTERN)?.length ?? 0;
  return cjk + words;
}

// ── 单例播放控制 ─────────────────────────────────────────────────────────
// 同一时刻只播一段。preempt() 递增 playGen；每个 speak / speakSequence 抢占后拿到自己的
// gen，全程用 `gen === playGen` 判断是否被后来的朗读打断。
//
// 注意 preempt() **不调后端的 stop**：`speak_text` 自己就会抢占上一段，而两个 invoke
// 谁先到达没有保证——先发 stop 再发 speak，stop 反而可能后到、把新的这段停掉。
// 只有明确的「停止朗读」（stopSpeaking）才调 stop_speech。
let playGen = 0;

function preempt(): number {
  playGen++;
  return playGen;
}

/** 停止当前朗读（若有），并熄灭「朗读中」/「加载中」标识。 */
export function stopSpeaking() {
  preempt();
  const store = useTtsStore.getState();
  store.setSpeakingId(null);
  store.setLoadingId(null);
  stopSpeech().catch((e) => appLog.error("[TTS] 停止朗读失败: " + String(e)));
}

/**
 * 在指定 generation 下朗读一段文本（内部使用）。调用方负责抢占（拿到 gen）和最终熄灯。
 *
 * `speakText` 在**播完之后**才 resolve；被后来的朗读抢占时后端会提前收场并正常返回，
 * 所以这里不需要额外的超时或看门狗。
 */
async function playOne(text: string, id: string, gen: number): Promise<void> {
  const normalized = text.trim();
  if (!normalized || gen !== playGen) return;

  // 音频还没到手（合成 + 网络往返可能好几秒）→ 按钮显示加载态，真正出声时熄灭
  useTtsStore.getState().setLoadingId(id);
  const clearLoading = () => {
    if (gen === playGen) useTtsStore.getState().setLoadingId(null);
  };

  const channel = new Channel<TtsPlaybackMessage>();
  channel.onmessage = (message) => {
    if (message?.event === "start") {
      appLog.info("[TTS] 音频已开始输出 (" + id + ")");
      clearLoading();
    } else {
      // 不认识就丢会变成「完全没声音且毫无线索」，至少留条日志
      appLog.warn("[TTS] 收到无法识别的播放状态消息，已忽略: " + JSON.stringify(message));
    }
  };

  try {
    appLog.info("[TTS] 发起朗读, 文本长度=" + normalized.length + " (" + id + ")");
    await speakText(normalized, channel);
    appLog.info("[TTS] 朗读结束 (" + id + ")");
  } finally {
    clearLoading();
  }
}

/**
 * 朗读一段文本。`id` 标识调用方（如 "source"/"target"），用于「朗读中」高亮。
 * 会抢占正在进行的朗读；返回的 promise 在播完 / 出错 / 被打断时兑现。
 */
export async function speak(text: string, id: string): Promise<void> {
  if (!text.trim()) return;
  const gen = preempt();
  useTtsStore.getState().setSpeakingId(id);
  try {
    await playOne(text, id, gen);
  } finally {
    if (gen === playGen) {
      const store = useTtsStore.getState();
      store.setSpeakingId(null);
      store.setLoadingId(null);
    }
  }
}

/**
 * 依次朗读多段文本（前一段播完再播下一段）；被外部朗读/停止打断则整体中止。
 * 单段失败不牵连后续段：原文合成挂了，译文该读还是要读。
 */
export async function speakSequence(items: { text: string; id: string }[]) {
  const filtered = items.filter((it) => it.text.trim());
  if (filtered.length === 0) return;
  const gen = preempt();
  const store = useTtsStore.getState();
  try {
    for (const item of filtered) {
      if (gen !== playGen) break;
      store.setSpeakingId(item.id);
      try {
        await playOne(item.text, item.id, gen);
      } catch (e) {
        appLog.error("[TTS] 朗读失败 (" + item.id + "): " + String(e));
      }
    }
  } finally {
    if (gen === playGen) {
      store.setSpeakingId(null);
      store.setLoadingId(null);
    }
  }
}

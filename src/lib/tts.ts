import { Channel } from "@tauri-apps/api/core";
import { synthesizeSpeech, synthesizeSpeechStream } from "./invoke";
import type { TtsStreamPayload } from "./invoke";
import { appLog } from "../stores/logStore";
import { useTtsStore } from "../stores/ttsStore";
import { useSettingsStore, resolveActiveProvider } from "../stores/settingsStore";

// ── 完整音频前端缓存（LRU，与后端进程内缓存互补，减少重复请求）──────────────
const TTS_CACHE_MAX_ENTRIES = 32;
const ttsAudioCache = new Map<string, string>();

function normalizeTtsText(text: string) {
  return text.trim().replace(/\r\n/g, "\n");
}

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

function getTtsCacheKey(baseUrl: string, model: string, extra: string, text: string) {
  return `${baseUrl}\n${model}\n${extra}\n${text}`;
}

function getCachedAudio(key: string) {
  const cached = ttsAudioCache.get(key);
  if (!cached) return null;
  ttsAudioCache.delete(key);
  ttsAudioCache.set(key, cached);
  return cached;
}

function setCachedAudio(key: string, value: string) {
  if (!value) return;
  if (ttsAudioCache.has(key)) ttsAudioCache.delete(key);
  ttsAudioCache.set(key, value);
  while (ttsAudioCache.size > TTS_CACHE_MAX_ENTRIES) {
    const oldestKey = ttsAudioCache.keys().next().value;
    if (!oldestKey) break;
    ttsAudioCache.delete(oldestKey);
  }
}

// ── 音频 MIME 嗅探（后端 base64 格式不定：mp3 / wav / ogg / flac）───────────
function detectAudioMime(base64Audio: string): string {
  try {
    const header = atob(base64Audio.slice(0, 8)); // 前 6 字节足够识别魔数
    if (header.startsWith("RIFF")) return "audio/wav";
    if (header.startsWith("ID3")) return "audio/mpeg";
    if (header.startsWith("OggS")) return "audio/ogg";
    if (header.startsWith("fLaC")) return "audio/flac";
    // MP3 帧同步：0xFF 0xEx/0xFx（无 ID3 头的 mp3）
    if (header.charCodeAt(0) === 0xff && (header.charCodeAt(1) & 0xe0) === 0xe0) {
      return "audio/mpeg";
    }
    return "audio/mpeg";
  } catch {
    return "audio/mpeg";
  }
}

// ── AudioContext 单例 ────────────────────────────────────────────────────
// 复用同一个 context：WebKit 对同时存在的 AudioContext 数量有硬上限，每次朗读都
// new + close 在连续朗读时容易踩到；而且复用后只需在首次用户手势时 resume 一次。
let sharedCtx: AudioContext | null = null;

function getAudioContext(): AudioContext | null {
  if (sharedCtx && sharedCtx.state !== "closed") return sharedCtx;
  const Ctor =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  sharedCtx = new Ctor();
  return sharedCtx;
}

/** 浏览器是否支持 Web Audio（边收边播依赖它，否则回退整段播放）。 */
export function isStreamPlaybackSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    !!(window.AudioContext || (window as unknown as { webkitAudioContext?: unknown }).webkitAudioContext)
  );
}

// ── 播放器单例控制 ───────────────────────────────────────────────────────
// 同一时刻只播一段。preempt() 递增 playGen 并停掉当前播放；每个 speak / speakSequence
// 抢占后拿到自己的 gen，全程用 `gen === playGen` 判断是否被后来的朗读打断。
let playGen = 0;
let stopCurrent: (() => void) | null = null;

function preempt(): number {
  playGen++;
  if (stopCurrent) {
    const fn = stopCurrent;
    stopCurrent = null;
    fn();
  }
  return playGen;
}

/** 停止当前朗读（若有），并熄灭「朗读中」/「加载中」标识。 */
export function stopSpeaking() {
  preempt();
  const store = useTtsStore.getState();
  store.setSpeakingId(null);
  store.setLoadingId(null);
}

/**
 * 首块播放前预留的缓冲时长（秒）。分块到达有网络抖动，若第一块紧贴 `currentTime` 起播，
 * 后续块稍慢一点就会在扬声器上听到断断续续的空隙。
 */
const STREAM_PREROLL_SECONDS = 0.2;

/**
 * 边收边播的 PCM16LE 播放器。
 * 逐块把二进制 PCM 调度进 AudioContext，按到达顺序无缝排布；上游流结束
 * （`markInputComplete`）且所有已排块播完时，`done` promise 兑现。
 */
class StreamingPcmPlayer {
  private ctx: AudioContext | null = null;
  private sampleRate = 24000;
  private nextTime = 0;
  private pending = new Set<AudioBufferSourceNode>();
  private scheduled = 0;
  private leftover: Uint8Array | null = null; // 跨块残留的奇数尾字节
  private inputDone = false;
  private stopped = false;
  private settled = false;
  private drainTimer: ReturnType<typeof setTimeout> | null = null;
  private resolveDone!: () => void;
  readonly done: Promise<void>;

  /** @param onFirstAudio 第一块 PCM 排入播放时回调一次（用于熄灭按钮的加载态）。 */
  constructor(private onFirstAudio?: () => void) {
    this.done = new Promise((resolve) => {
      this.resolveDone = resolve;
    });
  }

  /** 已实际排入播放的分块数。 */
  get scheduledChunks() {
    return this.scheduled;
  }

  /** 上游声明的分块格式，必须在第一块之前设置。 */
  setFormat(sampleRate: number, channels: number) {
    if (sampleRate > 0) this.sampleRate = sampleRate;
    if (channels > 1) {
      appLog.warn("[TTS] 流式分块声道数=" + channels + "，当前按单声道处理");
    }
  }

  pushChunk(chunk: ArrayBuffer) {
    if (this.stopped) return;
    try {
      const ctx = this.ctx ?? getAudioContext();
      if (!ctx) return;
      this.ctx = ctx;
      // 无用户手势时 AudioContext 可能是 suspended，不 resume 会一声不响地什么都不播
      if (ctx.state === "suspended") ctx.resume().catch(() => {});

      // 拼接上一块残留的奇数字节，保证 Int16 对齐
      let bytes = new Uint8Array(chunk);
      if (this.leftover && this.leftover.length) {
        const merged = new Uint8Array(this.leftover.length + bytes.length);
        merged.set(this.leftover, 0);
        merged.set(bytes, this.leftover.length);
        bytes = merged;
        this.leftover = null;
      }
      if (bytes.length % 2 !== 0) {
        this.leftover = bytes.slice(bytes.length - 1);
        bytes = bytes.slice(0, bytes.length - 1);
      }
      if (bytes.length === 0) return;

      const int16 = new Int16Array(bytes.buffer, bytes.byteOffset, bytes.length / 2);
      const float = new Float32Array(int16.length);
      for (let i = 0; i < int16.length; i++) float[i] = int16[i] / 32768;

      // 按 PCM 声明的采样率建 buffer，交给 AudioContext 重采样到其输出率，避免变调
      const buffer = ctx.createBuffer(1, float.length, this.sampleRate);
      buffer.copyToChannel(float, 0);
      const src = ctx.createBufferSource();
      src.buffer = buffer;
      src.connect(ctx.destination);

      const startAt =
        this.scheduled === 0
          ? ctx.currentTime + STREAM_PREROLL_SECONDS
          : Math.max(this.nextTime, ctx.currentTime);
      src.start(startAt);
      this.nextTime = startAt + buffer.duration;
      this.scheduled++;
      if (this.scheduled === 1 && this.onFirstAudio) {
        const notify = this.onFirstAudio;
        this.onFirstAudio = undefined;
        notify();
      }
      this.pending.add(src);
      src.onended = () => {
        this.pending.delete(src);
        this.maybeFinish();
      };
    } catch (e) {
      appLog.error("[TTS] 流式分块播放失败: " + String(e));
    }
  }

  /** 上游流已结束，不会再有新分块。 */
  markInputComplete() {
    if (this.inputDone) return;
    this.inputDone = true;
    this.armDrainWatchdog();
    this.maybeFinish();
  }

  /**
   * 兜底定时器：正常情况下靠 `onended` 收尾，但若 AudioContext 被系统挂起、或某个
   * source 的 `onended` 没有触发，这里保证 `done` 不会永远挂着（表现为「朗读中」不熄）。
   */
  private armDrainWatchdog() {
    if (this.drainTimer || !this.ctx) return;
    const ctx = this.ctx;
    const delayMs = Math.max(0, (this.nextTime - ctx.currentTime) * 1000) + 1000;
    this.drainTimer = setTimeout(() => {
      this.drainTimer = null;
      if (this.settled) return;
      if (this.pending.size === 0) {
        this.settle();
        return;
      }
      // 时间线确实还没走完（期间又排入了新块）→ 继续等
      if (ctx.state === "running" && ctx.currentTime < this.nextTime) {
        this.armDrainWatchdog();
        return;
      }
      appLog.warn("[TTS] 流式播放收尾异常，强制结束 (剩余分块=" + this.pending.size + ")");
      this.stop();
    }, delayMs);
  }

  private maybeFinish() {
    if (this.inputDone && this.pending.size === 0) this.settle();
  }

  private settle() {
    if (this.settled) return;
    this.settled = true;
    if (this.drainTimer) {
      clearTimeout(this.drainTimer);
      this.drainTimer = null;
    }
    this.resolveDone();
  }

  stop() {
    if (this.stopped) return;
    this.stopped = true;
    this.pending.forEach((src) => {
      try {
        src.stop();
      } catch {
        /* already stopped */
      }
    });
    this.pending.clear();
    // 不 close 共享 ctx，只断开声音；close 后无法复用，且连续朗读会顶到数量上限
    this.settle();
  }
}

/**
 * 用 HTMLAudioElement 播放整段 base64 音频，播完 / 出错时兑现；期间登记 stopCurrent。
 * `onStart` 在浏览器真正开始播放时回调一次（用于熄灭按钮的加载态）。
 */
function playWholeAudio(base64Audio: string, onStart?: () => void): Promise<void> {
  return new Promise((resolve) => {
    const mime = detectAudioMime(base64Audio);
    const audio = new Audio(`data:${mime};base64,${base64Audio}`);
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      resolve();
    };
    stopCurrent = () => {
      try {
        audio.pause();
      } catch {
        /* noop */
      }
      finish();
    };
    audio.onended = finish;
    audio.onerror = () => {
      appLog.error("[TTS] 音频播放失败");
      finish();
    };
    audio.play().then(
      () => {
        appLog.info("[TTS] 音频播放开始, mime=" + mime);
        onStart?.();
      },
      (e) => {
        appLog.error("[TTS] 音频播放启动失败: " + String(e));
        finish();
      }
    );
  });
}

/**
 * 在指定 generation 下朗读一段文本（内部使用）。调用方负责抢占（拿到 gen）和最终熄灯。
 * 全程用 `gen === playGen` 判断是否被打断；一旦不等立即停止并返回。
 */
async function playOne(text: string, id: string, gen: number): Promise<void> {
  const normalized = normalizeTtsText(text);
  if (!normalized || gen !== playGen) return;

  const settings = useSettingsStore.getState().settings;
  const tts = resolveActiveProvider(settings, "tts");
  const key = getTtsCacheKey(tts.base_url, tts.model, settings.tts.extra, normalized);

  const cached = getCachedAudio(key);
  if (cached) {
    appLog.info("[TTS] 命中前端缓存 (" + id + ")");
    if (gen === playGen) await playWholeAudio(cached);
    return;
  }

  // 音频还没到手（合成 + 网络往返可能好几秒）→ 按钮显示加载态，收到第一段数据时熄灭
  useTtsStore.getState().setLoadingId(id);
  const clearLoading = () => {
    if (gen === playGen) useTtsStore.getState().setLoadingId(null);
  };

  try {
    const wantStream = settings.speech?.stream_playback !== false && isStreamPlaybackSupported();

    if (wantStream) {
      appLog.info("[TTS] 前端缓存未命中，发起流式语音请求 (" + id + ")");
      const player = new StreamingPcmPlayer(clearLoading);
      stopCurrent = () => player.stop();

      // 通道按发送顺序投递，因此 end 一定排在所有分块之后；不依赖它与命令返回值的先后
      let endReceived = false;
      const channel = new Channel<TtsStreamPayload>();
      channel.onmessage = (message) => {
        if (gen !== playGen) return;
        if (message instanceof ArrayBuffer) {
          player.pushChunk(message);
          return;
        }
        if (message.event === "start") {
          player.setFormat(message.sampleRate, message.channels);
        } else if (message.event === "end") {
          endReceived = true;
          player.markInputComplete();
        }
      };

      const resp = await synthesizeSpeechStream(normalized, channel);
      if (gen !== playGen) {
        player.stop();
        return;
      }
      if (resp.chunkCount > 0) {
        appLog.info(
          "[TTS] 流式播放中, 分块数=" + resp.chunkCount + ", 已排入播放=" + player.scheduledChunks
        );
        // 兜底：万一 end 控制消息丢失（通道某条消息投递失败会卡住后续消息），
        // 也别让 done 永远挂着；给分块留足到达时间后再收口
        const guard = setTimeout(() => {
          if (!endReceived) {
            appLog.warn("[TTS] 未收到流结束消息，按已收到的分块收尾");
            player.markInputComplete();
          }
        }, 5000);
        try {
          await player.done;
        } finally {
          clearTimeout(guard);
        }
      } else {
        // 没有流式分块（后端缓存命中 / audio/speech 协议 / 服务端未流式）→ 整段播放
        player.stop();
        if (resp.audio) {
          setCachedAudio(key, resp.audio);
          if (gen === playGen) await playWholeAudio(resp.audio, clearLoading);
        }
      }
      return;
    }

    // 不走流式：普通请求 + 整段播放
    appLog.info("[TTS] 前端缓存未命中，发起整段语音请求 (" + id + ")");
    const audio = await synthesizeSpeech(normalized);
    setCachedAudio(key, audio);
    if (gen !== playGen) return;
    if (audio) await playWholeAudio(audio, clearLoading);
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
      stopCurrent = null;
    }
  }
}

/** 依次朗读多段文本（前一段播完再播下一段）；被外部朗读/停止打断则整体中止。 */
export async function speakSequence(items: { text: string; id: string }[]) {
  const filtered = items.filter((it) => it.text.trim());
  if (filtered.length === 0) return;
  const gen = preempt();
  const store = useTtsStore.getState();
  try {
    for (const item of filtered) {
      if (gen !== playGen) break;
      store.setSpeakingId(item.id);
      await playOne(item.text, item.id, gen);
    }
  } finally {
    if (gen === playGen) {
      store.setSpeakingId(null);
      store.setLoadingId(null);
      stopCurrent = null;
    }
  }
}

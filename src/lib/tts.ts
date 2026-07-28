import { listen } from "@tauri-apps/api/event";
import { synthesizeSpeech, synthesizeSpeechStream } from "./invoke";
import { appLog } from "../stores/logStore";
import { useTtsStore } from "../stores/ttsStore";
import { useSettingsStore, resolveActiveProvider } from "../stores/settingsStore";

// ── 完整音频前端缓存（LRU，与后端进程内缓存互补，减少重复请求）──────────────
const TTS_CACHE_MAX_ENTRIES = 32;
const ttsAudioCache = new Map<string, string>();

function normalizeTtsText(text: string) {
  return text.trim().replace(/\r\n/g, "\n");
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

function base64ToBytes(base64: string): Uint8Array {
  const raw = atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
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

/** 停止当前朗读（若有），并熄灭「朗读中」标识。 */
export function stopSpeaking() {
  preempt();
  useTtsStore.getState().setSpeakingId(null);
}

/**
 * 边收边播的 PCM16LE 播放器。
 * 逐块把 base64 PCM 调度进 AudioContext，按到达顺序无缝排布；上游流结束（`markInputComplete`）
 * 且所有已排块播完时，`done` promise 兑现。
 */
class StreamingPcmPlayer {
  private ctx: AudioContext | null = null;
  private nextTime = 0;
  private pending = new Set<AudioBufferSourceNode>();
  private receivedChunks = 0;
  private expectedChunks: number | null = null;
  private leftover: Uint8Array | null = null; // 跨块残留的奇数尾字节
  private stopped = false;
  private settled = false;
  private resolveDone!: () => void;
  readonly done: Promise<void>;

  constructor() {
    this.done = new Promise((resolve) => {
      this.resolveDone = resolve;
    });
  }

  pushChunk(base64Pcm: string, sampleRate: number) {
    if (this.stopped) return;
    const rate = sampleRate || 24000;
    try {
      if (!this.ctx) {
        const Ctor =
          window.AudioContext ||
          (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
        this.ctx = new Ctor();
      }
      const ctx = this.ctx;

      // 拼接上一块残留的奇数字节，保证 Int16 对齐
      let bytes = base64ToBytes(base64Pcm);
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
      this.receivedChunks++;
      if (bytes.length === 0) {
        this.maybeFinish();
        return;
      }

      const int16 = new Int16Array(bytes.buffer, bytes.byteOffset, bytes.length / 2);
      const float = new Float32Array(int16.length);
      for (let i = 0; i < int16.length; i++) float[i] = int16[i] / 32768;

      // 按 PCM 声明的采样率建 buffer，交给 AudioContext 重采样到其输出率，避免变调
      const buffer = ctx.createBuffer(1, float.length, rate);
      buffer.copyToChannel(float, 0);
      const src = ctx.createBufferSource();
      src.buffer = buffer;
      src.connect(ctx.destination);

      const startAt = Math.max(this.nextTime, ctx.currentTime);
      src.start(startAt);
      this.nextTime = startAt + buffer.duration;
      this.pending.add(src);
      src.onended = () => {
        this.pending.delete(src);
        this.maybeFinish();
      };
    } catch (e) {
      appLog.error("[TTS] 流式分块播放失败: " + String(e));
      this.receivedChunks++;
      this.maybeFinish();
    }
  }

  /** 上游流已结束，共收到 totalChunks 个分块。 */
  markInputComplete(totalChunks: number) {
    this.expectedChunks = totalChunks;
    this.maybeFinish();
  }

  private maybeFinish() {
    if (this.stopped || this.settled) return;
    if (
      this.expectedChunks !== null &&
      this.receivedChunks >= this.expectedChunks &&
      this.pending.size === 0
    ) {
      this.settled = true;
      this.resolveDone();
    }
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
    if (this.ctx) {
      this.ctx.close().catch(() => {});
      this.ctx = null;
    }
    if (!this.settled) {
      this.settled = true;
      this.resolveDone();
    }
  }
}

/** 用 HTMLAudioElement 播放整段 base64 音频，播完 / 出错时兑现；期间登记 stopCurrent。 */
function playWholeAudio(base64Audio: string): Promise<void> {
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
      () => appLog.info("[TTS] 音频播放开始, mime=" + mime),
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

  const wantStream = settings.speech?.stream_playback !== false && isStreamPlaybackSupported();

  if (wantStream) {
    appLog.info("[TTS] 前端缓存未命中，发起流式语音请求 (" + id + ")");
    const sessionId = `${id}-${gen}`;
    const player = new StreamingPcmPlayer();
    stopCurrent = () => player.stop();

    const unlisten = await listen<{
      sessionId: string;
      seq: number;
      data: string;
      sampleRate: number;
    }>("tts-chunk", (e) => {
      if (e.payload.sessionId !== sessionId || gen !== playGen) return;
      player.pushChunk(e.payload.data, e.payload.sampleRate);
    });

    try {
      const resp = await synthesizeSpeechStream(normalized, sessionId);
      setCachedAudio(key, resp.audio);
      if (gen !== playGen) {
        player.stop();
        return;
      }
      if (resp.chunkCount > 0) {
        appLog.info("[TTS] 流式播放中, 分块数=" + resp.chunkCount);
        player.markInputComplete(resp.chunkCount);
        await player.done;
      } else {
        // 没有流式分块（后端缓存命中 / audio/speech 协议 / 服务端未流式）→ 整段播放
        player.stop();
        if (gen === playGen && resp.audio) await playWholeAudio(resp.audio);
      }
    } finally {
      unlisten();
    }
    return;
  }

  // 不走流式：普通请求 + 整段播放
  appLog.info("[TTS] 前端缓存未命中，发起整段语音请求 (" + id + ")");
  const audio = await synthesizeSpeech(normalized);
  setCachedAudio(key, audio);
  if (gen !== playGen) return;
  if (audio) await playWholeAudio(audio);
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
      useTtsStore.getState().setSpeakingId(null);
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
      stopCurrent = null;
    }
  }
}

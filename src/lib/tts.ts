import { Channel } from "@tauri-apps/api/core";
import { synthesizeSpeech, synthesizeSpeechStream } from "./invoke";
import type { SpeechResponse, TtsStreamMessage, TtsStreamPayload } from "./invoke";
import { appLog } from "../stores/logStore";
import { useTtsStore } from "../stores/ttsStore";
import { useSettingsStore, resolveActiveProvider } from "../stores/settingsStore";

// ── 完整音频前端缓存（LRU，与后端进程内缓存互补，减少重复请求）──────────────
const TTS_CACHE_MAX_ENTRIES = 32;
/**
 * 缓存总字符数上限（base64 长度累加）。只按条数限制是不够的：一分钟语音的 WAV base64
 * 就有 ~3.8MB，32 条塞满能吃掉上百 MB JS 堆。
 */
const TTS_CACHE_MAX_CHARS = 32 * 1024 * 1024;
const ttsAudioCache = new Map<string, string>();
let ttsCacheChars = 0;

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

function dropCachedAudio(key: string) {
  const old = ttsAudioCache.get(key);
  if (old === undefined) return;
  ttsAudioCache.delete(key);
  ttsCacheChars -= old.length;
}

function setCachedAudio(key: string, value: string) {
  if (!value) return;
  dropCachedAudio(key);
  ttsAudioCache.set(key, value);
  ttsCacheChars += value.length;
  // 条数、字符数两个上限都要满足；但至少留住刚写入的这条，
  // 否则单条就超预算时会被立刻淘汰，缓存永远命不中
  while (
    ttsAudioCache.size > TTS_CACHE_MAX_ENTRIES ||
    (ttsCacheChars > TTS_CACHE_MAX_CHARS && ttsAudioCache.size > 1)
  ) {
    const oldestKey = ttsAudioCache.keys().next().value;
    if (oldestKey === undefined) break;
    dropCachedAudio(oldestKey);
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
// 单例而非每处随手 new：WebKit 对同时存在的 AudioContext 数量有硬上限。但**不跨朗读会话
// 复用**——见 `resetAudioContext()`，每次流式朗读前会先关掉旧的再建新的（始终只有一个存活）。
let sharedCtx: AudioContext | null = null;

function getAudioContext(): AudioContext | null {
  if (sharedCtx && sharedCtx.state !== "closed") return sharedCtx;
  const Ctor =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  sharedCtx = new Ctor();
  warmupUntil = 0;
  keepAliveSrc = null;
  appLog.info(
    "[TTS] AudioContext 已创建, state=" + sharedCtx.state + ", sampleRate=" + sharedCtx.sampleRate
  );
  return sharedCtx;
}

/**
 * 关闭并丢弃当前 AudioContext，下次取用时重建。
 *
 * 为什么必须重建而不能复用：主窗口失焦会自动隐藏，窗口一隐藏 WKWebView 就被标记为遮挡、
 * 底层音频单元被停掉；窗口再显示时，WebKit **不会**为一个仍处于 `running` 的 context 重新
 * 拉起音频单元，而是继续用定时器驱动的时钟「空转渲染」——`state` 依旧是 `running`、
 * `currentTime` 正常推进、`onended` 照常触发，样本却根本没送到输出设备。表现就是
 * 「第一次朗读有声，之后每次都完全无声，但手动点一下走 `<audio>` 整段播又是好的」。
 * 这种哑火在 JS 侧没有任何可查的状态位，只能靠换一个新 context 规避。
 *
 * 重建成本（创建 + 设备启动）被合成请求的 1s+ 网络往返和 `AUDIO_WARMUP_SECONDS` 静音预热
 * 吸收，听感上不增加起播延迟。
 */
async function resetAudioContext() {
  const ctx = sharedCtx;
  sharedCtx = null;
  warmupUntil = 0;
  keepAliveSrc = null;
  if (!ctx || ctx.state === "closed") return;
  try {
    await ctx.close();
  } catch (e) {
    appLog.warn("[TTS] 关闭旧 AudioContext 失败: " + String(e));
  }
}

/** 浏览器是否支持 Web Audio（边收边播依赖它，否则回退整段播放）。 */
export function isStreamPlaybackSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    !!(window.AudioContext || (window as unknown as { webkitAudioContext?: unknown }).webkitAudioContext)
  );
}

/**
 * 首块起播距预热开始的最小间隔（秒）——输出设备启动的最低保证。
 * 真正让设备保持运转的是 [`warmUpOutputDevice`] 排下的循环静音，这里只是个下限。
 */
const AUDIO_WARMUP_SECONDS = 0.35;
/**
 * 输出设备保活时长（秒）。`AudioContext` 刚创建 / 刚 `resume` 时底层设备还在启动
 * （蓝牙耳机可达 1s 以上），这段时间里已经排上时间线的 buffer 会被直接吞掉——表现就是
 * 「自动朗读第一次没声音，再点一次就正常」。
 *
 * 早先用一段 0.35s 静音唤醒设备，但设备启动比这慢时照样吞，而且静音放完到首块 PCM 到达
 * 之间还有 1s+ 的空档（合成 + 网络往返），设备可能又空转下去。改成排一段**循环静音**把
 * 设备一直拽着跑，覆盖整个等待期，到点由 `stop(when)` 自动收，不用定时器。
 * 整段播放（`<audio>`）不受影响，因为媒体元素自己会等设备就绪，所以只有流式路径会踩到。
 */
const AUDIO_KEEPALIVE_SECONDS = 30;
/** 循环静音 buffer 的长度（秒）；只是个载体，取值不影响听感 */
const AUDIO_KEEPALIVE_BUFFER_SECONDS = 0.5;
/** ctx 时间轴上的「预热完成」时刻；ctx 重建时归零 */
let warmupUntil = 0;
/** 当前保活中的静音源；ctx 重建 / 自然结束时置空 */
let keepAliveSrc: AudioBufferSourceNode | null = null;

/** 排一段循环静音把输出设备拽着跑（已有保活在跑则跳过）。 */
function warmUpOutputDevice(ctx: AudioContext) {
  if (ctx.state !== "running" || keepAliveSrc) return;
  try {
    const frames = Math.max(1, Math.ceil(ctx.sampleRate * AUDIO_KEEPALIVE_BUFFER_SECONDS));
    const src = ctx.createBufferSource();
    src.buffer = ctx.createBuffer(1, frames, ctx.sampleRate);
    src.loop = true;
    src.connect(ctx.destination);
    src.start();
    src.stop(ctx.currentTime + AUDIO_KEEPALIVE_SECONDS);
    src.onended = () => {
      if (keepAliveSrc === src) keepAliveSrc = null;
    };
    keepAliveSrc = src;
    warmupUntil = ctx.currentTime + AUDIO_WARMUP_SECONDS;
  } catch (e) {
    appLog.warn("[TTS] 输出设备预热失败: " + String(e));
  }
}

/**
 * 拿到一个「确实在跑」的 AudioContext：必要时 resume 并**等待完成**，随后预热输出设备。
 * 返回 `null` 表示 Web Audio 不可用或起不来（如平台要求用户手势），调用方应回退整段播放。
 *
 * @param recreate 先关掉旧 context 再建新的（每次流式朗读都要传 `true`，原因见
 *   [`resetAudioContext`]：复用旧 context 会在窗口隐藏过一次后彻底哑火）。
 */
async function ensureAudioContextRunning(recreate = false): Promise<AudioContext | null> {
  if (recreate) await resetAudioContext();
  const ctx = getAudioContext();
  if (!ctx) return null;
  // 覆盖 suspended 与 Safari 的 interrupted（来电/其它 App 抢占音频后会停在这个状态）
  if (ctx.state !== "running") {
    try {
      await ctx.resume();
    } catch (e) {
      appLog.warn("[TTS] AudioContext resume 失败: " + String(e));
    }
  }
  if (ctx.state !== "running") {
    appLog.warn("[TTS] AudioContext 未运行 (state=" + ctx.state + ")");
    return null;
  }
  warmUpOutputDevice(ctx);
  return ctx;
}

let gestureUnlockArmed = false;

/**
 * 预热音频链路：提前建好 AudioContext 并唤醒输出设备（应用启动 / 主窗口显示时调用）。
 * 首次 `new AudioContext()` + 设备启动有几百毫秒开销，拖到第一段 PCM 到达时才做就会
 * 吞掉开头的声音。若此时起不来（平台要求用户手势），再挂一次性手势监听兜底。
 */
export function primeAudio() {
  ensureAudioContextRunning()
    .then((ctx) => {
      if (ctx || gestureUnlockArmed) return;
      gestureUnlockArmed = true;
      const unlock = () => {
        window.removeEventListener("pointerdown", unlock, true);
        window.removeEventListener("keydown", unlock, true);
        gestureUnlockArmed = false;
        ensureAudioContextRunning().then((c) => {
          if (c) appLog.info("[TTS] 用户手势后 AudioContext 已启动");
        });
      };
      window.addEventListener("pointerdown", unlock, true);
      window.addEventListener("keydown", unlock, true);
    })
    .catch((e) => appLog.warn("[TTS] 音频链路预热失败: " + String(e)));
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
 * 判定「上游不再发分块」的静默阈值（毫秒）。超过这么久没收到新块又没收到 `end`，
 * 才按已收到的分块收口。小米流式的正常节奏是每块 0.32s 音频，3s 已经宽松得多。
 */
const STREAM_IDLE_TIMEOUT_MS = 3000;

/**
 * 本地留存流式 PCM 的上限（字节）。24kHz 单声道 PCM16 下约合 5.8 分钟音频。
 * 超过就不再留存，也就拼不出完整 WAV 去回填缓存——超长朗读本来也不该常驻内存。
 */
const STREAM_PCM_KEEP_MAX_BYTES = 16 * 1024 * 1024;

/**
 * 通道二进制分块 → `ArrayBuffer`；不是二进制则返回 `null`（交给控制消息分支）。
 *
 * 正常情况下 Tauri 两条投递路径都直接给 `ArrayBuffer`（<1KB 走 eval 里的
 * `new Uint8Array([...]).buffer`，更大的走 IPC 自定义协议的 `response.arrayBuffer()`）。
 * 但自定义协议一旦失败会整体回退 `postMessage`，分块就可能变成普通数组/TypedArray——
 * 只认 `instanceof ArrayBuffer` 的话会被当成控制消息静默丢掉，表现为「完全没声音」。
 */
function asArrayBuffer(message: unknown): ArrayBuffer | null {
  if (message instanceof ArrayBuffer) return message;
  if (ArrayBuffer.isView(message)) {
    const view = message as ArrayBufferView;
    return view.buffer.slice(view.byteOffset, view.byteOffset + view.byteLength) as ArrayBuffer;
  }
  if (Array.isArray(message)) return Uint8Array.from(message as number[]).buffer;
  return null;
}

/** `Uint8Array` → base64；分段处理，避免 `String.fromCharCode` 参数过多爆栈 */
function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** 给裸 PCM16LE 套 44 字节 WAV 头（与后端 `wrap_pcm16_wav` 等价） */
function wrapPcm16Wav(pcm: Uint8Array, sampleRate: number, channels: number): Uint8Array {
  const BITS_PER_SAMPLE = 16;
  const blockAlign = (channels * BITS_PER_SAMPLE) / 8;
  const out = new Uint8Array(44 + pcm.length);
  const view = new DataView(out.buffer);
  const ascii = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i++) out[offset + i] = s.charCodeAt(i);
  };
  ascii(0, "RIFF");
  view.setUint32(4, 36 + pcm.length, true);
  ascii(8, "WAVEfmt ");
  view.setUint32(16, 16, true); // PCM fmt chunk 长度
  view.setUint16(20, 1, true); // 1 = PCM
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * blockAlign, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, BITS_PER_SAMPLE, true);
  ascii(36, "data");
  view.setUint32(40, pcm.length, true);
  out.set(pcm, 44);
  return out;
}

/**
 * 边收边播的 PCM16LE 播放器。
 * 逐块把二进制 PCM 调度进 AudioContext，按到达顺序无缝排布；上游流结束
 * （`markInputComplete`）且所有已排块播完时，`done` promise 兑现。
 */
class StreamingPcmPlayer {
  private sampleRate = 24000;
  private nextTime = 0;
  private pending = new Set<AudioBufferSourceNode>();
  private scheduled = 0;
  private leftover: Uint8Array | null = null; // 跨块残留的奇数尾字节
  private inputDone = false;
  private stopped = false;
  private settled = false;
  private drainTimer: ReturnType<typeof setTimeout> | null = null;
  private startedWallMs = 0; // 首块排入时的墙上时钟，用于判断时间线是否真的走完
  private firstStartAt = 0; // 首块在 ctx 时间线上的起播时刻
  private endedCount = 0; // 实际触发 onended 的 source 数（时间线真在推进的证据）
  private lastChunkWallMs = performance.now(); // 最近一次收到分块的墙上时钟
  private pcmParts: Uint8Array[] = []; // 已播 PCM 原样留一份，用于本地拼完整 WAV
  private pcmBytes = 0;
  private pcmDropped = false; // 超上限已放弃留存，本地拼不出完整音频
  private wavBase64: string | null = null; // buildWavBase64() 的结果，只算一次
  private resolveDone!: () => void;
  readonly done: Promise<void>;

  /**
   * @param ctx 已确认处于 `running` 的共享 AudioContext（由 `ensureAudioContextRunning` 取得）
   * @param onFirstAudio 第一块 PCM 排入播放时回调一次（用于熄灭按钮的加载态）。
   */
  constructor(
    private ctx: AudioContext,
    private onFirstAudio?: () => void
  ) {
    this.done = new Promise((resolve) => {
      this.resolveDone = resolve;
    });
  }

  /** 已实际排入播放的分块数。 */
  get scheduledChunks() {
    return this.scheduled;
  }

  /** 距最近一次收到分块过了多久（毫秒）——用来判断上游是不是真的不发了。 */
  idleMs() {
    return performance.now() - this.lastChunkWallMs;
  }

  /**
   * 把已收到的 PCM 拼成完整 WAV 的 base64；留存被放弃（超上限）时返回空串。
   *
   * 用途有两个：① 流式播完后回填前端缓存（后端流式路径不回传完整音频，不回填的话
   * 下次重播还要再走一次 IPC 把几 MB base64 搬回来）；② 哑火兜底时直接拿它整段重播，
   * 省掉一次 `synthesize_speech` 往返。**只在收到 `end` 后调用**，否则可能是截断的。
   */
  buildWavBase64(): string {
    if (this.wavBase64 !== null) return this.wavBase64;
    if (this.pcmBytes === 0 || this.pcmDropped) return "";
    const pcm = new Uint8Array(this.pcmBytes);
    let offset = 0;
    for (const part of this.pcmParts) {
      pcm.set(part, offset);
      offset += part.length;
    }
    this.wavBase64 = bytesToBase64(wrapPcm16Wav(pcm, this.sampleRate, 1));
    // 拼完就把分片放掉，别让原始 PCM 和 base64 两份同时压在堆上
    this.pcmParts = [];
    return this.wavBase64;
  }

  /**
   * 时间线是否真的走完了（`done` 兑现后再读）。
   *
   * WebKit 里长期闲置的 AudioContext 底层音频单元可能已经停掉，但 `state` 仍报 `running`：
   * 分块能排进时间线、`onended` 也照常触发，扬声器却一声不响。此时「墙上时钟耗时」会远小于
   * 「音频总时长」，据此识别出这种哑火，调用方可回退整段播放。
   * 被中止 / 没排过块的情况不做判断（返回 `true`）。
   */
  get playedThrough(): boolean {
    if (this.stopped || !this.startedWallMs || this.scheduled === 0) return true;
    const duration = this.nextTime - this.firstStartAt;
    if (duration <= 1) return true; // 太短，墙上时钟的误差比信号还大
    return (performance.now() - this.startedWallMs) / 1000 >= duration * 0.5;
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
    this.lastChunkWallMs = performance.now();
    try {
      const ctx = this.ctx;
      // 播放途中 context 可能被系统挂起（设备切换 / 页面隐藏），不 resume 会一声不响地什么都不播
      if (ctx.state !== "running") ctx.resume().catch(() => {});

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

      // 原样留一份，结束后能在本地拼出完整 WAV（见 buildWavBase64）。
      // 超上限就放弃留存：AudioBuffer 已经占了 2 倍于 PCM 的堆，再叠 PCM + base64
      // 三份对超长朗读来说太重，这种情况退回「兜底时问后端要」。
      if (!this.pcmDropped) {
        this.pcmParts.push(bytes);
        this.pcmBytes += bytes.length;
        if (this.pcmBytes > STREAM_PCM_KEEP_MAX_BYTES) {
          appLog.warn(
            "[TTS] 流式 PCM 超过 " +
              Math.round(STREAM_PCM_KEEP_MAX_BYTES / 1024 / 1024) +
              "MB，放弃本地留存（不回填前端缓存）"
          );
          this.pcmDropped = true;
          this.pcmParts = [];
          this.pcmBytes = 0;
        }
      }

      const int16 = new Int16Array(bytes.buffer, bytes.byteOffset, bytes.length / 2);
      const float = new Float32Array(int16.length);
      for (let i = 0; i < int16.length; i++) float[i] = int16[i] / 32768;

      // 按 PCM 声明的采样率建 buffer，交给 AudioContext 重采样到其输出率，避免变调
      const buffer = ctx.createBuffer(1, float.length, this.sampleRate);
      buffer.copyToChannel(float, 0);
      const src = ctx.createBufferSource();
      src.buffer = buffer;
      src.connect(ctx.destination);

      // 首块除了留抖动缓冲，还必须排在输出设备预热完成之后，否则会被冷启动的设备吞掉
      const startAt =
        this.scheduled === 0
          ? Math.max(ctx.currentTime + STREAM_PREROLL_SECONDS, warmupUntil)
          : Math.max(this.nextTime, ctx.currentTime);
      src.start(startAt);
      this.nextTime = startAt + buffer.duration;
      this.scheduled++;
      if (this.scheduled === 1) {
        this.startedWallMs = performance.now();
        this.firstStartAt = startAt;
        // 峰值幅度：≈0 说明拿到的 PCM 本身就是静音（后端/解码问题）；正常人声在 0.1~1.0
        let peak = 0;
        for (let i = 0; i < float.length; i++) {
          const v = float[i] < 0 ? -float[i] : float[i];
          if (v > peak) peak = v;
        }
        appLog.info(
          "[TTS] 首块已排入播放, 起播延迟=" +
            (startAt - ctx.currentTime).toFixed(2) +
            "s, ctx=" +
            ctx.state +
            ", ctx采样率=" +
            ctx.sampleRate +
            ", PCM采样率=" +
            this.sampleRate +
            ", 块时长=" +
            buffer.duration.toFixed(3) +
            "s, 峰值=" +
            peak.toFixed(3)
        );
        if (this.onFirstAudio) {
          const notify = this.onFirstAudio;
          this.onFirstAudio = undefined;
          notify();
        }
      }
      this.pending.add(src);
      src.onended = () => {
        this.endedCount++;
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
    appLog.info(
      "[TTS] 上游流结束, 已排入=" +
        this.scheduled +
        ", 待播=" +
        this.pending.size +
        ", 剩余时间线=" +
        (this.nextTime - this.ctx.currentTime).toFixed(2) +
        "s"
    );
    this.armDrainWatchdog();
    this.maybeFinish();
  }

  /**
   * 兜底定时器：正常情况下靠 `onended` 收尾，但若 AudioContext 被系统挂起、或某个
   * source 的 `onended` 没有触发，这里保证 `done` 不会永远挂着（表现为「朗读中」不熄）。
   */
  private armDrainWatchdog() {
    if (this.drainTimer) return;
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
      appLog.warn(
        "[TTS] 流式播放收尾异常，强制结束 (剩余分块=" + this.pending.size + ", ctx=" + ctx.state + ")"
      );
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
    // 实际耗时应约等于音频总时长；若远小于则说明 source 根本没在时间线上走（输出设备没跑）
    if (this.startedWallMs) {
      appLog.info(
        "[TTS] 流式播放收尾, 实际耗时=" +
          ((performance.now() - this.startedWallMs) / 1000).toFixed(2) +
          "s, 音频总时长≈" +
          (this.scheduled ? (this.nextTime - this.firstStartAt).toFixed(2) : "0") +
          "s, 已播完分块=" +
          this.endedCount +
          "/" +
          this.scheduled +
          ", 被中止=" +
          this.stopped
      );
    }
    this.resolveDone();
  }

  stop() {
    if (this.stopped) return;
    this.stopped = true;
    this.leftover = null;
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
  const key = getTtsCacheKey(tts.base_url, tts.model, tts.extra, normalized);

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
    // 合成 + 网络往返通常要 1s 以上，正好用这段时间重建 context 并唤醒输出设备：
    // 等第一段 PCM 到达时设备已经在跑，不会被冷启动吞掉开头。
    // 必须重建而非复用——旧 context 在主窗口隐藏过一次后会「空转渲染」，详见 resetAudioContext()
    const ctx = wantStream ? await ensureAudioContextRunning(true) : null;
    if (gen !== playGen) return;
    if (wantStream && !ctx) {
      appLog.warn("[TTS] Web Audio 未就绪，本次回退整段播放 (" + id + ")");
    }

    if (ctx) {
      appLog.info("[TTS] 前端缓存未命中，发起流式语音请求 (" + id + ")");
      const player = new StreamingPcmPlayer(ctx, clearLoading);
      stopCurrent = () => player.stop();

      // 通道按发送顺序投递，因此 end 一定排在所有分块之后；不依赖它与命令返回值的先后
      let endReceived = false;
      const channel = new Channel<TtsStreamPayload>();
      channel.onmessage = (message) => {
        if (gen !== playGen) return;
        const pcm = asArrayBuffer(message);
        if (pcm) {
          player.pushChunk(pcm);
          return;
        }
        const control = message as TtsStreamMessage;
        if (control?.event === "start") {
          player.setFormat(control.sampleRate, control.channels);
        } else if (control?.event === "end") {
          endReceived = true;
          player.markInputComplete();
        } else {
          // 不认识就丢会变成「完全没声音且毫无线索」，至少留条日志
          appLog.warn("[TTS] 收到无法识别的通道消息，已忽略: " + typeof message);
        }
      };

      let resp: SpeechResponse;
      try {
        resp = await synthesizeSpeechStream(normalized, channel);
      } catch (e) {
        // 合成失败但分块可能已经排进时间线：不停掉就会变成 UI 已熄灭、stopSpeaking 也抓不到的幽灵音频
        player.stop();
        throw e;
      }
      if (gen !== playGen) {
        player.stop();
        return;
      }
      if (resp.chunkCount > 0) {
        appLog.info(
          "[TTS] 流式播放中, 分块数=" + resp.chunkCount + ", 已排入播放=" + player.scheduledChunks
        );
        // 兜底：万一 end 控制消息丢失（通道某条消息投递失败会卡住后续消息），也别让 done 永远挂着。
        // 用「分块停止到达」而不是「invoke 返回后固定 5s」判定：命令返回时分块往往还在路上，
        // 定时收口会在播放中途提前兑现 done，让下面的兜底和还在播的流式音频叠在一起。
        const guard = setInterval(() => {
          if (endReceived || player.idleMs() < STREAM_IDLE_TIMEOUT_MS) return;
          appLog.warn(
            "[TTS] 未收到流结束消息且分块已停止到达 " +
              Math.round(player.idleMs()) +
              "ms，按已收到的分块收尾"
          );
          player.markInputComplete();
        }, 1000);
        try {
          await player.done;
        } finally {
          clearInterval(guard);
        }

        // 哑火判定分两种：
        // ① 后端说推了 N 块、前端一块都没排上 —— 通道分块被丢弃，必然静音（这条最确定）
        // ② 块排进了时间线却没真正出声 —— WebKit 闲置 context 音频单元已停（playedThrough 启发式）
        const missing = resp.chunkCount - player.scheduledChunks;
        if (missing > 0) {
          appLog.warn(
            "[TTS] 有 " +
              missing +
              " 个分块未排入播放 (后端=" +
              resp.chunkCount +
              ", 前端=" +
              player.scheduledChunks +
              ")"
          );
        }
        const silent = player.scheduledChunks === 0 || !player.playedThrough;
        // 收全了才敢当完整音频用；被 guard 提前收口时手上这份可能是截断的
        const complete = endReceived ? player.buildWavBase64() : "";
        if (gen === playGen && silent) {
          appLog.warn("[TTS] 流式播放疑似未出声，回退整段播放 (" + id + ")");
          // 必须先停：兜底那一路走 <audio>，会把 stopCurrent 改写成 audio.pause，
          // 此时若流式播放器还活着，两路声音会重叠且再也停不掉
          player.stop();
          // 优先用本地已收到的 PCM，省掉一次几 MB 的 IPC 往返；拿不到才回后端要
          const audio = complete || (await synthesizeSpeech(normalized));
          setCachedAudio(key, audio);
          // 带上 clearLoading：一块都没排上时 onFirstAudio 没触发过，
          // 不在这里熄灯按钮会一直转到整段播完
          if (gen === playGen && audio) await playWholeAudio(audio, clearLoading);
        } else if (complete) {
          // 回填前端缓存：流式路径后端不回传完整音频，不回填的话下次重播还要再走一趟 IPC
          setCachedAudio(key, complete);
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
      stopCurrent = null;
    }
  }
}

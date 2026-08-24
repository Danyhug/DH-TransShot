# TTS 语音合成（tts/）

## 概述

将文本转为语音。合成结果是 **base64 编码的完整音频**（写进缓存供重播），chat+audio 流式路径则把 PCM 分块边收边送进本地输出设备。

> **播放不在前端**：音频由 [audio/](audio.md) 直接送进操作系统输出设备，一个字节都不过 IPC。原因见那篇文档——主窗口失焦自动隐藏后，WebKit 会让 `AudioContext` 空转渲染，日志一切正常却一声不响。

根据用户配置的 TTS 端点**自适应选择两种协议**：

| 协议 | 触发条件 | 端点 | 请求体 | 响应 |
|------|---------|------|--------|------|
| **audio/speech** | 端点非 `chat/completions`（默认） | `/v1/audio/speech` | `{model, input, voice, response_format}` | 二进制音频 |
| **chat+audio（非流式）** | 端点以 `chat/completions` 结尾，`extra.stream=false` | `/v1/chat/completions` | `{model, messages, audio:{format,voice}}` | JSON，音频在 `choices[0].message.audio.data`（base64） |
| **chat+audio（流式，默认）** | 端点以 `chat/completions` 结尾 | `/v1/chat/completions` | `{model, messages, audio:{format:"pcm16",voice}, stream:true}` | SSE，逐块 `choices[0].delta.audio.data`（base64 PCM16LE） |

- audio/speech：OpenAI 兼容标准 TTS（如 SiliconFlow `FunAudioLLM/CosyVoice2`）
- chat+audio：小米 MiMo 式「聊天补全 + 音频输出」TTS（如 `mimo-v2.5-tts`），文本走 `messages`、音色走 `audio` 对象
- chat+audio **默认走 SSE 流式**（`stream:true` + `pcm16`），分块实时回传前端播放；结束后把所有 PCM 拼成 WAV 返回（供缓存/重播）。可用 `extra.stream=false` 关掉流式
  - 注意：这是**本项目的默认选择**（请求体里显式发 `"stream": true`），小米 API 自身的 `stream` 默认值是 `false`

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tts/mod.rs` | TTS 服务层：解析端点、按协议构建请求、发送 HTTP、返回 base64 音频 |

## 核心逻辑

### mod.rs

**`synthesize(client, base_url, api_key, model, extra, text, on_chunk) -> Result<SpeechResult>`（协议分发入口）**

1. 校验 `base_url` 非空
2. `resolve_tts_endpoint_url(base_url)` 解析端点 URL（复用 `build_endpoint_url`，`endpoint="audio/speech"`）
3. `is_chat_audio_endpoint(url)` 判定：URL 去掉 query/尾斜杠后以 `chat/completions` 结尾 → 走 chat+audio，否则走 audio/speech
4. 两条路径最终都返回 `SpeechResult`

**`SpeechResult` — 合成结果**

| 字段 | 说明 |
|------|------|
| `audio_base64` | 完整音频 base64（流式路径为 PCM 拼接后套 WAV 头） |
| `chunk_count` | 已边收边播出去的流式分块数；`0` 表示未走流式分块（命中缓存/非 chat+audio/服务端未流式/关掉了边收边播），此时命令层整段播 `audio_base64` |
| `sample_rate` | 流式分块采样率（Hz），非流式为 0 |

**`StreamSink` / `ChunkSink<'a>` / `on_chunk`** — 流式分块出口，两个方法：

| 方法 | 说明 |
|------|------|
| `chunk(&[u8])` | 一块**已解码的 PCM16LE 裸字节**（不是 base64）；命令层把它直接 `push_pcm16` 进输出设备 |
| `discard()` | 丢掉已经排进播放队列的音频。**流刚开头就断、要整段重来时调用**，不丢的话重来的整段音频会和它叠在一起 |

`None` 时仍会累积拼成完整音频，只是不边收边播（`speech.stream_playback` 关掉时就是这样）。

> `SpeechResult` 的 `Debug` 是**手写**的，只打 `audio_base64_len` 而不是内容——derive 出来的实现会把几 MB base64 整段塞进日志/panic 信息。

**`StreamAccum<'a>` — 流式解析的累积状态**（`on_chunk` / `pcm` / `chunk_count` / `first_chunk_ms` / `started`）

由 `synthesize_chat_audio` 持有、以 `&mut` 传进 `synthesize_chat_audio_stream`，而不是后者内部自造。这样流中途失败时，调用方仍能读到 `chunk_count`（已推给前端多少块），据此决定**能否回退非流式**。核心方法 `handle_line(&str) -> Result<bool>`：解析一条 SSE 行、必要时推块，返回值表示「这行是不是 `data:` 行」（`saw_sse` 的判据）。

**`synthesize_audio_speech(...)` — 标准协议**

1. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
2. `merge_extra()` **整包合并** `extra`（可覆盖 `voice`、`speed`、`gain`、`response_format` 等）
3. `POST` → 检查状态码 → 读取二进制 → base64 编码，包成 `SpeechResult::whole`（`chunk_count=0`）

**`synthesize_chat_audio(...)` — 小米 chat+audio 协议入口**

1. `parse_chat_audio_options(extra)` 解析 `extra`（JSON），**只解释五个键**（不整包合并，避免 audio/speech 字段污染 chat 请求体）：
   - `voice`：音色裸名字，默认 `mimo_default`（跟随集群，中国区=冰糖中文女声）；**忽略含 `/` 的 model-scoped 音色**（如 `FunAudioLLM/...:alex`，那是另一协议的值），忽略时打 `warn` 日志提示改用裸音色名
   - `format`：**非流式**音频格式，默认 `wav`（仅认 `format` 键，不复用 `response_format`）
   - `style`：自然语言风格指令（`user` 消息），默认中性提示；置为空串则不发送 `user` 消息
   - `prefix`：**行首风格标签**（如 `(语速偏慢)`），拼在 `assistant` 目标文本最前面，默认空串=不加；经 `normalize_style_prefix` 规范化（裸文本自动补半角括号）
   - `stream`：是否走 SSE 流式，默认 `true`
2. `stream=true` → 先试 `synthesize_chat_audio_stream`，失败降级 `synthesize_chat_audio_once`；`stream=false` → 直接非流式
   - **降不降级看「已经播出去多久」**（`accum.played_secs()`，由累积 PCM 字节数换算），不看分块数：一块只有 0.16~0.32s，块数多少跟用户到底听到没听到不是一回事
   - `≥ MIN_PLAYED_SECS_TO_KEEP`（2s）→ **不降级**，直接返回错误（带「已播放 X.Xs」）：那段音频已经在扬声器上响过了，再合成一遍是多花一次钱、多等几秒，听感还是「读到一半跳回开头重读」
   - `< 2s` → `accum.discard()` 丢掉已播的那点音频，再走非流式整段重来。旧规则是「推过块就绝不降级」，线上因此踩过一次：501 字的文本只推了 1 块（0.16s）就断流，结果是「什么都没播还报个错」

**`synthesize_chat_audio_once(...)` — chat+audio 非流式**

1. 组织 `messages`（`build_chat_audio_messages`）：`user`=自然语言风格指令，`assistant`=行首风格标签(`prefix`) + 要朗读的文本
2. 构建请求体：`{ model, messages, audio: { format, voice } }`
3. `POST` → 检查状态码 → 解析 JSON，取 `choices[0].message.audio.data`（**已是 base64，直接返回**）

**`synthesize_chat_audio_stream(...)` — chat+audio 流式（默认）**

1. 请求体额外带 `stream:true`，音频格式**固定 `pcm16`**（官方要求，只有裸 PCM 分块能直接拼接）
2. 用 `response.bytes_stream()` 逐块读取，按行交给 `StreamAccum::handle_line` 解析 SSE（`data: {...}`）：
   - `extract_stream_audio_data` 从 `choices[0].delta.audio.data`（兼容末块 `message.audio.data`）取 base64 PCM
   - 每块 base64 解码后**先** `on_chunk(&bytes)` 实时回传前端、再累积到 PCM 缓冲（首帧延迟优先）；跨块残留的行/字节留到下次
   - 收到**非 null** 的 `error` 字段立即 bail。⚠️ 必须 `.filter(|e| !e.is_null())`：`Value::get` 对「字段存在但值为 null」返回 `Some(Value::Null)`，而不少 OpenAI 兼容网关每一帧都带 `"error": null`，不滤就会第一帧即失败，表现为「边收边播开着却每次都等整段合成完」
3. **收尾补一次 `handle_line`**：末行可能不带换行符（服务端直接断流），只靠「找 `\n`」的循环会把最后一块音频丢在残留缓冲里
4. 若整个响应**不含任何 `data:` 行**（服务端不支持 stream 的兼容端点）→ 整体当普通 JSON 解析，退回非流式结果
   - 解析用的是**另存的完整原文 `raw`**，不是残留的 `buffer`：逐行消费会把所有非 `data:` 行 drain 掉丢弃，格式化过（带换行）的 JSON 就这样被切碎，只剩最后一行必然解析失败。`raw` 只在确认走了 SSE 之前累积，之后立即释放，不给长音频白占一份内存
5. 结束后 `wrap_pcm16_wav(pcm, 24000, 1)` 套 44 字节 WAV 头 → base64，返回 `SpeechResult{ audio_base64, chunk_count, sample_rate:24000 }`

**辅助函数：**
- `resolve_tts_endpoint_url(base_url)` — `build_endpoint_url(base_url, "audio/speech")` 自适应拼接（根/版本段/完整端点/`#` raw，规则见 [config.md](config.md)）。用户用 `#` raw 或完整路径指向 `.../chat/completions` 时原样返回该 chat 端点
- `is_chat_audio_endpoint(url)` — 端点是否为 Chat Completions（决定走哪条协议）
- `stream_sample_rate()` / `stream_channels()` — 暴露流式 PCM 参数常量（24000 / 1）给命令层填入 `start` 控制消息
- `wrap_pcm16_wav(pcm, sample_rate, channels)` — 给裸 PCM16LE 套标准 WAV 头
- `normalize_style_prefix(raw)` — 规范化 `extra.prefix`：空白→空串；已以 `(`/`（`/`[` 开头→原样；否则补一对半角括号。**不自动补括号的话裸文本会被模型当正文念出来**（听感是音频开头凭空多几个字，看不出是配置写法问题）
- `extract_stream_audio_data(value)` — 从一条 SSE 分块里取音频 base64（优先 `delta`，兼容 `message`）

### commands/tts.rs

**`speak_text(state, text, on_event) -> Result<(), String>`** — 朗读一段文本：合成 → 送进本地输出设备 → **播完才返回**（前端 `speakText`）。

1. 规范化文本（`trim()` + `CRLF -> LF`），空文本直接返回
2. 读 `settings.speech.stream_playback` 决定要不要边收边播（**这个开关现在在后端判**，前端不再参与）
3. `state.audio.begin(24000, 1)` 打开输出设备——这一步同时**抢占**上一段朗读
4. `synthesize_inner(...)`，边收边播时传一个 `PlaybackSink`：
   - `chunk()`：先查 `is_current()`（被抢占就不再入队，别往关掉的设备上堆几 MB），再 `push_pcm16`；第一块入队时发一条 `{"event":"start"}`
   - `discard()`：转调 `Playback::discard()`
5. `chunk_count == 0`（命中缓存 / audio\_speech 协议 / 服务端未流式 / 关掉了边收边播）→ base64 解码后 `play_encoded` 整段播，并发 `start`
6. 轮询等播完（`DRAIN_POLL_INTERVAL` 100ms），两条退出路径：
   - 被抢占（`is_current()` 为假）——那时设备已关，队列不会再播空，死等会挂住
   - **播放停滞**：`position()` 连续 `PLAYBACK_STALL_TIMEOUT`（10s）没变 → 输出设备多半断开了，提前收场
7. `state.audio.finish(generation)` 关设备

**`stop_speech(state)`** — `state.audio.stop()`，停当前朗读并关设备（前端 `stopSpeech`）。

> **前端不需要「先 stop 再 speak」**：`speak_text` 自己就抢占上一段，而两个 invoke 谁先到达没有保证——先发 stop 再发 speak，stop 反而可能后到、把新的这段停掉。只有明确的「停止朗读」才调 `stop_speech`。

**`on_event: Channel<InvokeResponseBody>`** 上现在只剩一种控制消息：

```json
{"event":"start"}
```

含义是「第一段音频已送入输出设备」，前端据此熄灭按钮的加载态。**音频数据不再经过 IPC**——之前几百个 PCM 分块走 Channel 二进制、前端还要 `asArrayBuffer()` 兼容各种投递形态，这些现在全没了。

**`synthesize_inner` 流程：**
1. 从 `AppState.settings` 读取当前生效 TTS 配置（`tts.resolved(...)` 按 `active` 选默认或某个 provider，**四元组里已包含解析后的 `extra`**：provider 自带 extra 优先，留空才用服务级共享 extra）
2. 使用 `base_url + model + extra + text` 生成缓存键
3. 命中 `AppState.tts_cache` 直接返回缓存 base64（`chunk_count=0`，不走边收边播）；未命中调用 `tts::synthesize()`，成功后写入缓存

缓存为进程内内存缓存（`config::settings::TtsCache`），**条数与总字节数双上限**：最多 64 条、总计 64MB（`TTS_CACHE_MAX_ENTRIES` / `TTS_CACHE_MAX_BYTES`），按插入顺序淘汰，覆盖同键时正确回退字节计数；单条自身就超预算时保留它不自我淘汰。只按条数限制是不够的——一分钟语音拼出来的 WAV base64 就有 ~3.8MB，64 条塞满能占几百 MB 常驻内存。

保存设置时清空缓存。缓存键与协议无关（不同协议因 base_url 不同天然不撞键）。缓存存的始终是「完整音频 base64」，命中时整段播、不走边收边播。**流式路径也要写缓存**：流式返回值本身不回传给前端，重播全靠这份缓存。

## 超时

共享的 `reqwest::Client` 没设任何超时，TTS 这边自己兜：

| 常量 | 值 | 作用 |
|------|----|------|
| `STREAM_IDLE_TIMEOUT` | 20s | 流式：等响应头、以及**两次收到数据之间**的静默上限 |
| `WHOLE_REQUEST_TIMEOUT` | 120s | 非流式（audio/speech、chat+audio 非流式）的整体请求超时 |

- 流式的静默上限用 `tokio::time::timeout` 包住每次 `stream.next()`，**不能用 `RequestBuilder::timeout`**——那个会把整段流式响应体也算进去，长文本必然误杀
- 20s 的依据：小米首块通常 1~8s 到、块间隔 0.32s 量级。线上出现过「首块之后 15s 才断流」，整段朗读白等

## 播放

音频由 [audio/](audio.md) 送进操作系统输出设备，前端 `lib/tts.ts` 只剩编排（抢占、按钮状态、长度统计）。
详见 [audio.md](audio.md) 与 [frontend/lib.md](../frontend/lib.md)。

## API 请求格式

### audio/speech 协议

```json
{
  "model": "FunAudioLLM/CosyVoice2-0.5B",
  "input": "要朗读的文本",
  "voice": "FunAudioLLM/CosyVoice2-0.5B:alex",
  "response_format": "mp3"
}
```

默认 voice 为 `{model}:alex`，可用 voice（以 SiliconFlow 为例）：alex, anna, bella, benjamin, charles, claire, david, diana。可通过 `extra` 覆盖 `voice`/`speed` 等。

**参数取值范围**（据 SiliconFlow 官方 API 文档 `api-reference/audio/create-speech`，2026-08 核对）：

| 参数 | 默认 | 取值 |
|------|------|------|
| `speed` | `1.0` | `[0.25, 4.0]` |
| `gain`（dB） | `0.0` | `[-10, 10]` |
| `response_format` | `mp3` | `mp3` / `opus` / `wav` / `pcm` |
| `sample_rate` | 随格式 | **mp3：仅 32000 / 44100（默认 44100）**；wav、pcm：8000 / 16000 / 24000 / 32000 / 44100（默认 44100）；opus：仅 48000 |

> ⚠️ `sample_rate` 与 `response_format` **强耦合**：给 mp3 填 48000（opus 的值）会被服务端拒绝。设置界面的 `sample_rate` 预设 chip 默认填 `44100`，与默认的 mp3 匹配。
>
> `enable_thinking` / `temperature` 这类 chat completions 参数在 audio/speech 上没有定义，不要放进 TTS 的 extra。

### chat+audio 协议（小米 MiMo）

**非流式**（`extra.stream=false`）：

```json
{
  "model": "mimo-v2.5-tts",
  "messages": [
    { "role": "user", "content": "用自然、平稳、清晰的语气朗读。" },
    { "role": "assistant", "content": "(语速偏慢)要朗读的文本" }
  ],
  "audio": { "format": "wav", "voice": "Milo" }
}
```

> `assistant` 内容开头的 `(语速偏慢)` 来自 `extra.prefix`，不配就没有这段前缀、`content` 即原文。

响应（音频在 `choices[0].message.audio.data`，base64）：

```json
{ "choices": [ { "message": { "audio": { "data": "<base64 音频>" } } } ] }
```

**流式**（默认，`stream:true` + `pcm16`）：

```json
{
  "model": "mimo-v2.5-tts",
  "messages": [ /* 同上 */ ],
  "audio": { "format": "pcm16", "voice": "Milo" },
  "stream": true
}
```

SSE 响应逐块（PCM16LE / 24kHz / 单声道，需自行拼接）：

```
data: {"choices":[{"delta":{"audio":{"data":"<base64 PCM 分块>"}}}]}
data: [DONE]
```

**内置音色**（`mimo-v2.5-tts`，填裸名字。据小米官方文档「预置音色列表」，2026-08 核对）：`mimo_default`（随部署集群，中国集群=`冰糖`/其它集群=`Mia`）、`冰糖`/`茉莉`（中文女）、`苏打`/`白桦`（中文男）、`Mia`/`Chloe`（英文女）、`Milo`/`Dean`（英文男）。默认 `mimo_default`（本工具主要朗读中译文，写死英文音色读中文效果很差）。

`audio.format` 官方仅两个取值：`wav`（非流式）和 `pcm16`（流式必须用它才能拼接）；**没有 mp3**。预置音色仅 `mimo-v2.5-tts` 模型支持（`-voicedesign` / `-voiceclone` 不支持）。

**据小米官方文档 2026-08 核对（`mimo.mi.com/docs/zh-CN/quick-start/usage-guide/audio/speech-synthesis-v2.5`），以下与本实现相关的约定需注意：**

- **只有 `mimo-v2.5-tts` 是真流式**。`-voicedesign` / `-voiceclone` 的流式「目前降级为兼容模式，仅在所有推理完成后以流式格式返回一次结果」——配这两个模型时边收边播没有意义（分块会在最后一次性涌来），建议在其 provider 的 `extra` 里填 `{"stream": false}`
- **`-voicedesign` 的 `user` 消息是必填的**（内容即音色描述）。本实现在 `extra.style` 为空串时不发送 `user` 消息，配这个模型会失败
- **两种风格控制的位置不同，不能混用**：自然语言描述放 `user`（本实现 `extra.style`）；`(东北话)`、`(唱歌)`、`(语速加快)` 这类**风格标签必须放在 `assistant` 目标文本的开头**（本实现 `extra.prefix`），`[吸气]`/`[笑]` 这类音频标签可插在文本任意位置（跟在 `prefix` 后面亦可，整串原样透传）。把标签填进 `style`、或把自然语言描述填进 `prefix`，都不生效
- **API 没有 `speed` 参数**：`audio` 对象官方只有 `format` / `voice`（voicedesign 另有 `optimize_text_preview`）。调语速只能靠 `prefix`（`(语速偏慢)` / `(语速加快)`）或 `style`（"语速缓慢而沉稳"），是模型软遵循，不是精确倍速。extra 里填 `speed` 会被本实现直接忽略（那是 audio/speech 的参数）
- 官方流式分块节奏：首块 7680 字节（0.16s 音频），之后每块 15360 字节（0.32s 音频），24kHz/PCM16LE/单声道
- 认证头官方主推 `api-key: $MIMO_API_KEY`，同时也支持 `Authorization: Bearer`（本实现用后者）

通过 `extra` 自定义（仅这五个键生效）：

```json
{ "voice": "Milo", "format": "wav", "style": "低沉沙哑，像历经沧桑的老前辈娓娓道来。", "prefix": "(语速偏慢)", "stream": true }
```

## 小米 MiMo TTS 配置指引

在设置里为 TTS 增加一个 provider（或改默认服务的 base_url）：

- **base_url**：`https://api.xiaomimimo.com/v1/chat/completions#`
  - 结尾的 `#` 是 **raw 标记**，强制原样请求该 chat 端点（否则会被自适应规则错误拼成 `.../v1/audio/speech`）
- **model**：`mimo-v2.5-tts`
- **api_key**：小米控制台生成的 Key
- **自定义参数写在这个 provider 自己的 `extra` 里**（设置界面选中该提供商时编辑的就是它），例如：

  ```json
  { "voice": "冰糖", "style": "用自然、平稳、清晰的语气朗读。", "prefix": "(语速偏慢)" }
  ```

  provider 的 `extra` 非空即**整体覆盖**服务级共享 extra，因此硅基流动那套 `voice: "FunAudioLLM/...:alex"` / `speed` / `response_format` / `sample_rate` 不会漏进来。留空则继承共享 extra——此时共享 extra 里的 model-scoped `voice` 含 `/` 会被忽略并回退默认音色 `mimo_default`（日志有 warn）
- **流式默认开启**：`extra` 不填 `stream` 即走 SSE 流式（`pcm16`）；如需关掉填 `{"stream": false}`。「边收边播」还受前端 `speech.stream_playback` 开关控制

> ⚠️ 服务级 `extra` 是「默认 provider + 所有未单独填写 extra 的 provider」共用的。同时在用两种协议时，**给非默认协议的 provider 单独填 extra**，不要去改共享的那份（改了会连带影响默认的硅基流动 provider）。

### 调语速

MiMo 的 chat+audio **没有 `speed` 参数**（`audio` 对象官方只有 `format`/`voice`），语速只能通过风格控制，两条通道任选或叠加：

| 方式 | 写法 | 落在哪 |
|------|------|--------|
| `prefix`（推荐，最直接） | `{"prefix": "(语速偏慢)"}` | `assistant` 文本开头的风格标签 |
| `style`（自然语言） | `{"style": "用平稳清晰的语气朗读，语速缓慢而沉稳。"}` | `user` 消息 |

- 常用值：`(语速偏慢)` / `(语速加快)`；可与其它风格写在同一对括号里用空格分隔，如 `(语速偏慢 温柔)`
- **裸文本会自动补括号**：填 `语速偏慢` 等价于 `(语速偏慢)`。这层兜底是必要的——不补括号时模型会把这四个字当正文念出来
- 已带 `(`/`（`/`[` 的原样透传，后面还能接行内音频标签，如 `(平静)[吸气]`
- 这是模型的**软遵循**，不是 audio/speech 那种精确倍速；要精确倍速需在前端播放层做（`playbackRate`），目前未实现
- 改完保存即生效：保存设置会清空 TTS 缓存，同一段文本会按新参数重新合成（`extra` 本身也在缓存键里）

## 音频格式

合成结果的格式**不定**（audio/speech 为 mp3，chat+audio 非流式默认 wav，流式为拼接后的 wav），
由 rodio 的 `Decoder` 按内容嗅探并解码，见 [audio.md](audio.md#音频格式支持)。

⚠️ **`audio/speech` 的 `response_format` 只应填 `mp3` 或 `wav`**：本地解码器没有 opus 解码器，
裸 pcm 也没有容器，填了这两个会「合成成功但播不出来」（错误信息会明确指出）。

## 依赖关系

- **依赖**：`config::merge_extra`（仅 audio/speech 路径）、`api_client::build_endpoint_url`、`reqwest::Client`（含 `stream` feature）、`futures_util::StreamExt`、`serde`/`serde_json`、`base64`、`log`
- **被依赖**：`commands::tts::speak_text`（合成）、`audio::Playback`（播放）
- **与其他服务的区别**：Translation/OCR 固定走 Chat Completions 并解析文本；TTS 按端点在「二进制 audio/speech」与「chat+audio（流式 SSE / 非流式 JSON）」间自适应，都不复用 `send_chat_completion`

## 修改指南

- **新增 TTS 协议**时，在 `synthesize()` 的分发处增加判定分支，并新增独立的 `synthesize_xxx()` 函数；避免把不同协议的字段混进同一请求体
- chat+audio 路径**故意不 `merge_extra`**：其请求体是 chat 结构，audio/speech 的 `response_format`/`speed` 等会报错或被误解；如需暴露更多 chat 参数，在 `parse_chat_audio_options` 里显式解析特定键
- `messages` 结构（`user`=风格指令 / `assistant`=`prefix`+文本）照搬小米官方示例；若实测需调整（如单条 user、或加 `modalities`），改 `build_chat_audio_messages` 即可
- **`prefix` 只能拼在 `assistant` 文本最前面**：官方规定风格标签在别处不生效，别为了"看起来整齐"挪进 `user` 或包进 `style`
- 默认音色/格式/风格由 `DEFAULT_CHAT_AUDIO_VOICE`(`mimo_default`)/`DEFAULT_CHAT_AUDIO_FORMAT`(`wav`)/`DEFAULT_CHAT_AUDIO_STYLE` 常量控制；标签括号集合由 `STYLE_TAG_OPENERS`(`(`/`（`/`[`)；流式采样率/声道由 `CHAT_AUDIO_STREAM_SAMPLE_RATE`(24000)/`CHAT_AUDIO_STREAM_CHANNELS`(1)
- audio/speech 的默认音色仍是 `{model}:alex`、`response_format` 默认 `mp3`，均可经 `extra` 覆盖
- **流式格式固定 `pcm16`**：小米流式只有 PCM 裸块能拼接，`wrap_pcm16_wav` 依赖 24kHz/单声道假设；若上游改采样率需同步 `CHAT_AUDIO_STREAM_SAMPLE_RATE`、`CHAT_AUDIO_STREAM_CHANNELS`（命令层用它们调 `audio.begin()`）
- 新增会影响音频输出的默认字段时，须同步纳入缓存键（`commands/tts.rs`）
- ⚠️ **`StreamAccum.chunk_count` 是「已送去播放的块数」，不是「解析出多少帧」**：没有 sink（关掉边收边播）时它必须保持 0，否则命令层会以为「已经播过了」而什么都不播——听感是彻底静音。`chunk_count_stays_zero_without_a_sink` 锁死这条
- 改回退阈值 `MIN_PLAYED_SECS_TO_KEEP` 时想清楚两头：调大 → 断流后更爱重来（多花钱、用户多等）；调小 → 更容易出现「只响了一下就报错」
- 日志前缀：`[TTS]`（播放环节是 `[Audio]`）；日志中会带上 `协议=audio/speech|chat+audio(流式/非流式)`、`voice`、`format`、分块数、首块/总耗时，便于排查
- **改 SSE 解析必须跑 `cargo test --lib tts`**：`tts::tests` 里既有 `handle_line` 的单元用例，也有起本地假 HTTP 服务端的端到端用例（`spawn_once`，靠 `[dev-dependencies]` 的 `tokio` net feature），覆盖末行无换行、格式化 JSON 兜底、断流后回退与否这几条只在字节流层面才暴露的路径
- **改 `wrap_pcm16_wav` 必须跑 `wrapped_wav_is_decodable_by_the_player`**：缓存里存的就是这份 WAV，头写错了线上表现是「重播无声」

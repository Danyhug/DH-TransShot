# TTS 语音合成（tts/）

## 概述

将文本转为语音，返回 **base64 编码的完整音频**，前端通过 `Audio` 元素播放；chat+audio 流式路径额外通过 **IPC Channel** 把 PCM 分块以二进制实时推给前端边收边播。

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
| `chunk_count` | 已推送给前端的流式分块数；`0` 表示未走流式分块（命中缓存/非 chat+audio/服务端未流式） |
| `sample_rate` | 流式分块采样率（Hz），非流式为 0 |

**`ChunkSink<'a>` / `on_chunk`** — 流式分块回调 `Fn(&[u8])`，参数是**已解码的 PCM16LE 裸字节**（不是 base64）；命令层把它原样写进 IPC Channel 的二进制消息。`None` 时仍会累积拼成完整音频，只是不实时回调。

> `SpeechResult` 的 `Debug` 是**手写**的，只打 `audio_base64_len` 而不是内容——derive 出来的实现会把几 MB base64 整段塞进日志/panic 信息。

**`StreamAccum<'a>` — 流式解析的累积状态**（`on_chunk` / `pcm` / `chunk_count` / `first_chunk_ms` / `started`）

由 `synthesize_chat_audio` 持有、以 `&mut` 传进 `synthesize_chat_audio_stream`，而不是后者内部自造。这样流中途失败时，调用方仍能读到 `chunk_count`（已推给前端多少块），据此决定**能否回退非流式**。核心方法 `handle_line(&str) -> Result<bool>`：解析一条 SSE 行、必要时推块，返回值表示「这行是不是 `data:` 行」（`saw_sse` 的判据）。

**`synthesize_audio_speech(...)` — 标准协议**

1. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
2. `merge_extra()` **整包合并** `extra`（可覆盖 `voice`、`speed`、`gain`、`response_format` 等）
3. `POST` → 检查状态码 → 读取二进制 → base64 编码，包成 `SpeechResult::whole`（`chunk_count=0`）

**`synthesize_chat_audio(...)` — 小米 chat+audio 协议入口**

1. `parse_chat_audio_options(extra)` 解析 `extra`（JSON），**只解释四个键**（不整包合并，避免 audio/speech 字段污染 chat 请求体）：
   - `voice`：音色裸名字，默认 `mimo_default`（跟随集群，中国区=冰糖中文女声）；**忽略含 `/` 的 model-scoped 音色**（如 `FunAudioLLM/...:alex`，那是另一协议的值），忽略时打 `warn` 日志提示改用裸音色名
   - `format`：**非流式**音频格式，默认 `wav`（仅认 `format` 键，不复用 `response_format`）
   - `style`：风格指令（`user` 消息），默认中性提示；置为空串则不发送 `user` 消息
   - `stream`：是否走 SSE 流式，默认 `true`
2. `stream=true` → 先试 `synthesize_chat_audio_stream`，失败降级 `synthesize_chat_audio_once`；`stream=false` → 直接非流式
   - **但已经推给前端分块之后不再降级**（`accum.chunk_count > 0` 时直接返回错误，错误里带「已播放 N 个分块」）：那一半音频已经在用户扬声器上响过了，再合成一遍是多花一次钱、多等几秒，听感还是「读到一半跳回开头重读」。此时由前端 `player.stop()` 收场

**`synthesize_chat_audio_once(...)` — chat+audio 非流式**

1. 组织 `messages`（`build_chat_audio_messages`）：`user`=风格指令，`assistant`=要朗读的文本
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
- `extract_stream_audio_data(value)` — 从一条 SSE 分块里取音频 base64（优先 `delta`，兼容 `message`）

### commands/tts.rs

两个命令共用 `synthesize_inner`（规范化文本 → 解析配置 → 查缓存 → 合成 → 写缓存），区别只在是否传 `on_chunk`：

**`synthesize_speech(state, text) -> Result<String, String>`** — 非流式回调，返回完整音频 base64（前端 `synthesizeSpeech`）。

**`synthesize_speech_stream(state, text, on_chunk) -> Result<SpeechResponse, String>`** — 边收边播版本（前端 `synthesizeSpeechStream`）：
- `on_chunk: Channel<InvokeResponseBody>` 是前端传入的 **IPC Channel**（不是全局事件）：
  - 合成前先发一条 JSON 控制消息 `{"event":"start","sampleRate":24000,"channels":1}`
  - 每个分块以 `InvokeResponseBody::Raw(pcm)` 二进制发送（前端收到 `ArrayBuffer`）
  - 合成结束后发 `{"event":"end","chunkCount":N}`
- 通道消息由 Tauri 保证按发送顺序投递，`end` 一定排在所有分块之后，前端无需比对分块计数
- 返回 `SpeechResponse{ audio, chunkCount, sampleRate }`（camelCase）：`chunkCount == 0` 时 `audio` 是完整音频、前端直接播；**`chunkCount > 0` 时 `audio` 被清空**（音频已逐块送达，再回传一份完整 WAV 会让长文本白白多传数 MB），前端重播时靠后端缓存

> **为什么用 Channel 而不是 `app.emit`**：`app.emit` 会把负载 JSON 拼进 `eval` 脚本字符串，广播给**每一个** webview（本项目有 main/screenshot/debug/settings 四个），且必须在主线程逐条执行。长文本几百个 base64 分块会把主线程堵死，表现为「必须等整段流传完才开始播、长文本干脆播不出来」。Channel 只投递给发起调用的 webview，**大于 1KB 的二进制其数据体走 IPC 自定义协议（fetch）**而不是被塞进 eval 字符串。
>
> 准确地说（Tauri 2.11.5 `ipc/channel.rs`）：Channel 的每条消息仍会 `webview.eval` 一小段触发代码，区别在负载本身——`Raw` 小于 `MAX_RAW_DIRECT_EXECUTE_THRESHOLD`(1KB) 时内联成 `new Uint8Array([...]).buffer`，更大则先存进 `ChannelDataIpcQueue`、由前端 `fetch` 取回。两条路径前端拿到的都是 `ArrayBuffer`。只有自定义协议 IPC 整体失败回退 `postMessage` 时形态才会变（前端 `asArrayBuffer()` 对此做了兼容）。

**`synthesize_inner` 流程：**
1. 从 `AppState.settings` 读取当前生效 TTS 配置（`tts.resolved(...)` 按 `active` 选默认或某个 provider，**四元组里已包含解析后的 `extra`**：provider 自带 extra 优先，留空才用服务级共享 extra）
2. 使用 `base_url + model + extra + text` 生成缓存键（文本先规范化：`trim()` + `CRLF -> LF`）
3. 命中 `AppState.tts_cache` 直接返回缓存 base64（`chunk_count=0`，不推流式分块）；未命中调用 `tts::synthesize()`，成功后写入缓存

缓存为进程内内存缓存（`config::settings::TtsCache`），**条数与总字节数双上限**：最多 64 条、总计 64MB（`TTS_CACHE_MAX_ENTRIES` / `TTS_CACHE_MAX_BYTES`），按插入顺序淘汰，覆盖同键时正确回退字节计数；单条自身就超预算时保留它不自我淘汰。只按条数限制是不够的——一分钟语音拼出来的 WAV base64 就有 ~3.8MB，64 条塞满能占几百 MB 常驻内存。

保存设置时清空缓存。缓存键与协议无关（不同协议因 base_url 不同天然不撞键）。缓存存的始终是「完整音频 base64」，命中时不再推流式分块，前端整段播放。

## 前端通道与播放

- 分块走 IPC Channel：控制消息为 JSON（`{event:"start"|"end", ...}`），音频分块为二进制 `ArrayBuffer`（PCM16LE 单声道）
- 前端 `lib/tts.ts` 的 `StreamingPcmPlayer` 逐块解码 PCM16→Float32，调度进共享 `AudioContext` 无缝排布；首块预留 0.2s 缓冲吸收网络抖动，且不早于输出设备预热完成（`AudioContext` 冷启动期间排上时间线的音频会被设备吞掉），收到 `end` 且全部播完时结束
- **哑火兜底**（两条判据）：① 后端 `chunkCount > 0` 而前端 `scheduledChunks === 0`（通道分块全被丢弃，必然静音，这条最确定）；② `playedThrough` 启发式判定分块排进时间线却没真正出声（WebKit 闲置 `AudioContext` 的已知表现）。兜底优先用**前端本地留存的 PCM** 拼出完整 WAV 整段播，省掉一次几 MB 的 IPC 往返；本地拿不到（超 16MB 上限已放弃留存）才再调 `synthesize_speech` 走后端缓存，所以**后端缓存在流式路径下也要写入**（`synthesize_inner` 目前如此）
- **前端 WAV 拼装与后端 `wrap_pcm16_wav` 必须字节一致**：前端流式播完会本地拼 WAV 回填自己的缓存，和后端那份进同一套缓存语义。后端 `wav_bytes_match_frontend_implementation` 测试用固定向量锁死这个契约，改任一侧的 WAV 头都会先让它红
- 详见 [docs/frontend/lib.md](../frontend/lib.md)

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
    { "role": "assistant", "content": "要朗读的文本" }
  ],
  "audio": { "format": "wav", "voice": "Milo" }
}
```

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
- **两种风格控制的位置不同，不能混用**：自然语言描述放 `user`（本实现 `extra.style` 走的就是这条，正确）；而 `(东北话)`、`(唱歌)`、`(开心 变快)` 这类**风格标签必须放在 `assistant` 目标文本的开头**，`[吸气]`/`[笑]` 这类音频标签可插在文本任意位置——把它们填进 `extra.style` 不生效
- 官方流式分块节奏：首块 7680 字节（0.16s 音频），之后每块 15360 字节（0.32s 音频），24kHz/PCM16LE/单声道
- 认证头官方主推 `api-key: $MIMO_API_KEY`，同时也支持 `Authorization: Bearer`（本实现用后者）

通过 `extra` 自定义（仅这四个键生效）：

```json
{ "voice": "Milo", "format": "wav", "style": "低沉沙哑，像历经沧桑的老前辈娓娓道来。", "stream": true }
```

## 小米 MiMo TTS 配置指引

在设置里为 TTS 增加一个 provider（或改默认服务的 base_url）：

- **base_url**：`https://api.xiaomimimo.com/v1/chat/completions#`
  - 结尾的 `#` 是 **raw 标记**，强制原样请求该 chat 端点（否则会被自适应规则错误拼成 `.../v1/audio/speech`）
- **model**：`mimo-v2.5-tts`
- **api_key**：小米控制台生成的 Key
- **自定义参数写在这个 provider 自己的 `extra` 里**（设置界面选中该提供商时编辑的就是它），例如：

  ```json
  { "voice": "冰糖", "style": "用自然、平稳、清晰的语气朗读。" }
  ```

  provider 的 `extra` 非空即**整体覆盖**服务级共享 extra，因此硅基流动那套 `voice: "FunAudioLLM/...:alex"` / `speed` / `response_format` / `sample_rate` 不会漏进来。留空则继承共享 extra——此时共享 extra 里的 model-scoped `voice` 含 `/` 会被忽略并回退默认音色 `mimo_default`（日志有 warn）
- **流式默认开启**：`extra` 不填 `stream` 即走 SSE 流式（`pcm16`）；如需关掉填 `{"stream": false}`。「边收边播」还受前端 `speech.stream_playback` 开关控制

> ⚠️ 服务级 `extra` 是「默认 provider + 所有未单独填写 extra 的 provider」共用的。同时在用两种协议时，**给非默认协议的 provider 单独填 extra**，不要去改共享的那份（改了会连带影响默认的硅基流动 provider）。

## 前端播放（音频格式自适应）

后端返回的 base64 音频**格式不定**（audio/speech 为 mp3，chat+audio 非流式默认 wav，流式为拼接后的 wav）。`lib/tts.ts` 的 `detectAudioMime()` 按音频魔数（`RIFF`→wav、`ID3`/帧同步→mp3、`OggS`→ogg、`fLaC`→flac）嗅探 MIME，再拼 `data:{mime};base64,...` 播放整段，**不写死 mp3**，保证 wav 也能播放。流式分块则走 `StreamingPcmPlayer`（Web Audio）边收边播，见 [docs/frontend/lib.md](../frontend/lib.md)。

## 依赖关系

- **依赖**：`config::merge_extra`（仅 audio/speech 路径）、`api_client::build_endpoint_url`、`reqwest::Client`（含 `stream` feature）、`futures_util::StreamExt`、`serde`/`serde_json`、`base64`、`log`
- **被依赖**：`commands::tts::synthesize_speech`、`commands::tts::synthesize_speech_stream`
- **与其他服务的区别**：Translation/OCR 固定走 Chat Completions 并解析文本；TTS 按端点在「二进制 audio/speech」与「chat+audio（流式 SSE / 非流式 JSON）」间自适应，都不复用 `send_chat_completion`

## 修改指南

- **新增 TTS 协议**时，在 `synthesize()` 的分发处增加判定分支，并新增独立的 `synthesize_xxx()` 函数；避免把不同协议的字段混进同一请求体
- chat+audio 路径**故意不 `merge_extra`**：其请求体是 chat 结构，audio/speech 的 `response_format`/`speed` 等会报错或被误解；如需暴露更多 chat 参数，在 `parse_chat_audio_options` 里显式解析特定键
- `messages` 结构（`user`=风格指令 / `assistant`=文本）照搬小米官方示例；若实测需调整（如单条 user、或加 `modalities`），改 `build_chat_audio_messages` 即可
- 默认音色/格式/风格由 `DEFAULT_CHAT_AUDIO_VOICE`(`mimo_default`)/`DEFAULT_CHAT_AUDIO_FORMAT`(`wav`)/`DEFAULT_CHAT_AUDIO_STYLE` 常量控制；流式采样率/声道由 `CHAT_AUDIO_STREAM_SAMPLE_RATE`(24000)/`CHAT_AUDIO_STREAM_CHANNELS`(1)
- audio/speech 的默认音色仍是 `{model}:alex`、`response_format` 默认 `mp3`，均可经 `extra` 覆盖
- **流式格式固定 `pcm16`**：小米流式只有 PCM 裸块能拼接，`wrap_pcm16_wav` 依赖 24kHz/单声道假设；若上游改采样率需同步 `CHAT_AUDIO_STREAM_SAMPLE_RATE` 和事件里的 `sampleRate`
- 新增会影响音频输出的默认字段时，须同步纳入缓存键（`commands/tts.rs`）
- 前端整段播放依赖 `detectAudioMime`，新增音频格式时补充对应魔数
- 日志前缀：`[TTS]`；日志中会带上 `协议=audio/speech|chat+audio(流式/非流式)`、`voice`、`format`、分块数、首块/总耗时，便于排查
- **改 SSE 解析必须跑 `cargo test --lib tts`**：`tts::tests` 里既有 `handle_line` 的单元用例，也有起本地假 HTTP 服务端的端到端用例（`spawn_once`，靠 `[dev-dependencies]` 的 `tokio` net feature），覆盖末行无换行、格式化 JSON 兜底、已推块不回退这几条只在字节流层面才暴露的路径

# 窗口销毁（window_lifecycle.rs）

## 概述

统一封装「销毁一个还带着 webview 的窗口」这件事。**所有销毁窗口的路径都必须走本模块，不要直接调 `WebviewWindow::close()`。**

## 为什么需要它

macOS 上直接销毁还活着的 webview，WebKit 的 display link 可能在下一次刷新时访问已经释放的滚动树，把整个 App 打崩。

真实崩溃现场（v1.1.3，用户日志与崩溃报告时间戳对得上）：

1. `⌥A` 区域截图 → 覆盖层窗口创建 → 截图、标注、复制到剪贴板
2. 覆盖层 emit `close-all-overlays` → 旧代码在同一个调用里对覆盖层窗口调 `close()`
3. 约 1.1 秒后主线程 SIGSEGV

崩溃栈上**没有任何本项目的帧**：

```
WebCore::ScrollingTree::takePendingScrollUpdates()                       <-- 出错点
WebKit::RemoteScrollingCoordinatorProxy::sendScrollingTreeNodeUpdate()
WebKit::RemoteLayerTreeDrawingAreaProxy::didRefreshDisplay(ProcessState&, IPC::Connection&)
WebKit::RemoteLayerTreeDrawingAreaProxy::forEachProcessState(...)
WebKit::RemoteLayerTreeDrawingAreaProxyMac::didRefreshDisplay()
WebKit::RemoteLayerTreeDisplayLinkClient::displayLinkFired(...)
WTF::RunLoop::performWork()
```

`Exception Type: EXC_BAD_ACCESS (SIGSEGV)`、`KERN_INVALID_ADDRESS at 0x122`：`forEachProcessState` 正在遍历该 drawing area 的进程列表，而里面那个条目的滚动树已经释放了，于是从空指针偏移 `0x122` 处读取。

命中窗口很窄——必须正好落在 display link 刷新过程中——所以现场是**偶发**的：同样的截图流程通常没事，偶尔崩一次；本次会话早些时候的同一套流程就安然无恙。

## 文件清单

| 项 | 职责 |
|------|------|
| `TEARDOWN_SETTLE_DELAY` | `hide()` 与真正 `close()` 之间的让帧时间（120ms） |
| `close_many_deferred(windows)` | 先全部隐藏 → 让帧 → 逐个销毁。空列表直接返回 |
| `close_deferred(window)` | 单窗口版本 |
| `close_overlays_deferred(app, keep_label)` | 按 `screenshot::OVERLAY_LABEL_PREFIX` 筛出覆盖层窗口再走上面的流程 |
| `close_window_deferred(app, label)` | `#[tauri::command]`，前端 `closeWindowDeferred()` 的落点 |

## 核心逻辑

规避方式是**不要在同一帧里销毁**：

```
hide()  →  等待 TEARDOWN_SETTLE_DELAY  →  close()
```

`hide()` 让窗口退出刷新循环（macOS 不再为不可见的窗口驱动 display link），等一两帧确保这一点已经生效，再释放 webview——这样 display link 就不会再去碰一块正在消失的滚动树。

`TEARDOWN_SETTLE_DELAY` 取 120ms：60Hz 下两帧约 33ms，留足余量。**调成 0 就等于退回原来的崩溃路径**，不要为了「感觉更跟手」把它改小。

`close_many_deferred` 是先对所有窗口 `hide()`、再统一等待、最后统一 `close()`，而不是逐个「隐藏-等待-关闭」，这样多窗口（多显示器覆盖层、settings + debug-log）只花一次等待时间。

## 依赖关系

- **被依赖**：
  - `commands/screenshot.rs` — `start_region_select` 关闭旧覆盖层、关闭 settings / debug-log、覆盖层 label 冲突时重试；`close_all_overlays` / `close_other_overlays`（`close-all-overlays` / `close-other-overlays` 事件的处理器）
  - `lib.rs` — 注册 `close_window_deferred` 命令
  - 前端 `lib/invoke.ts` 的 `closeWindowDeferred()` — `ScreenshotOverlay` 获取冻结截图失败、`SettingsPanel` 保存/取消、`LogPanel` 关闭按钮
- **依赖**：`screenshot::OVERLAY_LABEL_PREFIX`（覆盖层 label 前缀）、`tauri`（`Manager`、`AppHandle`、`WebviewWindow`）、`tokio::time`、`log`

## 修改指南

- 新增任何「关闭窗口」的路径时，一律走本模块；前端不要用 `getCurrentWindow().close()`
- 关闭之后要立刻复用同一个 label 建窗口（如覆盖层 label 冲突重试）时，等 `TEARDOWN_SETTLE_DELAY` 过去再建，否则会撞名建不出来
- 从事件回调或 `RunEvent` 里调用时用会 spawn 的包装（`close_all_overlays` / `close_other_overlays`），不要在回调线程上阻塞等待
- 判断「这是不是覆盖层窗口」请用 `OVERLAY_LABEL_PREFIX`，不要散落字面量 `"screenshot-overlay"`
- 这是 WebKit 层面的竞态，只能缩小窗口、不能证明消除；同类报告见 [tauri-apps/wry](https://github.com/tauri-apps/wry/issues) 的 macOS 崩溃 issue

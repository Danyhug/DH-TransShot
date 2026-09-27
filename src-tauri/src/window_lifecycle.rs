//! 销毁 webview 窗口时的「让帧」封装。
//!
//! macOS 上直接把还活着的 webview 从窗口树上摘掉，WebKit 的 display link 可能在下一次
//! 刷新时访问已经释放的滚动树：`RemoteLayerTreeDrawingAreaProxy::didRefreshDisplay()`
//! 会遍历该 drawing area 的进程列表，而列表里还留着刚被拆掉的 webview，于是
//! `WebCore::ScrollingTree::takePendingScrollUpdates()` 在空指针偏移 `0x122` 处读取，
//! 整个 App 以 `EXC_BAD_ACCESS (SIGSEGV)` 退出——崩溃栈上没有任何本项目的帧。
//!
//! 命中窗口很窄（必须正好落在 display link 刷新过程中），所以现场是偶发的：同样的
//! 截图流程通常没事，偶尔崩一次。
//!
//! 规避方式是**不要在同一帧里销毁**：先 `hide()` 让窗口退出刷新循环，等一两帧再
//! `close()`。所有销毁 webview 窗口的路径都要走这里，不要直接调 `close()`。

use std::time::Duration;

use log::{info, warn};
use tauri::{AppHandle, Manager, WebviewWindow};

/// `hide()` 与真正 `close()` 之间的让帧时间。
///
/// 60Hz 下两帧约 33ms，这里留足余量，保证 display link 至少跑过一轮并观察到窗口
/// 已经不可见。调成 0 就等于退回原来的崩溃路径。
pub const TEARDOWN_SETTLE_DELAY: Duration = Duration::from_millis(120);

/// 先全部隐藏、让帧，再逐个销毁。空列表直接返回。
pub async fn close_many_deferred(windows: Vec<WebviewWindow>) {
    if windows.is_empty() {
        return;
    }

    for window in &windows {
        info!(
            "[Window] 先隐藏窗口 {}，让出 {}ms 等 display link 停下再销毁",
            window.label(),
            TEARDOWN_SETTLE_DELAY.as_millis()
        );
        if let Err(error) = window.hide() {
            warn!("[Window] 隐藏窗口 {} 失败: {}", window.label(), error);
        }
    }

    tokio::time::sleep(TEARDOWN_SETTLE_DELAY).await;

    for window in windows {
        let label = window.label().to_string();
        match window.close() {
            Ok(()) => info!("[Window] 已销毁窗口 {}", label),
            Err(error) => warn!("[Window] 销毁窗口 {} 失败: {}", label, error),
        }
    }
}

/// 关闭单个窗口（`close_many_deferred` 的单窗口形式）。
pub async fn close_deferred(window: WebviewWindow) {
    close_many_deferred(vec![window]).await;
}

/// 关闭截图覆盖层窗口，`keep_label` 指定的那一个保留（`None` 表示全关）。
pub async fn close_overlays_deferred(app: &AppHandle, keep_label: Option<&str>) {
    let targets: Vec<WebviewWindow> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(crate::screenshot::OVERLAY_LABEL_PREFIX))
        .filter(|(label, _)| Some(label.as_str()) != keep_label)
        .map(|(_, window)| window)
        .collect();
    close_many_deferred(targets).await;
}

/// 前端请求关闭窗口时的统一入口。
///
/// 对应前端的 `closeWindowDeferred()`：不要用 `getCurrentWindow().close()`，
/// 那是在 display link 刷新过程中直接销毁 webview，正是本模块要解决的崩溃成因。
#[tauri::command]
pub async fn close_window_deferred(app: AppHandle, label: String) -> Result<(), String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("窗口不存在: {}", label))?;
    close_deferred(window).await;
    Ok(())
}

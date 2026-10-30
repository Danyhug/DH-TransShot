//! macOS 抓屏：ScreenCaptureKit 单帧截图。
//!
//! # 为什么不再只用 `CGWindowListCreateImage`
//!
//! 那条老 API 合成的是「**允许共享的窗口**」，而不是显示器的像素：
//!
//! - SDK 的 `CGWindowSharingType` 说明写得很直白：`kCGWindowSharingNone` 的窗口**不会参与合成**
//! - 系统弹窗 / 授权框 / 登录面板属于 macOS 刻意保护的 UI，基本都落在这类里
//! - 硬件解码视频、Metal / IOSurface 支撑的图层，窗口后备存储里没有那些像素，合成出来是黑的
//! - 它只看**当前空间**，别的 Space / 全屏 App 里的窗口也进不来
//!
//! 所以「弹窗截不上」「某些页面截不上」并不是覆盖层层级不够——覆盖层显示的是建窗之前就拍好的
//! 冻结图，层级再高也补不回缺失的像素；缺的是这张图本身。
//!
//! ScreenCaptureKit 走的是 WindowServer **合成后的显示帧缓冲**，拿到的是屏幕上真实可见的内容。
//! Apple 从 macOS 15 起把 `CGWindowListCreateImage` 标成 obsoleted 并要求迁到 SCK
//! （`SCREEN_CAPTURE_OBSOLETE(10.5, 14.0, 15.0)`），这也是 CleanShot / Shottr 这类工具的做法。
//!
//! 仍然拿不到的只剩 macOS 自己过滤掉的安全 UI（登录窗口、TCC 授权框、安全输入框），
//! 这是系统级限制，没有公开 API 可以绕过。

use std::ffi::{c_void, CStr};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};

/// SCK 的回调正常几十毫秒就回来；超过这个时间说明 WindowServer 那边没响应。
/// 超时即回退老路径——这个等待直接压在「按下快捷键 → 覆盖层出现」的关键路径上，不能设长。
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(
        max_displays: u32,
        displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// 按名字找 Objective-C 类。找不到说明系统版本不支持（SCK 需要 macOS 12.3，
/// 单帧截图 `SCScreenshotManager` 需要 14+），调用方据此回退到老的 CG 路径。
fn lookup_class(null_terminated_name: &'static str) -> Result<&'static AnyClass> {
    let name = CStr::from_bytes_with_nul(null_terminated_name.as_bytes())
        .map_err(|_| anyhow!("Objective-C 类名格式错误"))?;
    AnyClass::get(name).ok_or_else(|| {
        anyhow!(
            "系统里没有 {}",
            &null_terminated_name[..null_terminated_name.len() - 1]
        )
    })
}

/// 本机是否具备 SCK 单帧截图能力（macOS 14+）。
pub fn is_available() -> bool {
    lookup_class("SCScreenshotManager\0").is_ok() && lookup_class("SCShareableContent\0").is_ok()
}

/// 屏幕录制权限：缺失时 `CGWindowListCreateImage` 会**静默**只返回桌面和自己的窗口，
/// 表现为「截出来是壁纸」。这里显式检查一次，缺失时请求授权（系统只会弹一次）。
pub fn ensure_screen_recording_permission() -> bool {
    unsafe {
        if CGPreflightScreenCaptureAccess() {
            return true;
        }
        CGRequestScreenCaptureAccess()
    }
}

/// 预热 SCK。
///
/// `SCShareableContent` 第一次查询要和 WindowServer 握手（实测 200~300ms），而这条调用正好卡在
/// 「按下快捷键 → 覆盖层出现」的关键路径上。启动后在后台先跑一次，后续查询就能走系统缓存。
///
/// 只在**已经有**屏幕录制权限时预热：没有权限就去查会当场弹系统授权框，而项目约定是
/// 首次截图时才申请，别在启动阶段打扰不用截图的用户。
pub fn warm_up() {
    if !is_available() || !unsafe { CGPreflightScreenCaptureAccess() } {
        return;
    }
    let Ok(content_class) = lookup_class("SCShareableContent\0") else {
        return;
    };

    let (finished_tx, finished_rx) = mpsc::channel::<()>();
    let completion = RcBlock::new(move |_content: *mut AnyObject, _error: *mut AnyObject| {
        let _ = finished_tx.send(());
    });
    // SAFETY: 类存在，标准类方法调用；回调只用来发个信号
    unsafe {
        let _: () =
            msg_send![content_class, getShareableContentWithCompletionHandler: &*completion];
    }
    let _ = finished_rx.recv_timeout(CAPTURE_TIMEOUT);
}

/// 用逻辑坐标反查这块显示器对应的 `CGDirectDisplayID`。
///
/// SCK 的 `SCDisplay` 没有「按顺序取第 N 块屏」的语义，和 Tauri 的显示器顺序也不保证一致，
/// 只能按 bounds 匹配。bounds 是 points，正好和传进来的逻辑矩形同一套坐标。
fn display_id_for_rect(logical_rect: (f64, f64, f64, f64)) -> Option<u32> {
    let (rect_x, rect_y, rect_width, rect_height) = logical_rect;
    // 常见最多也就三块屏；超出这个数量就回退老路径，不值得为它分配
    let mut display_ids = [0u32; 16];
    let mut display_count = 0u32;
    let error = unsafe {
        CGGetActiveDisplayList(
            display_ids.len() as u32,
            display_ids.as_mut_ptr(),
            &mut display_count,
        )
    };
    if error != 0 {
        return None;
    }

    for &display_id in &display_ids[..display_count as usize] {
        let bounds = unsafe { CGDisplayBounds(display_id) };
        if (bounds.origin.x - rect_x).abs() < 1.0
            && (bounds.origin.y - rect_y).abs() < 1.0
            && (bounds.size.width - rect_width).abs() < 1.0
            && (bounds.size.height - rect_height).abs() < 1.0
        {
            return Some(display_id);
        }
    }
    None
}

/// 截取整块显示器，返回**已 retain** 的 CGImage。
///
/// 调用方负责 `CFRelease`。图像为该显示器的原生像素尺寸，坐标原点在左上角，
/// 与 `CGWindowListCreateImage` 的结果一致，下游裁切逻辑不用改。
pub fn capture_display(logical_rect: (f64, f64, f64, f64)) -> Result<*const c_void> {
    let display_id = display_id_for_rect(logical_rect)
        .ok_or_else(|| anyhow!("找不到与逻辑矩形 {:?} 匹配的显示器", logical_rect))?;

    let (result_tx, result_rx) = mpsc::channel::<Result<usize, String>>();

    // 第一段：拿到可共享内容，再在回调里发起截图。
    // 整条链都放在回调里做，避免把 Objective-C 对象跨线程传出去（它们只在本回调内有效）。
    let content_block = RcBlock::new(move |content: *mut AnyObject, error: *mut AnyObject| {
        if content.is_null() {
            let _ = result_tx.send(Err(describe_error(error, "获取可共享内容失败")));
            return;
        }
        // SAFETY: content 非空且在本回调内有效，取到的 display 也随内容对象存活。
        unsafe {
            match find_display(content, display_id) {
                Some(display) => {
                    if let Err(message) = request_frame(display, result_tx.clone()) {
                        let _ = result_tx.send(Err(message));
                    }
                }
                None => {
                    let _ = result_tx.send(Err(format!("可共享内容里没有显示器 {}", display_id)));
                }
            }
        }
    });

    let content_class = lookup_class("SCShareableContent\0")?;
    // SAFETY: 类存在，表达式为标准的类方法调用；回调在 SCK 自己的队列上执行。
    unsafe {
        let _: () =
            msg_send![content_class, getShareableContentWithCompletionHandler: &*content_block];
    }

    match result_rx.recv_timeout(CAPTURE_TIMEOUT) {
        Ok(Ok(image_pointer)) => Ok(image_pointer as *const c_void),
        Ok(Err(message)) => Err(anyhow!(message)),
        Err(_) => Err(anyhow!(
            "ScreenCaptureKit 截图超时（{}s）",
            CAPTURE_TIMEOUT.as_secs()
        )),
    }
}

/// 在 `SCShareableContent.displays` 里按 `displayID` 找到目标显示器。
unsafe fn find_display(content: *mut AnyObject, display_id: u32) -> Option<*mut AnyObject> {
    let displays: *mut AnyObject = msg_send![content, displays];
    if displays.is_null() {
        return None;
    }
    let display_count: usize = msg_send![displays, count];
    for index in 0..display_count {
        let display: *mut AnyObject = msg_send![displays, objectAtIndex: index];
        if display.is_null() {
            continue;
        }
        let candidate_id: u32 = msg_send![display, displayID];
        if candidate_id == display_id {
            return Some(display);
        }
    }
    None
}

/// 对单块显示器发起一次 `SCScreenshotManager.captureImage`。
///
/// 结果通过 channel 异步回传：`Ok` 是已 retain 的 CGImage 指针。
unsafe fn request_frame(
    display: *mut AnyObject,
    result_tx: mpsc::Sender<Result<usize, String>>,
) -> std::result::Result<(), String> {
    let filter_class = lookup_class("SCContentFilter\0").map_err(|error| error.to_string())?;
    let array_class = lookup_class("NSArray\0").map_err(|error| error.to_string())?;
    let configuration_class =
        lookup_class("SCStreamConfiguration\0").map_err(|error| error.to_string())?;
    let manager_class = lookup_class("SCScreenshotManager\0").map_err(|error| error.to_string())?;

    // 不加排除列表：display 级别的过滤拿到的是整块屏的合成结果
    let excluded_windows: *mut AnyObject = msg_send![array_class, array];
    let filter_alloc: *mut AnyObject = msg_send![filter_class, alloc];
    let filter: *mut AnyObject =
        msg_send![filter_alloc, initWithDisplay: display, excludingWindows: excluded_windows];
    if filter.is_null() {
        return Err("创建 SCContentFilter 失败".to_string());
    }

    let configuration: *mut AnyObject = msg_send![configuration_class, new];
    if configuration.is_null() {
        return Err("创建 SCStreamConfiguration 失败".to_string());
    }
    let pixel_width: isize = msg_send![display, width];
    let pixel_height: isize = msg_send![display, height];
    let _: () = msg_send![configuration, setWidth: pixel_width];
    let _: () = msg_send![configuration, setHeight: pixel_height];
    // 与 CGWindowListCreateImage 的默认行为对齐：鼠标指针不进图。
    // 用 objc2 的 Bool 而不是 Rust bool——ObjC 的 BOOL 在 x86_64 上是 signed char、
    // 在 arm64 上是 bool，编码不一致会在 msg_send 的校验里炸掉
    let _: () = msg_send![configuration, setShowsCursor: Bool::NO];

    // 过滤器与配置要活到截图完成之后，所以带进回调里释放
    let filter_address = filter as usize;
    let configuration_address = configuration as usize;
    let frame_block = RcBlock::new(move |image: *const c_void, error: *mut AnyObject| {
        let result = if image.is_null() {
            Err(describe_error(error, "截图失败"))
        } else {
            // SCK 交回来的 CGImage 在回调返回后就会被释放，先 retain 再带出去
            // SAFETY: image 非空，是 SCK 交回来的 CGImageRef
            unsafe { CFRetain(image) };
            Ok(image as usize)
        };
        // SAFETY: 这两个对象是上面 alloc / new 出来的，引用计数归本次调用所有
        unsafe {
            CFRelease(filter_address as *const c_void);
            CFRelease(configuration_address as *const c_void);
        }
        let _ = result_tx.send(result);
    });

    let _: () = msg_send![
        manager_class,
        captureImageWithFilter: filter,
        configuration: configuration,
        completionHandler: &*frame_block
    ];
    Ok(())
}

/// 不引入 Foundation 绑定，只把 NSError 的 code / domain 打出来定位问题。
fn describe_error(error: *mut AnyObject, fallback: &str) -> String {
    if error.is_null() {
        return fallback.to_string();
    }
    // SAFETY: 调用方保证 error 是当前回调拿到的 NSError 对象或空指针。
    unsafe {
        let code: isize = msg_send![error, code];
        let domain: *mut AnyObject = msg_send![error, domain];
        format!("{fallback}（code={code}, domain={domain:p}）")
    }
}

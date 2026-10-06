//! Windows 键盘状态查询与按键注入（SendInput），供快捷键和选中文本读取共用。

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// 未分配的虚拟键码（AutoHotkey 的 MenuMaskKey 默认值），目标应用不会对它做任何响应。
const VK_MASK: VIRTUAL_KEY = 0xE8;

/// 合成 Ctrl+C 前必须松开的修饰键：残留的 Alt/Shift/Win 会把它变成别的组合
/// （例如 Chrome 里 Ctrl+Shift+C 是「检查元素」）。Ctrl 本身不影响，不在此列。
const STRAY_MODIFIERS: [VIRTUAL_KEY; 4] = [VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];

pub fn is_key_down(vk: VIRTUAL_KEY) -> bool {
    // 最高位为 1 表示按键当前处于按下状态
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// 按顺序注入一组按键事件。注入数少于请求数时返回错误，通常是目标窗口
/// 以管理员权限运行（UIPI 拦截了低权限进程的输入）。
fn send(events: &[(VIRTUAL_KEY, bool)]) -> Result<(), String> {
    let inputs: Vec<INPUT> = events.iter().map(|&(vk, up)| key_input(vk, up)).collect();
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        return Err(format!(
            "SendInput 只注入了 {}/{} 个事件（目标窗口可能以管理员权限运行）",
            sent,
            inputs.len()
        ));
    }
    Ok(())
}

/// 快捷键按下时 Alt/Win 仍被按住，而触发键已被 RegisterHotKey 吞掉，前台应用
/// 只会看到「单独按下又松开 Alt」，于是进入菜单栏模式（Chrome 会把焦点移到右上角
/// 菜单按钮）。之后模拟的 Ctrl+C 落进菜单而不是页面，选中文本读不到。
/// 趁修饰键还按着时插入一个无意义按键，打断「单独 Alt」的判定。
pub fn mask_modifier_menu() {
    if !(is_key_down(VK_MENU) || is_key_down(VK_LWIN) || is_key_down(VK_RWIN)) {
        return;
    }
    if let Err(e) = send(&[(VK_MASK, false), (VK_MASK, true)]) {
        log::warn!("[Hotkey] 屏蔽 Alt 菜单激活失败: {}", e);
    }
}

/// 等待用户松开 Alt/Shift/Win，最多约 `timeout_ms`。返回是否全部已松开。
pub fn wait_for_modifiers_release(timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        if !STRAY_MODIFIERS.iter().any(|&vk| is_key_down(vk)) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// 模拟一次干净的 Ctrl+C：先为仍按着的 Alt/Shift/Win 注入 key-up，再发送 Ctrl+C。
pub fn send_ctrl_c() -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL;
    const VK_C: VIRTUAL_KEY = b'C' as VIRTUAL_KEY;

    let mut events: Vec<(VIRTUAL_KEY, bool)> = STRAY_MODIFIERS
        .iter()
        .filter(|&&vk| is_key_down(vk))
        .map(|&vk| (vk, true))
        .collect();
    if !events.is_empty() {
        // 合成的 Alt key-up 同样可能触发菜单，前面垫一个屏蔽键
        events.insert(0, (VK_MASK, false));
        events.insert(1, (VK_MASK, true));
    }
    events.extend([
        (VK_CONTROL, false),
        (VK_C, false),
        (VK_C, true),
        (VK_CONTROL, true),
    ]);
    send(&events)
}

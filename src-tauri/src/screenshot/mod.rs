pub mod capture;

pub use capture::{
    capture_monitors, capture_region_bytes, capture_region_from_full, list_window_rects,
};

/// 截图覆盖层窗口的 label 前缀。每块显示器一个：`screenshot-overlay-0`、`screenshot-overlay-1` …
pub const OVERLAY_LABEL_PREFIX: &str = "screenshot-overlay";

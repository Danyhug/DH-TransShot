use base64::Engine;
use image::ImageFormat;
use log::info;
use std::io::Cursor;

/// A visible window rectangle in logical (points) coordinates.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// List all visible normal-layer window rectangles in front-to-back order.
/// Returns logical coordinates (points on macOS).
/// On non-macOS platforms, returns an empty Vec.
pub fn list_window_rects() -> Vec<WindowRect> {
    #[cfg(target_os = "macos")]
    {
        list_window_rects_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn list_window_rects_macos() -> Vec<WindowRect> {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: u32) -> *const c_void;
        fn CGRectMakeWithDictionaryRepresentation(
            dict: *const c_void,
            rect: *mut CGRectRaw,
        ) -> bool;

        // CFString constants exported from CoreGraphics
        static kCGWindowLayer: *const c_void;
        static kCGWindowBounds: *const c_void;
        static kCGWindowIsOnscreen: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFRelease(cf: *const c_void);

        // CFNumber
        fn CFNumberGetValue(
            number: *const c_void,
            the_type: isize,
            value_ptr: *mut c_void,
        ) -> bool;

        // CFBoolean
        fn CFBooleanGetValue(boolean: *const c_void) -> bool;
    }

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct CGRectRaw {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    // kCFNumberSInt32Type = 3
    const K_CF_NUMBER_SINT32_TYPE: isize = 3;

    unsafe {
        // kCGWindowListOptionOnScreenOnly(1) | kCGWindowListExcludeDesktopElements(16) = 17
        let window_list = CGWindowListCopyWindowInfo(17, 0);
        if window_list.is_null() {
            info!("[Capture] CGWindowListCopyWindowInfo returned null");
            return Vec::new();
        }

        let count = CFArrayGetCount(window_list);
        let mut rects = Vec::new();

        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(window_list, i);
            if dict.is_null() {
                continue;
            }

            // Filter: only normal layer (layer == 0)
            let layer_val = CFDictionaryGetValue(dict, kCGWindowLayer);
            if layer_val.is_null() {
                continue;
            }
            let mut layer: i32 = -1;
            if !CFNumberGetValue(
                layer_val,
                K_CF_NUMBER_SINT32_TYPE,
                &mut layer as *mut i32 as *mut c_void,
            ) {
                continue;
            }
            if layer != 0 {
                continue;
            }

            // Filter: must be on screen
            let on_screen_val = CFDictionaryGetValue(dict, kCGWindowIsOnscreen);
            if !on_screen_val.is_null() {
                if !CFBooleanGetValue(on_screen_val) {
                    continue;
                }
            }

            // Get bounds dictionary
            let bounds_val = CFDictionaryGetValue(dict, kCGWindowBounds);
            if bounds_val.is_null() {
                continue;
            }

            let mut cg_rect = CGRectRaw::default();
            if !CGRectMakeWithDictionaryRepresentation(bounds_val, &mut cg_rect) {
                continue;
            }

            // Skip tiny windows (< 10x10)
            if cg_rect.width < 10.0 || cg_rect.height < 10.0 {
                continue;
            }

            rects.push(WindowRect {
                x: cg_rect.x,
                y: cg_rect.y,
                width: cg_rect.width,
                height: cg_rect.height,
            });
        }

        CFRelease(window_list);

        info!("[Capture] list_window_rects: found {} windows", rects.len());
        rects
    }
}

/// Capture the full screen of the primary monitor, return as base64 PNG.
pub fn capture_full() -> anyhow::Result<String> {
    info!("[Capture] capture_full 开始");
    #[cfg(target_os = "macos")]
    {
        return capture_full_macos();
    }

    #[cfg(not(target_os = "macos"))]
    {
        capture_full_xcap()
    }
}

/// macOS: Use Core Graphics CGWindowListCreateImage directly.
/// This avoids xcap's ObjC exception issues with IMK.
#[cfg(target_os = "macos")]
fn capture_full_macos() -> anyhow::Result<String> {
    use std::ffi::c_void;

    // Core Graphics FFI
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsWide(display: u32) -> usize;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
        fn CGWindowListCreateImage(
            rect: CGRect,
            option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *const c_void;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageGetBitsPerPixel(image: *const c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *const c_void) -> usize;
        fn CGImageGetDataProvider(image: *const c_void) -> *const c_void;
        fn CGDataProviderCopyData(provider: *const c_void) -> *const c_void;
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFRelease(cf: *const c_void);
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    unsafe {
        let display = CGMainDisplayID();
        let width = CGDisplayPixelsWide(display);
        let height = CGDisplayPixelsHigh(display);

        info!("[Capture] macOS CG 截图, display={}, 分辨率={}x{}", display, width, height);

        // CGRectInfinite captures all displays; use explicit rect for primary
        let rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };

        // kCGWindowListOptionOnScreenOnly = 1, kCGNullWindowID = 0
        // kCGWindowImageDefault = 0
        let cg_image = CGWindowListCreateImage(rect, 1, 0, 0);
        if cg_image.is_null() {
            anyhow::bail!("CGWindowListCreateImage returned null - check screen recording permission");
        }

        let img_width = CGImageGetWidth(cg_image);
        let img_height = CGImageGetHeight(cg_image);
        let bytes_per_row = CGImageGetBytesPerRow(cg_image);
        let bits_per_pixel = CGImageGetBitsPerPixel(cg_image);

        info!("[Capture] CGImage: {}x{}, bpp={}, bpr={}", img_width, img_height, bits_per_pixel, bytes_per_row);

        let provider = CGImageGetDataProvider(cg_image);
        if provider.is_null() {
            CFRelease(cg_image);
            anyhow::bail!("CGImageGetDataProvider returned null");
        }

        let cf_data = CGDataProviderCopyData(provider);
        if cf_data.is_null() {
            CFRelease(cg_image);
            anyhow::bail!("CGDataProviderCopyData returned null");
        }

        let ptr = CFDataGetBytePtr(cf_data);
        let len = CFDataGetLength(cf_data) as usize;
        let raw_bytes = std::slice::from_raw_parts(ptr, len);

        // CG returns BGRA, convert to RGBA
        let bytes_per_pixel = bits_per_pixel / 8;
        let mut rgba_buf = Vec::with_capacity(img_width * img_height * 4);

        for y in 0..img_height {
            let row_start = y * bytes_per_row;
            for x in 0..img_width {
                let offset = row_start + x * bytes_per_pixel;
                if offset + 3 < len {
                    let b = raw_bytes[offset];
                    let g = raw_bytes[offset + 1];
                    let r = raw_bytes[offset + 2];
                    let a = raw_bytes[offset + 3];
                    rgba_buf.push(r);
                    rgba_buf.push(g);
                    rgba_buf.push(b);
                    rgba_buf.push(a);
                }
            }
        }

        CFRelease(cf_data);
        CFRelease(cg_image);

        let img = image::RgbaImage::from_raw(img_width as u32, img_height as u32, rgba_buf)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image from raw data"))?;

        image_to_base64(&img)
    }
}

/// Fallback: Use xcap for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
fn capture_full_xcap() -> anyhow::Result<String> {
    use xcap::Monitor;

    let monitors = Monitor::all()?;
    let monitor = monitors
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No monitor found"))?;

    let img = monitor.capture_image()?;
    image_to_base64(&img)
}

/// Given a full-screen image as base64 PNG, crop the specified region.
pub fn capture_region_from_full(
    full_base64: &str,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> anyhow::Result<String> {
    info!("[Capture] capture_region_from_full, region=({},{},{}x{}), full base64 size={}", x, y, width, height, full_base64.len());
    let bytes = base64::engine::general_purpose::STANDARD.decode(full_base64)?;
    let img = image::load_from_memory_with_format(&bytes, ImageFormat::Png)?;
    info!("[Capture] 原图解码完成, 尺寸={}x{}", img.width(), img.height());
    let cropped = img.crop_imm(x, y, width, height);
    image_to_base64(&cropped.to_rgba8())
}

fn image_to_base64(img: &image::RgbaImage) -> anyhow::Result<String> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

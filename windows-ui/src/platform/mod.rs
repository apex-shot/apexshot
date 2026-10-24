//! Windows process preparation: DPI awareness and the bundled UI font.

use std::sync::atomic::{AtomicU32, Ordering};

/// GDI's system DPI as a scale, in thousandths. GDK reports 1 on this Windows
/// build even when the display is at 125%, so the settings surface applies the
/// system scale itself.
static UI_SCALE_MILLIS: AtomicU32 = AtomicU32::new(1000);

pub fn ui_scale() -> f64 {
    UI_SCALE_MILLIS.load(Ordering::Relaxed) as f64 / 1000.0
}

#[cfg(target_os = "windows")]
pub fn prepare_process() {
    use windows_sys::Win32::UI::HiDpi::{
        GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    // Set before GTK initializes, so GDK reports the monitor scale instead of
    // rendering the logical UI at 1x on a scaled display.
    let aware =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let dpi = unsafe { GetDpiForSystem() };
    if dpi > 0 {
        UI_SCALE_MILLIS.store(
            (dpi as f64 / 96.0 * 1000.0).round() as u32,
            Ordering::Relaxed,
        );
    }
    eprintln!(
        "[settings] dpi aware={aware} system_dpi={dpi} ui_scale={}",
        ui_scale()
    );

    register_bundled_fonts();
}

/// Make the bundled Inter faces available to both text stacks: GDI for the
/// text cairo draws on canvases, and Pango for every GTK label. Pango's
/// DirectWrite font map never sees fonts added privately through GDI, so
/// without its own registration GTK falls back to a system font.
#[cfg(target_os = "windows")]
fn register_bundled_fonts() {
    use pango::prelude::FontMapExt;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::AddFontResourceExW;

    // FR_PRIVATE: available to this process without installing it system-wide.
    const FR_PRIVATE: u32 = 0x10;

    let directory = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../capture-overlay/assets/fonts"
    );

    let fonts = pangocairo::FontMap::default();
    for file in [
        "Inter-Regular.ttf",
        "Inter-Medium.ttf",
        "Inter-SemiBold.ttf",
        "Inter-Bold.ttf",
    ] {
        let path = format!("{directory}/{file}");
        if let Err(error) = fonts.add_font_file(&path) {
            eprintln!("[settings] GTK could not load {file}: {error}");
        }
        let wide: Vec<u16> = OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            AddFontResourceExW(wide.as_ptr(), FR_PRIVATE, std::ptr::null());
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_process() {}

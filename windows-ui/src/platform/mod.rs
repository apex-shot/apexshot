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

/// Make the bundled faces available to both text stacks: GDI for the text
/// cairo draws on canvases, and Pango for every GTK label. Pango's
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

    let Some(directory) = font_directory() else {
        eprintln!("[settings] the bundled fonts are missing; GTK uses a system face");
        return;
    };

    let fonts = pangocairo::FontMap::default();
    for file in [
        "Inter-Regular.ttf",
        "Inter-Medium.ttf",
        "Inter-SemiBold.ttf",
        "Inter-Bold.ttf",
        // The Settings, History, image editor and video editor typeface.
        "DMSans-Regular.ttf",
        "DMSans-Medium.ttf",
        "DMSans-SemiBold.ttf",
    ] {
        let path = directory.join(file);
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

/// Where the bundled faces live: `share/apexshot/fonts` beside the install's
/// `bin` folder, or the checkout copy the Qt shell and the Linux build read.
///
/// The installed copy comes first because a released binary must not depend on
/// the directory it was compiled in.
#[cfg(target_os = "windows")]
fn font_directory() -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        candidates.push(directory.join("share/apexshot/fonts"));
        candidates.push(directory.join("../share/apexshot/fonts"));
    }
    if let Some(source) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(
            std::path::Path::new(source)
                .join("../capture-overlay/assets/fonts")
                .to_path_buf(),
        );
    }
    candidates.into_iter().find(|path| path.is_dir())
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_process() {}

/// The display name of the account this process runs as ("Godwin Mayodi"),
/// falling back to the environment's login name.
///
/// The settings shell shows it in the account chip. A Microsoft account has a
/// display name even when the profile directory is a truncated login, which is
/// why the extended-name lookup comes first.
pub fn account_name() -> Option<String> {
    display_name().or_else(|| {
        std::env::var("USERNAME")
            .ok()
            .or_else(|| std::env::var("USER").ok())
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    })
}

#[cfg(target_os = "windows")]
fn display_name() -> Option<String> {
    use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};

    let mut size = 0u32;
    unsafe {
        GetUserNameExW(NameDisplay, std::ptr::null_mut(), &mut size);
    }
    if size == 0 {
        return None;
    }

    let mut buffer = vec![0u16; size as usize];
    let written = unsafe { GetUserNameExW(NameDisplay, buffer.as_mut_ptr(), &mut size) };
    if !written {
        return None;
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end])
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

#[cfg(not(target_os = "windows"))]
fn display_name() -> Option<String> {
    None
}

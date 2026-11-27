//! Windows process preparation: DPI awareness and the bundled UI font.

#[cfg(target_os = "windows")]
pub mod file_drag;

use std::sync::atomic::{AtomicU32, Ordering};

/// GDI's system DPI as a scale, in thousandths. GDK reports 1 on this Windows
/// build even when the display is at 125%, so the settings surface applies the
/// system scale itself.
static UI_SCALE_MILLIS: AtomicU32 = AtomicU32::new(1000);

pub fn ui_scale() -> f64 {
    UI_SCALE_MILLIS.load(Ordering::Relaxed) as f64 / 1000.0
}

/// Open a stand-alone card in the middle of the screen the pointer is on.
///
/// GTK 4 on Windows leaves a new top-level window at the system's cascade
/// position near the top-left corner. A card that is not attached to another
/// window should open where the person is working, as it does on Linux. Call
/// this before `present()`: the move happens as the window is mapped, before
/// its first frame is drawn.
#[cfg(target_os = "windows")]
pub fn center_on_pointer_screen(window: &impl gtk4::prelude::IsA<gtk4::Window>) {
    use gtk4::prelude::*;

    // Only the first showing: a window the person has moved, then hidden and
    // shown again, stays where they put it.
    let placed = std::cell::Cell::new(false);
    window.as_ref().connect_map(move |window| {
        if placed.replace(true) {
            return;
        }
        let title = window
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default();
        center_thread_window_by_title(&title);
    });
}

#[cfg(target_os = "windows")]
fn center_thread_window_by_title(title: &str) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetCursorPos, GetWindowRect, GetWindowTextW, IsWindowVisible,
        SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    struct Search {
        title: Vec<u16>,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut Search) };
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut buffer = [0u16; 512];
        let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        if length > 0 && buffer[..length as usize] == search.title[..] {
            search.found.push(hwnd);
        }
        1
    }

    let mut search = Search {
        title: title.encode_utf16().collect(),
        found: Vec::new(),
    };
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            &mut search as *mut Search as LPARAM,
        );

        let mut pointer = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pointer);
        let monitor = MonitorFromPoint(pointer, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        let area = info.rcWork;

        for hwnd in search.found {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetWindowRect(hwnd, &mut rect) == 0 {
                continue;
            }
            let (x, y) = centered_origin(
                (area.left, area.top, area.right, area.bottom),
                (rect.right - rect.left, rect.bottom - rect.top),
            );
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// The top-left corner that centres a `size` window in `area` (left, top,
/// right, bottom), kept inside the area when the window is larger than it.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn centered_origin(area: (i32, i32, i32, i32), size: (i32, i32)) -> (i32, i32) {
    let (left, top, right, bottom) = area;
    let (width, height) = size;
    let x = left + ((right - left - width) / 2).max(0);
    let y = top + ((bottom - top - height) / 2).max(0);
    (x, y)
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

/// Draw text hinted to the pixel grid instead of GTK's automatic mode, which
/// leaves glyphs unhinted and at fractional positions and reads soft at
/// ordinary display scales. Slight hinting keeps the face's shapes, and hinted
/// metrics keep letters on whole pixels.
///
/// Call this from an application's activate handler: it needs a display, and
/// the setting is process-wide, so each window process installs it itself.
pub fn sharpen_text() {
    use gtk4::prelude::ObjectExt;
    let Some(settings) = gtk4::Settings::default() else {
        return;
    };
    // An enum this build's bindings do not name, so it is set by its nick.
    if let Some(spec) = settings.find_property("gtk-font-rendering") {
        let manual = gtk4::glib::EnumClass::with_type(spec.value_type())
            .and_then(|class| class.to_value_by_nick("manual"));
        if let Some(manual) = manual {
            settings.set_property_from_value("gtk-font-rendering", &manual);
        }
    }
    if settings.find_property("gtk-xft-hintstyle").is_some() {
        settings.set_property("gtk-xft-hintstyle", "hintslight");
    }
    if settings.find_property("gtk-xft-antialias").is_some() {
        settings.set_property("gtk-xft-antialias", 1i32);
    }
    if settings.find_property("gtk-xft-hinting").is_some() {
        settings.set_property("gtk-xft-hinting", 1i32);
    }
    if settings.find_property("gtk-hint-font-metrics").is_some() {
        settings.set_property("gtk-hint-font-metrics", true);
    }
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
        // The video editor's timeline still draws its labels through cairo.
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

#[cfg(test)]
mod tests {
    use super::centered_origin;

    #[test]
    fn a_card_is_centred_in_the_work_area() {
        assert_eq!(centered_origin((0, 0, 1920, 1146), (504, 439)), (708, 353));
    }

    #[test]
    fn a_card_on_a_second_screen_is_centred_on_that_screen() {
        assert_eq!(
            centered_origin((1920, 0, 4480, 1400), (500, 400)),
            (2950, 500)
        );
    }

    #[test]
    fn a_window_larger_than_the_screen_keeps_its_top_left_on_it() {
        assert_eq!(centered_origin((0, 0, 800, 600), (1000, 700)), (0, 0));
    }
}

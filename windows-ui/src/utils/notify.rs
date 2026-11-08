//! Windows system notifications.
//!
//! Windows has no notification service an unpackaged desktop app can call the
//! way Linux has `org.freedesktop.Notifications`: the shell presents a
//! notification through the notification-area entity that owns it, and that
//! entity is an icon the app has to keep alive while the popup shows. So the
//! transport is one icon on a hidden window of its own, on a thread of its
//! own: the icon appears with the notification and is hidden again as soon as
//! the shell reports the balloon closed, which leaves the notification area
//! looking exactly as the user left it.
//!
//! An empty body is not the same as a short notification here — the shell
//! reads it as "remove the balloon" — so a notification with only a summary is
//! shown with the app name as its title and the summary as its body.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    ExtractIconExW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_STATE, NIF_TIP,
    NIIF_INFO, NIIF_LARGE_ICON, NIIF_USER, NIIF_WARNING, NIM_ADD, NIM_MODIFY, NIN_BALLOONHIDE,
    NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NIS_HIDDEN, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DispatchMessageW, LoadIconW, PeekMessageW,
    PrivateExtractIconsW, RegisterClassW, TranslateMessage, HICON, IDI_APPLICATION, MSG, PM_REMOVE,
    WM_APP, WNDCLASSW, WS_EX_TOOLWINDOW,
};

/// The window class the notification icon belongs to.
const WINDOW_CLASS: &str = "ApexShotNotificationHost";

/// The window message the shell reports balloon events through.
const CALLBACK_MESSAGE: u32 = WM_APP + 1;

/// Identifies our icon within the window, so a late add replaces it instead of
/// leaving a second one behind.
const ICON_ID: u32 = 1;

/// How long the icon may stay up if the shell never reports the balloon
/// closed. It always does, so this only covers a lost callback.
const ICON_LIFETIME: Duration = Duration::from_secs(60);

/// How often the worker looks for balloon events and new notifications: the
/// icon hides at most this long after the popup goes away.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Set by the window procedure when the shell says a balloon is gone; the
/// worker clears it before showing the next notification.
static BALLOON_CLOSED: AtomicBool = AtomicBool::new(false);

static QUEUE: OnceLock<Sender<Notification>> = OnceLock::new();

/// Show a system notification with ApexShot branding.
pub fn desktop_notification(summary: &str, body: &str) {
    send(Notification {
        summary: summary.to_owned(),
        body: body.to_owned(),
        important: false,
    });
}

/// Important user-facing feedback (failed copies, failed exports, connection
/// changes), shown with the attention icon rather than the information one.
pub fn desktop_notification_important(summary: &str, body: &str) {
    send(Notification {
        summary: summary.to_owned(),
        body: body.to_owned(),
        important: true,
    });
}

/// The shell has no replace-by-id call, so a replaced notification is simply
/// the next one. Upload paths use this and ignore the returned id.
pub fn desktop_notification_replace(_replaces_id: u32, summary: &str, body: &str) -> u32 {
    desktop_notification(summary, body);
    0
}

/// Show one notification and wait until the shell takes it down.
///
/// A helper process that exits right after queueing a balloon tears down the
/// hidden window the shell is drawing from, so nothing appears. This keeps
/// the process alive for the balloon's lifetime instead.
pub fn desktop_notification_and_wait(summary: &str, body: &str, important: bool) {
    const WAIT_LIFETIME: Duration = Duration::from_secs(15);

    if summary.is_empty() && body.is_empty() {
        return;
    }
    let Some(mut host) = Host::create() else {
        return;
    };
    host.show(&Notification {
        summary: summary.to_owned(),
        body: body.to_owned(),
        important,
    });
    let shown = Instant::now();
    while shown.elapsed() < WAIT_LIFETIME {
        host.pump();
        if BALLOON_CLOSED.swap(false, Ordering::AcqRel) {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    host.hide();
}

struct Notification {
    summary: String,
    body: String,
    important: bool,
}

impl Notification {
    /// The title and body the shell shows.
    fn fields(&self) -> (&str, &str) {
        if self.body.is_empty() {
            (app_name(), self.summary.as_str())
        } else {
            (self.summary.as_str(), self.body.as_str())
        }
    }
}

fn app_name() -> &'static str {
    crate::app_identity::app_name()
}

fn send(notification: Notification) {
    if notification.summary.is_empty() && notification.body.is_empty() {
        return;
    }
    if let Err(error) = queue().send(notification) {
        eprintln!("[notify] the notification thread is gone: {error}");
    }
}

/// The queue the worker thread reads, started on first use.
fn queue() -> &'static Sender<Notification> {
    QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("apexshot-notifications".to_string())
            .spawn(move || run(receiver));
        if let Err(error) = spawned {
            eprintln!("[notify] could not start the notification thread: {error}");
        }
        sender
    })
}

/// The notification area for this process: the hidden window the icon belongs
/// to, and the icon itself.
struct Host {
    window: HWND,
    icon: HICON,
    balloon_icon: HICON,
    added: bool,
    shown_at: Option<Instant>,
}

impl Host {
    fn create() -> Option<Self> {
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        register_class(instance);

        let class = wide(WINDOW_CLASS);
        let title = wide(app_name());
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                // No WS_VISIBLE: the window exists only to receive the
                // shell's balloon events, never to be seen.
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if window.is_null() {
            eprintln!("[notify] could not create the notification window");
            return None;
        }

        let (icon, balloon_icon) = app_icons();
        Some(Self {
            window,
            icon,
            balloon_icon,
            added: false,
            shown_at: None,
        })
    }

    /// Deliver the messages the shell queued for the hidden window.
    fn pump(&self) {
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, self.window, 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn visible(&self) -> bool {
        self.shown_at.is_some()
    }

    fn shown_long_enough(&self) -> bool {
        self.shown_at
            .is_some_and(|shown| shown.elapsed() > ICON_LIFETIME)
    }

    /// Show the icon — a hidden icon cannot raise a balloon — with the balloon
    /// attached.
    fn show(&mut self, notification: &Notification) {
        let (title, body) = notification.fields();
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_INFO | NIF_STATE;
        data.uCallbackMessage = CALLBACK_MESSAGE;
        data.hIcon = self.icon;
        let has_balloon_icon = !notification.important && !self.balloon_icon.is_null();
        if has_balloon_icon {
            data.hBalloonIcon = self.balloon_icon;
        }
        // The previous balloon left the icon hidden; clearing the bit reveals
        // it again before the new balloon goes up.
        data.dwState = 0;
        data.dwStateMask = NIS_HIDDEN;
        write_utf16(&mut data.szTip, app_name());
        write_utf16(&mut data.szInfoTitle, title);
        write_utf16(&mut data.szInfo, body);
        data.dwInfoFlags = if notification.important {
            NIIF_WARNING
        } else if has_balloon_icon {
            NIIF_USER | NIIF_LARGE_ICON
        } else {
            NIIF_INFO
        };

        let message = if self.added { NIM_MODIFY } else { NIM_ADD };
        if unsafe { Shell_NotifyIconW(message, &data) } == 0 {
            eprintln!("[notify] the notification area refused {title}: {body}");
            return;
        }

        self.added = true;
        self.shown_at = Some(Instant::now());
    }

    /// Put the icon back out of sight, keeping the entity the next balloon
    /// needs.
    fn hide(&mut self) {
        let mut data = self.data();
        data.uFlags = NIF_STATE;
        data.dwState = NIS_HIDDEN;
        data.dwStateMask = NIS_HIDDEN;
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        self.shown_at = None;
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uID = ICON_ID;
        data
    }
}

fn run(notifications: Receiver<Notification>) {
    let Some(mut host) = Host::create() else {
        return;
    };

    loop {
        host.pump();
        if host.visible()
            && (BALLOON_CLOSED.swap(false, Ordering::AcqRel) || host.shown_long_enough())
        {
            host.hide();
        }

        match notifications.recv_timeout(POLL_INTERVAL) {
            Ok(notification) => host.show(&notification),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn register_class(instance: HINSTANCE) {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        let class = wide(WINDOW_CLASS);
        let definition = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_procedure),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        if unsafe { RegisterClassW(&definition) } == 0 {
            eprintln!("[notify] could not register the notification window class");
        }
    });
}

/// The shell reports what happened to the balloon through the callback
/// message: once it says the balloon is gone, the icon can go too.
unsafe extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == CALLBACK_MESSAGE {
        if matches!(
            lparam as u32,
            NIN_BALLOONHIDE | NIN_BALLOONTIMEOUT | NIN_BALLOONUSERCLICK
        ) {
            BALLOON_CLOSED.store(true, Ordering::Release);
        }
        return 0;
    }
    DefWindowProcW(window, message, wparam, lparam)
}

/// ApexShot's own icons, taken from the executable's embedded resource so a
/// notification carries the same mark as the app. The shell uses the small
/// one in the notification area and the largest frame in the balloon body.
fn app_icons() -> (HICON, HICON) {
    if let Ok(executable) = std::env::current_exe() {
        let path = wide(&executable.to_string_lossy());
        let mut large: HICON = std::ptr::null_mut();
        let mut small: HICON = std::ptr::null_mut();
        let extracted = unsafe { ExtractIconExW(path.as_ptr(), 0, &mut large, &mut small, 1) };
        if extracted > 0 && !small.is_null() {
            let balloon = balloon_icon(&path);
            if !large.is_null() {
                unsafe { DestroyIcon(large) };
            }
            return (small, balloon);
        }
        if !large.is_null() {
            let balloon = balloon_icon(&path);
            return (large, balloon);
        }
    }
    let fallback = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
    (fallback, fallback)
}

/// The largest frame in the executable, for the balloon body.
///
/// `ExtractIconExW` only returns the system-large 32×32 frame. The shell draws
/// the body icon at a size that grows with the display scale, so upscaling that
/// frame makes the mark soft; ask for the 256×256 frame instead and let the
/// shell downscale it.
fn balloon_icon(path: &[u16]) -> HICON {
    let mut icon: HICON = std::ptr::null_mut();
    let extracted = unsafe {
        PrivateExtractIconsW(
            path.as_ptr(),
            0,
            256,
            256,
            &mut icon,
            std::ptr::null_mut(),
            1,
            0,
        )
    };
    if extracted > 0 && !icon.is_null() {
        return icon;
    }
    unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Copy `text` into a fixed-size shell field, keeping it terminated.
///
/// The fields count UTF-16 units, not characters, and a truncated surrogate
/// pair is not text: drop a trailing high surrogate instead of letting the
/// shell show half a character.
fn write_utf16(destination: &mut [u16], text: &str) {
    if destination.is_empty() {
        return;
    }
    let mut units: Vec<u16> = text.encode_utf16().take(destination.len() - 1).collect();
    if units
        .last()
        .is_some_and(|unit| (0xD800..0xDC00).contains(unit))
    {
        units.pop();
    }
    destination[..units.len()].copy_from_slice(&units);
    destination[units.len()] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bodyless_notification_puts_the_summary_in_the_body() {
        let notification = Notification {
            summary: "Copied to clipboard".to_string(),
            body: String::new(),
            important: false,
        };
        assert_eq!(notification.fields(), (app_name(), "Copied to clipboard"));
    }

    #[test]
    fn a_notification_with_a_body_keeps_the_summary_as_its_title() {
        let notification = Notification {
            summary: "Upload complete".to_string(),
            body: "Share link copied to clipboard".to_string(),
            important: false,
        };
        assert_eq!(
            notification.fields(),
            ("Upload complete", "Share link copied to clipboard")
        );
    }

    #[test]
    fn an_empty_notification_is_a_noop() {
        desktop_notification("", "");
        desktop_notification_important("", "");
    }

    #[test]
    fn a_field_is_terminated_and_holds_the_whole_text() {
        let mut field = [0xFFFF_u16; 16];
        write_utf16(&mut field, "ApexShot");
        assert_eq!(&field[..9], &[65, 112, 101, 120, 83, 104, 111, 116, 0]);
    }

    #[test]
    fn a_long_field_is_cut_short_and_stays_terminated() {
        let mut field = [0xFFFF_u16; 8];
        write_utf16(&mut field, "a much longer notification body");
        assert_eq!(&field[..8], &[97, 32, 109, 117, 99, 104, 32, 0]);
    }

    #[test]
    fn a_cut_surrogate_pair_is_dropped_instead_of_split() {
        // The emoji is two UTF-16 units; a field with room for one of them must
        // not keep half of it.
        let mut field = [0xFFFF_u16; 4];
        write_utf16(&mut field, "ab\u{1F600}");
        assert_eq!(&field[..4], &[97, 98, 0, 0xFFFF]);
    }

    #[test]
    fn smoke_send_notification_when_requested() {
        if std::env::var_os("APEXSHOT_NOTIFY_SMOKE").is_none() {
            return;
        }
        desktop_notification(
            "ApexShot notifications",
            "A system notification from the Windows build.",
        );
        desktop_notification_important(
            "ApexShot notifications",
            "An important notification from the Windows build.",
        );
        std::thread::sleep(Duration::from_millis(2_000));
    }
}

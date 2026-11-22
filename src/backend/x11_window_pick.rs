//! Interactive window picking for the X11 still-capture helper.
//!
//! Wayland takes its window grant from the ScreenCast portal. Desktops such as
//! Cinnamon, Xfce and MATE have no such portal, so the helper grabs the pointer
//! on the root window and reads the top-level child under the click instead.

use std::{cell::Cell, collections::VecDeque, time::Duration};

use anyhow::{anyhow, bail, Context};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            AtomEnum, ButtonIndex, ConnectionExt as _, CursorEnum, EventMask, GrabMode, GrabStatus,
            Keycode, Window,
        },
        Event,
    },
    rust_connection::RustConnection,
    CURRENT_TIME, NONE,
};

use super::{CaptureData, DisplayBackend, DisplayError, DisplayResult, X11Backend};

/// How many times the pointer and keyboard grabs are attempted while the
/// toolkit may still own them.
const GRAB_ATTEMPTS: u32 = 10;
const GRAB_RETRY_DELAY: Duration = Duration::from_millis(50);

/// The first frame is read after the toolbar has had a chance to disappear.
const FIRST_CAPTURE_SETTLE: Duration = Duration::from_millis(150);

/// How far to descend from the frame looking for the client window.
const MAX_CLIENT_SEARCH_DEPTH: u8 = 3;

const XC_CROSSHAIR: u16 = 34;
const XC_CROSSHAIR_MASK: u16 = 35;
const ESCAPE_KEYSYM: u32 = 0xff1b;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// A picked window that a recording follows: the frame and client windows plus
/// the visible rectangle they covered at pick time. The recording captures the
/// screen and re-crops it to the window's current visible rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X11WindowSource {
    /// The window-manager frame; also the fallback when no client was found.
    pub frame: u32,
    /// The client window inside the frame, when one carried `WM_STATE`.
    pub client: Option<u32>,
    /// The pick-time visible rectangle in root coordinates, rounded down to
    /// even dimensions. It is the recording's fixed output size and the
    /// pointer sidecar's capture region.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// The root size at pick time, so the initial crop needs no X query.
    pub screen_width: u32,
    pub screen_height: u32,
}

/// Follows a picked window's current visible rectangle on its own X connection.
/// A recording uses it to re-crop the screen whenever the window moves or
/// resizes.
pub struct X11WindowFollower {
    conn: RustConnection,
    root: Window,
    frame: Window,
    client: Option<Window>,
    extents_atom: u32,
    net_extents_atom: u32,
    screen: (u32, u32),
}

impl X11WindowFollower {
    pub fn new(source: &X11WindowSource) -> anyhow::Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).context("Failed to connect to the X server")?;
        let screen = conn
            .setup()
            .roots
            .get(screen_num)
            .ok_or_else(|| anyhow!("The X server reported no screen"))?;
        let root = screen.root;
        let screen = (
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        );
        let extents_atom = intern_atom(&conn, b"_GTK_FRAME_EXTENTS")?;
        let net_extents_atom = intern_atom(&conn, b"_NET_FRAME_EXTENTS")?;
        Ok(Self {
            conn,
            root,
            frame: source.frame,
            client: source.client,
            extents_atom,
            net_extents_atom,
            screen,
        })
    }

    /// The root size this follower measured on its own connection.
    pub fn screen_size(&self) -> (u32, u32) {
        self.screen
    }

    /// The window's current visible rectangle in root coordinates, or `None`
    /// when the window is gone or fully off-screen. It applies the same client
    /// and decoration-extent rules the picker uses.
    pub fn visible_rect(&self) -> Option<(i32, i32, u32, u32)> {
        let frame = root_rect(&self.conn, self.frame, self.root).ok()?;
        let client = self
            .client
            .and_then(|client| root_rect(&self.conn, client, self.root).ok());
        let extents = self
            .client
            .and_then(|client| cardinal4(&self.conn, client, self.extents_atom));
        let net_extents = self
            .client
            .and_then(|client| cardinal4(&self.conn, client, self.net_extents_atom));
        let screen = (
            i32::try_from(self.screen.0).ok()?,
            i32::try_from(self.screen.1).ok()?,
        );
        let rect = capture_rect(frame, client, extents, net_extents, screen)?;
        Some((
            rect.x,
            rect.y,
            u32::try_from(rect.width).ok()?,
            u32::try_from(rect.height).ok()?,
        ))
    }
}

/// The `videocrop` margins that leave only `rect` on a `screen`-sized root.
pub fn crop_for_rect(rect: (i32, i32, u32, u32), screen: (u32, u32)) -> (u32, u32, u32, u32) {
    let (x, y, width, height) = rect;
    let right_edge = i64::from(x) + i64::from(width);
    let bottom_edge = i64::from(y) + i64::from(height);
    let left = i64::from(x).max(0);
    let top = i64::from(y).max(0);
    let right = (i64::from(screen.0) - right_edge).max(0);
    let bottom = (i64::from(screen.1) - bottom_edge).max(0);
    (
        u32::try_from(left).unwrap_or(0),
        u32::try_from(top).unwrap_or(0),
        u32::try_from(right).unwrap_or(0),
        u32::try_from(bottom).unwrap_or(0),
    )
}

/// Result of a pick: either a window ready for captures, or a user cancel.
pub enum PickOutcome {
    Picked(Box<X11PickedWindow>),
    Cancelled,
}

/// A window picked on X11, with everything needed to re-capture it later.
pub struct X11PickedWindow {
    conn: RustConnection,
    root: Window,
    frame: Window,
    client: Option<Window>,
    extents_atom: u32,
    net_extents_atom: u32,
    backend: X11Backend,
    screen: (i32, i32),
    include_cursor: bool,
    first_capture: Cell<bool>,
}

impl X11PickedWindow {
    /// Capture the picked window through the X11 backend.
    pub fn capture(&self) -> DisplayResult<CaptureData> {
        if self.first_capture.replace(false) {
            std::thread::sleep(FIRST_CAPTURE_SETTLE);
        }
        let rect = self.current_rect()?;
        let mut capture = self
            .backend
            .capture_area(rect.x, rect.y, rect.width, rect.height)?;
        if !self.include_cursor {
            capture.cursor = None;
        }
        Ok(capture)
    }

    /// The source that follows this window: its windows plus the pick-time
    /// visible rectangle and screen size.
    pub fn follow_source(&self) -> DisplayResult<X11WindowSource> {
        let rect = self.current_rect()?;
        let screen = (
            u32::try_from(self.screen.0).unwrap_or(0),
            u32::try_from(self.screen.1).unwrap_or(0),
        );
        Ok(X11WindowSource {
            frame: self.frame,
            client: self.client,
            x: rect.x,
            y: rect.y,
            width: even_size(u32::try_from(rect.width).unwrap_or(0)),
            height: even_size(u32::try_from(rect.height).unwrap_or(0)),
            screen_width: screen.0,
            screen_height: screen.1,
        })
    }

    /// The window synthetic input should target: the client when the frame
    /// has one, so wheel events reach the application rather than the
    /// window-manager decorations.
    pub fn input_window(&self) -> Window {
        self.client.unwrap_or(self.frame)
    }

    /// Re-read the window's current root-space rectangle; it may have moved.
    fn current_rect(&self) -> DisplayResult<Rect> {
        let (frame, client, extents, net_extents) = self.window_parts()?;
        capture_rect(frame, client, extents, net_extents, self.screen).ok_or_else(|| {
            DisplayError::CaptureError("The selected window is no longer visible".into())
        })
    }

    /// The frame and client rectangles plus both decoration-extent properties.
    fn window_parts(
        &self,
    ) -> DisplayResult<(Rect, Option<Rect>, Option<[u32; 4]>, Option<[u32; 4]>)> {
        let frame = root_rect(&self.conn, self.frame, self.root).map_err(|_| {
            DisplayError::CaptureError("The selected window is no longer available".into())
        })?;
        let client = self
            .client
            .and_then(|client| root_rect(&self.conn, client, self.root).ok());
        let extents = self
            .client
            .and_then(|client| cardinal4(&self.conn, client, self.extents_atom));
        let net_extents = self
            .client
            .and_then(|client| cardinal4(&self.conn, client, self.net_extents_atom));
        Ok((frame, client, extents, net_extents))
    }
}

/// Let the user click a window on the root window.
pub fn pick_window(include_cursor: bool) -> anyhow::Result<PickOutcome> {
    let (conn, screen_num) = x11rb::connect(None).context("Failed to connect to the X server")?;
    let screen = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| anyhow!("The X server reported no screen"))?;
    let root = screen.root;
    let screen_size = (
        i32::from(screen.width_in_pixels),
        i32::from(screen.height_in_pixels),
    );

    let escape_keys = escape_keycodes(&conn)?;
    let cursor = crosshair_cursor(&conn).unwrap_or(u32::from(CursorEnum::NONE));

    let frame = {
        let _grab = PointerKeyboardGrab::acquire(&conn, root, cursor)?;
        loop {
            match conn.wait_for_event()? {
                Event::ButtonPress(event) if event.detail == u8::from(ButtonIndex::M1) => {
                    if event.child != NONE {
                        break event.child;
                    }
                }
                Event::ButtonPress(event) if event.detail == u8::from(ButtonIndex::M3) => {
                    return Ok(PickOutcome::Cancelled);
                }
                Event::KeyPress(event) if escape_keys.contains(&event.detail) => {
                    return Ok(PickOutcome::Cancelled);
                }
                _ => {}
            }
        }
    };

    let wm_state = intern_atom(&conn, b"WM_STATE")?;
    let extents_atom = intern_atom(&conn, b"_GTK_FRAME_EXTENTS")?;
    let net_extents_atom = intern_atom(&conn, b"_NET_FRAME_EXTENTS")?;
    let client = find_client_window(&conn, frame, wm_state);
    let backend = X11Backend::new()?;
    let picked = X11PickedWindow {
        conn,
        root,
        frame,
        client,
        extents_atom,
        net_extents_atom,
        backend,
        screen: screen_size,
        include_cursor,
        first_capture: Cell::new(true),
    };
    picked.current_rect()?;
    Ok(PickOutcome::Picked(Box::new(picked)))
}

/// One JSON line for the pick helper: the picked window's visible rectangle,
/// a user cancel, or a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickReport {
    Picked {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        window: u32,
    },
    Cancelled,
    Error(String),
}

impl PickReport {
    /// The compact one-line payload the capture helper parses.
    pub fn to_json(&self) -> String {
        match self {
            PickReport::Picked {
                x,
                y,
                width,
                height,
                window,
            } => format!(
                r#"{{"x":{x},"y":{y},"width":{width},"height":{height},"window":{window}}}"#
            ),
            PickReport::Cancelled => r#"{"cancelled":true}"#.to_string(),
            PickReport::Error(message) => {
                format!(r#"{{"error":"{}"}}"#, json_escape(message))
            }
        }
    }
}

fn json_escape(value: &str) -> String {
    use std::fmt::Write as _;

    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

/// Run the `x11-pick-window-internal` helper: pick a window with the root
/// crosshair and print its rectangle for the capture overlay.
pub fn run_internal_helper() -> anyhow::Result<()> {
    let report = match pick_window(false) {
        Ok(PickOutcome::Picked(window)) => match window.current_rect() {
            Ok(rect) if rect.width > 0 && rect.height > 0 => PickReport::Picked {
                x: rect.x,
                y: rect.y,
                width: rect.width as u32,
                height: rect.height as u32,
                window: window.input_window(),
            },
            Ok(_) => PickReport::Error("The selected window has no visible area.".into()),
            Err(error) => PickReport::Error(error.to_string()),
        },
        Ok(PickOutcome::Cancelled) => PickReport::Cancelled,
        Err(error) => PickReport::Error(error.to_string()),
    };

    {
        use std::io::Write as _;

        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{}", report.to_json())?;
        stdout.flush()?;
    }

    match report {
        PickReport::Error(message) => bail!(message),
        _ => Ok(()),
    }
}

/// Active pointer and keyboard grabs released when the pick ends.
struct PointerKeyboardGrab<'a> {
    conn: &'a RustConnection,
    cursor: u32,
    pointer: bool,
    keyboard: bool,
}

impl<'a> PointerKeyboardGrab<'a> {
    fn acquire(conn: &'a RustConnection, root: Window, cursor: u32) -> anyhow::Result<Self> {
        let mut guard = Self {
            conn,
            cursor,
            pointer: false,
            keyboard: false,
        };
        for attempt in 0..GRAB_ATTEMPTS {
            let pointer = conn
                .grab_pointer(
                    false,
                    root,
                    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    NONE,
                    cursor,
                    CURRENT_TIME,
                )?
                .reply()?;
            if pointer.status != GrabStatus::SUCCESS {
                if attempt + 1 == GRAB_ATTEMPTS {
                    break;
                }
                std::thread::sleep(GRAB_RETRY_DELAY);
                continue;
            }
            guard.pointer = true;
            let keyboard = conn
                .grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?
                .reply()?;
            if keyboard.status == GrabStatus::SUCCESS {
                guard.keyboard = true;
                return Ok(guard);
            }
            guard.release_pointer();
            if attempt + 1 == GRAB_ATTEMPTS {
                break;
            }
            std::thread::sleep(GRAB_RETRY_DELAY);
        }
        bail!("Could not grab the pointer for window selection; another application is holding it")
    }

    fn release_pointer(&mut self) {
        if self.pointer {
            let _ = self.conn.ungrab_pointer(CURRENT_TIME);
            self.pointer = false;
        }
    }
}

impl Drop for PointerKeyboardGrab<'_> {
    fn drop(&mut self) {
        if self.keyboard {
            let _ = self.conn.ungrab_keyboard(CURRENT_TIME);
        }
        self.release_pointer();
        if self.cursor != NONE {
            let _ = self.conn.free_cursor(self.cursor);
        }
        let _ = self.conn.flush();
    }
}

/// Create the crosshair glyph from the core cursor font, if the font exists.
fn crosshair_cursor(conn: &RustConnection) -> Option<u32> {
    let font = conn.generate_id().ok()?;
    conn.open_font(font, b"cursor").ok()?.check().ok()?;
    let cursor = conn.generate_id().ok()?;
    let created = conn
        .create_glyph_cursor(
            cursor,
            font,
            font,
            XC_CROSSHAIR,
            XC_CROSSHAIR_MASK,
            0xffff,
            0xffff,
            0xffff,
            0x0000,
            0x0000,
            0x0000,
        )
        .ok()?
        .check();
    let _ = conn.close_font(font);
    created.ok()?;
    Some(cursor)
}

/// Every keycode whose mapping includes Escape.
fn escape_keycodes(conn: &RustConnection) -> anyhow::Result<Vec<Keycode>> {
    let setup = conn.setup();
    let count = setup.max_keycode - setup.min_keycode + 1;
    let reply = conn
        .get_keyboard_mapping(setup.min_keycode, count)?
        .reply()?;
    let per_keycode = usize::from(reply.keysyms_per_keycode);
    if per_keycode == 0 {
        return Ok(Vec::new());
    }
    let mut keycodes = Vec::new();
    for (index, keysyms) in reply.keysyms.chunks(per_keycode).enumerate() {
        if !keysyms.contains(&ESCAPE_KEYSYM) {
            continue;
        }
        if let Ok(index) = u8::try_from(index) {
            keycodes.push(setup.min_keycode + index);
        }
    }
    Ok(keycodes)
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> anyhow::Result<u32> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

/// Find the client window below a frame: the nearest descendant with WM_STATE.
fn find_client_window(conn: &RustConnection, frame: Window, wm_state: u32) -> Option<Window> {
    let mut queue = VecDeque::from([(frame, 0u8)]);
    while let Some((window, depth)) = queue.pop_front() {
        if has_property(conn, window, wm_state) {
            return Some(window);
        }
        if depth >= MAX_CLIENT_SEARCH_DEPTH {
            continue;
        }
        let Ok(cookie) = conn.query_tree(window) else {
            continue;
        };
        let Ok(tree) = cookie.reply() else {
            continue;
        };
        for child in tree.children {
            queue.push_back((child, depth + 1));
        }
    }
    None
}

fn has_property(conn: &RustConnection, window: Window, atom: u32) -> bool {
    conn.get_property(false, window, atom, AtomEnum::ANY, 0, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.type_ != u32::from(AtomEnum::NONE))
}

/// Read a window's rectangle in root coordinates.
fn root_rect(conn: &RustConnection, window: Window, root: Window) -> anyhow::Result<Rect> {
    let geometry = conn.get_geometry(window)?.reply()?;
    let translated = conn.translate_coordinates(window, root, 0, 0)?.reply()?;
    Ok(Rect {
        x: i32::from(translated.dst_x),
        y: i32::from(translated.dst_y),
        width: i32::from(geometry.width),
        height: i32::from(geometry.height),
    })
}

/// Read a left, right, top, bottom CARDINAL[4] property such as the shadow a
/// client-side-decorated window draws (`_GTK_FRAME_EXTENTS`) or the visible
/// decorations the window manager adds (`_NET_FRAME_EXTENTS`).
fn cardinal4(conn: &RustConnection, window: Window, atom: u32) -> Option<[u32; 4]> {
    let reply = conn
        .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 4)
        .ok()?
        .reply()
        .ok()?;
    let values: Vec<u32> = reply.value32()?.take(4).collect();
    match values.as_slice() {
        [left, right, top, bottom] => Some([*left, *right, *top, *bottom]),
        _ => None,
    }
}

/// The visible rectangle in root coordinates before clamping: the client minus
/// its CSD shadow, the client plus the window manager's visible decorations, or
/// the frame as a last resort.
fn visible_base(
    frame: Rect,
    client: Option<Rect>,
    extents: Option<[u32; 4]>,
    net_extents: Option<[u32; 4]>,
) -> (i64, i64, i64, i64) {
    match (client, extents, net_extents) {
        (Some(client), Some([left, right, top, bottom]), _) => (
            i64::from(client.x) + i64::from(left),
            i64::from(client.y) + i64::from(top),
            i64::from(client.width) - i64::from(left) - i64::from(right),
            i64::from(client.height) - i64::from(top) - i64::from(bottom),
        ),
        (Some(client), None, Some([left, right, top, bottom])) => (
            i64::from(client.x) - i64::from(left),
            i64::from(client.y) - i64::from(top),
            i64::from(client.width) + i64::from(left) + i64::from(right),
            i64::from(client.height) + i64::from(top) + i64::from(bottom),
        ),
        _ => (
            i64::from(frame.x),
            i64::from(frame.y),
            i64::from(frame.width),
            i64::from(frame.height),
        ),
    }
}

/// The rectangle to capture, clamped to the screen: the client minus its CSD
/// shadow, or the client plus the window manager's visible decorations, or the
/// frame as a last resort. The frame itself can include invisible resize
/// borders, so it is not preferred. `None` when nothing visible is left.
fn capture_rect(
    frame: Rect,
    client: Option<Rect>,
    extents: Option<[u32; 4]>,
    net_extents: Option<[u32; 4]>,
    screen: (i32, i32),
) -> Option<Rect> {
    let (base_x, base_y, base_width, base_height) =
        visible_base(frame, client, extents, net_extents);
    let (screen_width, screen_height) = (i64::from(screen.0), i64::from(screen.1));
    let left = base_x.max(0);
    let top = base_y.max(0);
    let right = (base_x + base_width).min(screen_width);
    let bottom = (base_y + base_height).min(screen_height);
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect {
        x: i32::try_from(left).ok()?,
        y: i32::try_from(top).ok()?,
        width: i32::try_from(right - left).ok()?,
        height: i32::try_from(bottom - top).ok()?,
    })
}

/// A window dimension the encoder accepts: even and at least two.
fn even_size(value: u32) -> u32 {
    (value & !1).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn csd_shadow_extents_are_excluded() {
        let picked = capture_rect(
            rect(100, 100, 300, 400),
            Some(rect(110, 120, 280, 380)),
            Some([10, 10, 20, 10]),
            None,
            (1920, 1080),
        );
        assert_eq!(picked, Some(rect(120, 140, 260, 350)));
    }

    #[test]
    fn frame_is_used_without_extents_or_client() {
        let frame = rect(100, 100, 300, 200);
        let client = rect(110, 120, 280, 180);
        assert_eq!(
            capture_rect(frame, Some(client), None, None, (1920, 1080)),
            Some(frame)
        );
        assert_eq!(
            capture_rect(frame, None, Some([10, 10, 10, 10]), None, (1920, 1080)),
            Some(frame)
        );
    }

    #[test]
    fn wm_decorations_are_added_and_invisible_borders_left_out() {
        // A frame with 10 px invisible resize borders around a client under
        // a 32 px title bar: only the title bar and client are captured.
        let picked = capture_rect(
            rect(949, 142, 754, 484),
            Some(rect(959, 182, 734, 434)),
            None,
            Some([0, 0, 32, 0]),
            (1920, 1080),
        );
        assert_eq!(picked, Some(rect(959, 150, 734, 466)));
    }

    #[test]
    fn result_is_clamped_to_the_screen() {
        assert_eq!(
            capture_rect(rect(-50, -40, 300, 200), None, None, None, (1000, 800)),
            Some(rect(0, 0, 250, 160))
        );
        assert_eq!(
            capture_rect(rect(900, 700, 300, 200), None, None, None, (1000, 800)),
            Some(rect(900, 700, 100, 100))
        );
    }

    #[test]
    fn clamping_happens_after_the_extents_shrink() {
        let picked = capture_rect(
            rect(900, 700, 300, 200),
            Some(rect(900, 700, 300, 200)),
            Some([20, 20, 30, 30]),
            None,
            (1000, 800),
        );
        assert_eq!(picked, Some(rect(920, 730, 80, 70)));
    }

    #[test]
    fn fully_hidden_or_empty_windows_have_no_rect() {
        assert_eq!(
            capture_rect(rect(1000, 0, 100, 100), None, None, None, (1000, 800)),
            None
        );
        assert_eq!(
            capture_rect(rect(0, 0, 0, 0), None, None, None, (1000, 800)),
            None
        );
        assert_eq!(
            capture_rect(
                rect(100, 100, 300, 300),
                Some(rect(110, 110, 100, 100)),
                Some([60, 60, 60, 60]),
                None,
                (1000, 800)
            ),
            None
        );
    }

    #[test]
    fn the_crop_leaves_the_window_rectangle_on_the_screen() {
        // A 300x200 window at (100, 120) on a 1920x1080 root.
        assert_eq!(
            crop_for_rect((100, 120, 300, 200), (1920, 1080)),
            (100, 120, 1520, 760)
        );
    }

    #[test]
    fn a_window_touching_the_screen_edges_leaves_no_crop_there() {
        assert_eq!(
            crop_for_rect((0, 0, 1920, 1080), (1920, 1080)),
            (0, 0, 0, 0)
        );
        assert_eq!(
            crop_for_rect((1820, 980, 100, 100), (1920, 1080)),
            (1820, 980, 0, 0)
        );
    }

    #[test]
    fn window_dimensions_round_down_to_even() {
        assert_eq!(even_size(101), 100);
        assert_eq!(even_size(480), 480);
        assert_eq!(even_size(1), 2);
        assert_eq!(even_size(0), 2);
    }

    #[test]
    fn pick_report_json_has_one_shape_per_outcome() {
        assert_eq!(
            PickReport::Picked {
                x: -10,
                y: 20,
                width: 640,
                height: 480,
                window: 0x1a2b,
            }
            .to_json(),
            r#"{"x":-10,"y":20,"width":640,"height":480,"window":6699}"#
        );
        assert_eq!(PickReport::Cancelled.to_json(), r#"{"cancelled":true}"#);
        assert_eq!(
            PickReport::Error("bad \"value\"\n".into()).to_json(),
            r#"{"error":"bad \"value\"\n"}"#
        );
    }
}

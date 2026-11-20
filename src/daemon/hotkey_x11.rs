//! X11 key-grab hotkey tier for desktops without GNOME Shell or a working
//! GlobalShortcuts portal (Cinnamon, Xfce, MATE, …).
//!
//! Bindings are converted into (keycode, modifier) pairs from the server's
//! keyboard mapping, grabbed on the root window, and matched in a blocking
//! event loop on a dedicated thread.

use std::collections::HashMap;

use anyhow::Context;
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{ConnectionExt as _, GrabMode, Keycode, ModMask, Timestamp, Window},
        ErrorKind, Event,
    },
    rust_connection::RustConnection,
};

use crate::hotkeys::{HotkeyBinding, HotkeyConfig};

use super::DaemonAction;

const SHIFT_MASK: u16 = 1 << 0;
const LOCK_MASK: u16 = 1 << 1;
const CONTROL_MASK: u16 = 1 << 2;
const MOD1_MASK: u16 = 1 << 3;
const MOD2_MASK: u16 = 1 << 4;
const MOD4_MASK: u16 = 1 << 6;

/// Modifier bits that identify a shortcut; lock bits are stripped from events.
const MOD_MASK_BITS: u16 = SHIFT_MASK | CONTROL_MASK | MOD1_MASK | MOD4_MASK;

/// Caps Lock and Num Lock change the event state but not the shortcut.
const LOCK_VARIANTS: [u16; 4] = [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK];

/// True when the session is X11, where keys must be grabbed directly.
pub(super) fn is_x11_session() -> bool {
    crate::backend::is_x11_session()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ParsedAccelerator {
    pub(super) modifiers: u16,
    pub(super) keysym: u32,
}

/// Parse a hotkey accelerator ("CTRL+ALT+S", "<Shift>Print", …) into an X11
/// modifier mask and keysym.
pub(super) fn parse_accelerator(input: &str) -> Option<ParsedAccelerator> {
    let normalized = crate::hotkeys::accel_to_portal(input);
    let parts: Vec<&str> = normalized
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let (key_part, modifier_parts) = parts.split_last()?;

    let mut modifiers = 0u16;
    for modifier in modifier_parts {
        modifiers |= match modifier.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => CONTROL_MASK,
            "SHIFT" => SHIFT_MASK,
            "ALT" => MOD1_MASK,
            "SUPER" | "META" | "LOGO" | "WIN" => MOD4_MASK,
            _ => return None,
        };
    }

    Some(ParsedAccelerator {
        modifiers,
        keysym: keysym_for_name(key_part)?,
    })
}

/// Map a key name from a hotkey config to its X keysym.
fn keysym_for_name(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.len() == 1 {
        let ch = lower.chars().next()?;
        if ch.is_ascii_alphanumeric() {
            return Some(ch as u32);
        }
    }
    match lower.as_str() {
        "print" | "printscreen" => Some(0xff61),
        "escape" | "esc" => Some(0xff1b),
        "return" | "enter" => Some(0xff0d),
        "space" => Some(0x0020),
        "tab" => Some(0xff09),
        "home" => Some(0xff50),
        "end" => Some(0xff57),
        "insert" => Some(0xff63),
        "delete" => Some(0xffff),
        "page_up" | "pageup" | "prior" => Some(0xff55),
        "page_down" | "pagedown" | "next" => Some(0xff56),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        "pause" => Some(0xff13),
        "menu" => Some(0xff67),
        "caps_lock" => Some(0xffe5),
        "comma" => Some(0x2c),
        "period" => Some(0x2e),
        "slash" => Some(0x2f),
        "semicolon" => Some(0x3b),
        "apostrophe" => Some(0x27),
        "bracketleft" => Some(0x5b),
        "bracketright" => Some(0x5d),
        "backslash" => Some(0x5c),
        "minus" => Some(0x2d),
        "plus" => Some(0x2b),
        "equal" => Some(0x3d),
        "grave" => Some(0x60),
        _ => function_key_keysym(&lower),
    }
}

fn function_key_keysym(lower: &str) -> Option<u32> {
    let number: u8 = lower.strip_prefix('f')?.parse().ok()?;
    (1..=24)
        .contains(&number)
        .then(|| 0xffbe + u32::from(number) - 1)
}

/// Keysym → keycode(s) for the whole keyboard.
fn keyboard_keycodes(conn: &RustConnection) -> anyhow::Result<HashMap<u32, Vec<Keycode>>> {
    let setup = conn.setup();
    let min_keycode = setup.min_keycode;
    let count = setup
        .max_keycode
        .saturating_sub(min_keycode)
        .saturating_add(1);
    if count == 0 {
        anyhow::bail!("X11 server reported an empty keyboard mapping");
    }
    let reply = conn
        .get_keyboard_mapping(min_keycode, count)
        .map_err(|error| anyhow::anyhow!("GetKeyboardMapping failed: {error}"))?
        .reply()
        .map_err(|error| anyhow::anyhow!("GetKeyboardMapping reply failed: {error}"))?;
    let per_keycode = usize::from(reply.keysyms_per_keycode).max(1);

    let mut keycodes: HashMap<u32, Vec<Keycode>> = HashMap::new();
    for (index, chunk) in reply.keysyms.chunks(per_keycode).enumerate() {
        let Ok(offset) = u8::try_from(index) else {
            break;
        };
        let keycode = min_keycode.saturating_add(offset);
        for &keysym in chunk {
            keycodes.entry(keysym).or_default().push(keycode);
        }
    }
    Ok(keycodes)
}

/// Grab every configured binding that resolves to a key on this keyboard.
/// Bindings already owned by the desktop are logged and skipped.
fn grab_bindings(
    conn: &RustConnection,
    root: Window,
    cfg: &HotkeyConfig,
) -> anyhow::Result<HashMap<(Keycode, u16), HotkeyBinding>> {
    let keycodes = keyboard_keycodes(conn)?;
    let mut grabs: HashMap<(Keycode, u16), HotkeyBinding> = HashMap::new();
    let mut registered = 0usize;

    for binding in &cfg.bindings {
        let Some(parsed) = parse_accelerator(&binding.accelerator) else {
            eprintln!(
                "[daemon] X11 shortcut {} skipped: unsupported key",
                binding.accelerator
            );
            continue;
        };
        let Some(keycodes) = keycodes.get(&parsed.keysym) else {
            eprintln!(
                "[daemon] X11 shortcut {} skipped: key not present on this keyboard",
                binding.accelerator
            );
            continue;
        };

        let mut grabbed = false;
        let mut access_denied = false;
        for &keycode in keycodes {
            let mut keycode_grabbed = false;
            for lock in LOCK_VARIANTS {
                let modifiers = ModMask::from(parsed.modifiers | lock);
                let result = conn
                    .grab_key(
                        false,
                        root,
                        modifiers,
                        keycode,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    )
                    .map_err(x11rb::errors::ReplyError::ConnectionError)
                    .and_then(|cookie| cookie.check());
                match result {
                    Ok(()) => {
                        keycode_grabbed = true;
                        grabbed = true;
                    }
                    Err(x11rb::errors::ReplyError::X11Error(error))
                        if error.error_kind == ErrorKind::Access =>
                    {
                        access_denied = true;
                    }
                    Err(_) => {}
                }
            }
            if keycode_grabbed {
                grabs.insert((keycode, parsed.modifiers), binding.clone());
            }
        }

        if grabbed {
            registered += 1;
        } else if access_denied {
            eprintln!(
                "[daemon] X11 shortcut {} unavailable: already used by another application",
                binding.accelerator
            );
        } else {
            eprintln!(
                "[daemon] X11 shortcut {} unavailable: key grab failed",
                binding.accelerator
            );
        }
    }

    let _ = conn.flush();
    if grabs.is_empty() {
        anyhow::bail!("no X11 shortcuts could be grabbed");
    }
    eprintln!(
        "[daemon] {} hotkey(s) registered via X11 key grabs.",
        registered
    );
    Ok(grabs)
}

fn run_x11_event_loop(
    conn: RustConnection,
    grabs: HashMap<(Keycode, u16), HotkeyBinding>,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) {
    // Auto-repeat arrives as a Release+Press pair sharing one timestamp. The
    // release of a shortcut that opens a keyboard-grabbing overlay may never
    // reach us, so a "still pressed" flag would swallow the next real press.
    let mut repeat_pair: Option<(Keycode, Timestamp)> = None;
    loop {
        match conn.wait_for_event() {
            Ok(Event::KeyPress(event)) => {
                if repeat_pair.take() == Some((event.detail, event.time)) {
                    continue;
                }

                let modifiers = u16::from(event.state) & MOD_MASK_BITS;
                let Some(binding) = grabs.get(&(event.detail, modifiers)) else {
                    continue;
                };
                if super::is_hotkey_suppressed() {
                    eprintln!("[daemon] Hotkey suppressed (shortcut edit active)");
                    continue;
                }
                if let Some(action) = super::hotkey_listener::binding_to_daemon_action(binding) {
                    eprintln!("[daemon] Hotkey fired: {:?}", action);
                    let _ = tx.send(action);
                }
            }
            Ok(Event::KeyRelease(event)) => {
                repeat_pair = Some((event.detail, event.time));
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("[daemon] X11 hotkey listener stopped: {error}");
                break;
            }
        }
    }
}

/// Tier for X11 sessions: grab the configured accelerators on the root window
/// and dispatch matching key presses. Never returns while the grabs are held.
pub(super) async fn run_hotkey_listener_x11(
    cfg: &HotkeyConfig,
    tx: std::sync::mpsc::Sender<DaemonAction>,
) -> anyhow::Result<()> {
    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|error| anyhow::anyhow!("Failed to connect to the X11 display: {error}"))?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .map(|screen| screen.root)
        .context("X11 server reported no screen")?;

    let grabs = grab_bindings(&conn, root, cfg)?;

    std::thread::Builder::new()
        .name("x11-hotkeys".to_string())
        .spawn(move || run_x11_event_loop(conn, grabs, tx))
        .context("Failed to spawn the X11 hotkey listener thread")?;

    // Keep the async listener alive for the daemon's lifetime; the thread owns
    // the connection and its key grabs.
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ctrl_alt_key() {
        let parsed = parse_accelerator("CTRL+ALT+S").expect("valid accelerator");
        assert_eq!(parsed.modifiers, CONTROL_MASK | MOD1_MASK);
        assert_eq!(parsed.keysym, u32::from(b's'));
    }

    #[test]
    fn parses_shift_super_digit() {
        let parsed = parse_accelerator("SHIFT+SUPER+4").expect("valid accelerator");
        assert_eq!(parsed.modifiers, SHIFT_MASK | MOD4_MASK);
        assert_eq!(parsed.keysym, u32::from(b'4'));
    }

    #[test]
    fn parses_bare_print() {
        let parsed = parse_accelerator("Print").expect("valid accelerator");
        assert_eq!(parsed.modifiers, 0);
        assert_eq!(parsed.keysym, 0xff61);
    }

    #[test]
    fn parses_three_modifiers() {
        let parsed = parse_accelerator("CTRL+ALT+SHIFT+S").expect("valid accelerator");
        assert_eq!(parsed.modifiers, CONTROL_MASK | MOD1_MASK | SHIFT_MASK);
        assert_eq!(parsed.keysym, u32::from(b's'));
    }

    #[test]
    fn parsing_is_case_insensitive() {
        let lower = parse_accelerator("ctrl+alt+s");
        let upper = parse_accelerator("CTRL+ALT+S");
        assert_eq!(lower, upper);
        assert!(lower.is_some());
    }

    #[test]
    fn parses_gnome_style_accelerator() {
        let parsed = parse_accelerator("<Ctrl><Shift>Print").expect("valid accelerator");
        assert_eq!(parsed.modifiers, CONTROL_MASK | SHIFT_MASK);
        assert_eq!(parsed.keysym, 0xff61);
    }

    #[test]
    fn parses_function_key() {
        let parsed = parse_accelerator("CTRL+F12").expect("valid accelerator");
        assert_eq!(parsed.modifiers, CONTROL_MASK);
        assert_eq!(parsed.keysym, 0xffc9);
    }

    #[test]
    fn unknown_key_is_rejected() {
        assert_eq!(parse_accelerator("CTRL+ALT+Nonsense"), None);
        assert_eq!(parse_accelerator("HYPER+S"), None);
    }

    #[test]
    fn lock_variants_cover_caps_and_num_lock() {
        assert_eq!(LOCK_VARIANTS.len(), 4);
        for required in [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
            assert!(LOCK_VARIANTS.contains(&required));
        }
    }

    #[test]
    fn modifier_mask_drops_lock_bits() {
        let state = SHIFT_MASK | CONTROL_MASK | LOCK_MASK | MOD2_MASK;
        assert_eq!(state & MOD_MASK_BITS, SHIFT_MASK | CONTROL_MASK);
    }
}

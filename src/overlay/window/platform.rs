use gtk4::gdk;
use gtk4::{prelude::*, ApplicationWindow, CssProvider};
use x11rb::wrapper::ConnectionExt;

pub(crate) fn install_overlay_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            window.overlay {
                background-color: transparent;
                transition: none;
                transition-duration: 0s;
                animation: none;
                animation-duration: 0s;
            }

            window.overlay > * {
                background-color: transparent;
            }

            drawingarea {
                background-color: transparent;
            }
            ",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

/// On X11, tell the compositor to treat this window as a transient system
/// overlay (no open/close animation, no taskbar entry, no pager entry).
///
/// This is called from `connect_realize` — i.e. the XID exists but the
/// window has not been mapped yet — so the compositor sees all hints on
/// the very first MapNotify and never starts an animation.
pub(crate) fn suppress_x11_compositor_animation(window: &ApplicationWindow) {
    use gdk4x11::X11Surface;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{self, ConnectionExt as _},
    };

    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(x11_surface) = surface.downcast::<X11Surface>() else {
        return; // Wayland – nothing to do
    };
    let Ok(xid) = u32::try_from(x11_surface.xid()) else {
        return;
    };
    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    // _NET_WM_BYPASS_COMPOSITOR = 1
    // Asks the compositor to skip compositing this window entirely, which
    // also disables any open/close transition effects.
    if let Ok(cookie) = conn.intern_atom(false, b"_NET_WM_BYPASS_COMPOSITOR") {
        if let Ok(reply) = cookie.reply() {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                reply.atom,
                xproto::AtomEnum::CARDINAL,
                &[1u32],
            );
        }
    }

    // _NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_UTILITY
    // UTILITY windows are never animated by compositors (Mutter, KWin, Picom).
    // We prefer UTILITY over SPLASH because SPLASH can cause focus/stacking
    // issues on some window managers.
    if let (Ok(type_cookie), Ok(util_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE"),
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY"),
    ) {
        if let (Ok(type_reply), Ok(util_reply)) = (type_cookie.reply(), util_cookie.reply()) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                type_reply.atom,
                xproto::AtomEnum::ATOM,
                &[util_reply.atom],
            );
        }
    }

    // _NET_WM_STATE: add SKIP_TASKBAR + SKIP_PAGER so the overlay never
    // appears in the taskbar or workspace switcher.
    if let (Ok(state_cookie), Ok(skip_taskbar_cookie), Ok(skip_pager_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_STATE"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER"),
    ) {
        if let (Ok(state_reply), Ok(skip_taskbar_reply), Ok(skip_pager_reply)) = (
            state_cookie.reply(),
            skip_taskbar_cookie.reply(),
            skip_pager_cookie.reply(),
        ) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                state_reply.atom,
                xproto::AtomEnum::ATOM,
                &[skip_taskbar_reply.atom, skip_pager_reply.atom],
            );
        }
    }

    let _ = conn.flush();
}

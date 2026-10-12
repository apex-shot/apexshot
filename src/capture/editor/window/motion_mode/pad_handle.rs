//! The marker the Motion pads paint: one soft, plain white handle.
//!
//! Shared by the position, anchor, and text pads, so every point the Motion
//! inspector lets you choose is drawn the same way.

use gtk4::cairo;

/// Half the handle's side.
pub(super) const HANDLE_HALF: f64 = 13.0;
/// Corner radius: a soft square, not a disc.
const HANDLE_RADIUS: f64 = 9.0;

/// Paint the handle centred on `(x, y)`.
///
/// A flat white shape with a blurred shadow, rather than a light disc ringed
/// in white around an accent core. Each pad's field is a plain surface, so the
/// marker only has to state which point is chosen; the soft edge keeps it from
/// reading as a second widget parked on the pad, and white carries on the dark
/// field, the light theme's pale panel, and a thumbnail alike.
///
/// The shape is inscribed in the radius the pads already reserve for their
/// travel — its furthest point is its rounded corner, 14.7 from centre — so no
/// pad has to widen its inset for it.
pub(super) fn draw_soft_handle(context: &cairo::Context, x: f64, y: f64) {
    let shadow = cairo::RadialGradient::new(
        x,
        y + 2.0,
        HANDLE_HALF * 0.35,
        x,
        y + 2.0,
        HANDLE_HALF * 1.9,
    );
    shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.42);
    shadow.add_color_stop_rgba(0.55, 0.0, 0.0, 0.0, 0.18);
    shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    context.set_source(&shadow).ok();
    context.arc(x, y + 2.0, HANDLE_HALF * 1.9, 0.0, std::f64::consts::TAU);
    context.fill().ok();

    crate::capture::editor::render::rounded_rect_path(
        context,
        x - HANDLE_HALF,
        y - HANDLE_HALF,
        HANDLE_HALF * 2.0,
        HANDLE_HALF * 2.0,
        HANDLE_RADIUS,
    );
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.fill().ok();
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_handle_is_plain_white_with_a_soft_edge() {
        // The marker used to be a nearly white disc around an accent core on
        // every pad, and the position pad's field carried a dot matrix under
        // it. The handle is one plain white shape with a blurred shadow: the
        // shadow is the whole of its chrome, so no ring, no core, and no
        // second colour come back onto it.
        let source = include_str!("pad_handle.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        assert!(
            production.contains("RadialGradient::new"),
            "the handle keeps its soft edge"
        );
        assert!(
            production.contains("set_source_rgb(1.0, 1.0, 1.0)"),
            "the handle is plain white"
        );
        assert!(
            !production.contains("0.690, 0.361, 0.220"),
            "the accent core is gone: the marker is one white shape"
        );
    }
}

/// The clip held on the clipboard, drawn where a click would drop it.
///
/// Copy and Cut put a clip aside with no visible result, so the only feedback
/// was that nothing appeared to happen. Showing the span under the pointer
/// answers both questions at once: is anything copied, and where would it land.
/// Every editor surveyed for this keeps paste-at-playhead and gives no ghost;
/// this app has no playhead-driven paste yet, so the ghost carries the intent
/// of the click instead.
pub fn draw_clipboard_ghost(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    start: f64,
    light: bool,
    is_zoom_track: bool,
) {
    let Some(duration) = state.clipboard_duration_for(is_zoom_track) else {
        return;
    };
    let placed = start.max(0.0);
    let x0 = state.time_to_x(placed, w);
    let x1 = state.time_to_x(placed + duration, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let height = h - 14.0;

    // A placeable ghost reads as the real pill, so what the click will produce
    // is obvious. One that would collide keeps the outline and drops the fill,
    // matching the hollow treatment a disabled clip already uses.
    let free = state.paste_spot_is_free(placed, duration, is_zoom_track);
    let base = if is_zoom_track {
        (0.0, 0.0, 1.0)
    } else {
        (0.4, 0.0, 0.2)
    };
    let fill_alpha = if free { 0.45 } else { 0.10 };
    rounded_rect(cr, x0, y, clip_w, height, 5.0);
    if light {
        let ring = if is_zoom_track {
            (0.0, 0.0, 0.45)
        } else {
            (0.20, 0.0, 0.10)
        };
        cr.set_source_rgba(base.0, base.1, base.2, fill_alpha);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(ring.0, ring.1, ring.2, 0.85);
    } else {
        cr.set_source_rgba(base.0, base.1, base.2, fill_alpha + 0.15);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
    }
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);

    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, if free { 0.92 } else { 0.55 });
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let label = if free { t("Paste") } else { t("Occupied") };
        let _ = cr.show_text(&label);
    }
}

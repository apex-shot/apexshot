use super::drawing::rounded_rect_path;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ToolbarIcon {
    Area,
    Fullscreen,
    Window,
    Scroll,
    Timer,
    Ocr,
    Recording,
    Controls,
    Crop,
    Mic,
    Speaker,

    Video,
    Gif,
}

pub(crate) const TOOLBAR_ICONS: [ToolbarIcon; 7] = [
    ToolbarIcon::Area,
    ToolbarIcon::Fullscreen,
    ToolbarIcon::Window,
    ToolbarIcon::Scroll,
    ToolbarIcon::Timer,
    ToolbarIcon::Ocr,
    ToolbarIcon::Recording,
];

pub(crate) const TOOLBAR_AREA_INDEX: usize = 0;
pub(crate) const TOOLBAR_FULLSCREEN_INDEX: usize = 1;
pub(crate) const TOOLBAR_WINDOW_INDEX: usize = 2;
pub(crate) const TOOLBAR_SCROLL_INDEX: usize = 3;
pub(crate) const TOOLBAR_RECORDING_INDEX: usize = 6;

pub(crate) const TOOLBAR_LABELS: [&str; 7] = [
    "Area",
    "Fullscreen",
    "Window",
    "Scroll",
    "Timer",
    "OCR",
    "Recording",
];

pub(crate) fn draw_toolbar_icon(
    context: &gtk4::cairo::Context,
    icon: ToolbarIcon,
    cx: f64,
    cy: f64,
    color: (f64, f64, f64, f64),
) {
    let _ = context.save();
    context.new_path();
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.set_line_width(1.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);

    match icon {
        ToolbarIcon::Area => {
            let h = 5.5;
            context.move_to(cx - 7.0, cy - 1.5);
            context.line_to(cx - 7.0, cy - h);
            context.line_to(cx - 1.5, cy - h);

            context.move_to(cx + 1.5, cy - h);
            context.line_to(cx + 7.0, cy - h);
            context.line_to(cx + 7.0, cy - 1.5);

            context.move_to(cx - 7.0, cy + 1.5);
            context.line_to(cx - 7.0, cy + h);
            context.line_to(cx - 1.5, cy + h);

            context.move_to(cx + 1.5, cy + h);
            context.line_to(cx + 7.0, cy + h);
            context.line_to(cx + 7.0, cy + 1.5);
            let _ = context.stroke();
        }
        ToolbarIcon::Fullscreen => {
            rounded_rect_path(context, cx - 7.0, cy - 6.0, 14.0, 10.5, 2.0);
            let _ = context.stroke();
            context.move_to(cx, cy + 4.5);
            context.line_to(cx, cy + 7.5);
            context.move_to(cx - 4.5, cy + 7.5);
            context.line_to(cx + 4.5, cy + 7.5);
            let _ = context.stroke();
        }
        ToolbarIcon::Window => {
            rounded_rect_path(context, cx - 7.0, cy - 5.5, 14.0, 9.5, 1.7);
            let _ = context.stroke();
            context.move_to(cx - 7.0, cy - 2.0);
            context.line_to(cx + 7.0, cy - 2.0);
            let _ = context.stroke();
        }
        ToolbarIcon::Scroll => {
            context.new_path();
            context.move_to(cx, cy - 4.8);
            context.line_to(cx, cy + 1.8);
            context.move_to(cx - 3.2, cy - 1.0);
            context.line_to(cx, cy + 1.9);
            context.line_to(cx + 3.2, cy - 1.0);
            let _ = context.stroke();
        }
        ToolbarIcon::Timer => {
            context.new_path();
            context.arc(cx, cy, 6.0, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.new_path();
            context.move_to(cx, cy);
            context.line_to(cx, cy - 2.8);
            context.move_to(cx, cy);
            context.line_to(cx + 2.2, cy + 1.7);
            let _ = context.stroke();
        }
        ToolbarIcon::Ocr => {
            context.select_font_face(
                "Sans",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            context.set_font_size(8.0);
            if let Ok(extents) = context.text_extents("Aa") {
                let text_x = cx - extents.width() / 2.0 - extents.x_bearing();
                let text_y = cy - (extents.y_bearing() + extents.height() / 2.0) + 0.2;
                context.move_to(text_x, text_y);
                let _ = context.show_text("Aa");
            }
        }
        ToolbarIcon::Recording => {
            rounded_rect_path(context, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
            let _ = context.stroke();
            context.move_to(cx + 2.4, cy - 2.8);
            context.line_to(cx + 7.4, cy - 5.2);
            context.line_to(cx + 7.4, cy + 5.2);
            context.line_to(cx + 2.4, cy + 2.8);
            context.close_path();
            let _ = context.stroke();
        }
        ToolbarIcon::Controls => {
            for i in 0..3 {
                let x = cx - 4.5 + i as f64 * 4.5;
                context.move_to(x, cy - 6.0);
                context.line_to(x, cy + 6.0);
                let slider_y = if i == 0 {
                    cy - 2.0
                } else if i == 1 {
                    cy + 2.0
                } else {
                    cy - 1.0
                };
                context.arc(x, slider_y, 1.8, 0.0, PI * 2.0);
            }
            let _ = context.stroke();
        }
        ToolbarIcon::Crop => {
            context.set_line_cap(gtk4::cairo::LineCap::Butt);
            context.set_line_join(gtk4::cairo::LineJoin::Miter);
            let s = 10.5;
            let t = 2.8;
            let o = 1.2;
            context.move_to(cx - s / 2.0 - t, cy - s / 2.0 + o);
            context.line_to(cx + s / 2.0 - o, cy - s / 2.0 + o);
            context.move_to(cx - s / 2.0 + o, cy - s / 2.0 - t);
            context.line_to(cx - s / 2.0 + o, cy + s / 2.0 - o);
            context.move_to(cx + s / 2.0 + t, cy + s / 2.0 - o);
            context.line_to(cx - s / 2.0 + o, cy + s / 2.0 - o);
            context.move_to(cx + s / 2.0 - o, cy + s / 2.0 + t);
            context.line_to(cx + s / 2.0 - o, cy - s / 2.0 + o);
            let _ = context.stroke();
        }
        ToolbarIcon::Mic => {
            rounded_rect_path(context, cx - 3.1, cy - 7.0, 6.2, 9.6, 3.1);
            let _ = context.stroke();
            context.move_to(cx - 5.0, cy - 0.3);
            context.line_to(cx - 5.0, cy + 1.6);
            context.move_to(cx + 5.0, cy - 0.3);
            context.line_to(cx + 5.0, cy + 1.6);
            context.arc(cx, cy + 0.7, 5.0, 0.0, PI);
            context.move_to(cx, cy + 6.1);
            context.line_to(cx, cy + 8.3);
            context.move_to(cx - 3.4, cy + 8.3);
            context.line_to(cx + 3.4, cy + 8.3);
            let _ = context.stroke();
        }
        ToolbarIcon::Speaker => {
            context.move_to(cx - 6.8, cy - 2.3);
            context.line_to(cx - 4.4, cy - 2.3);
            context.line_to(cx - 1.2, cy - 5.1);
            context.line_to(cx - 1.2, cy + 5.1);
            context.line_to(cx - 4.4, cy + 2.3);
            context.line_to(cx - 6.8, cy + 2.3);
            context.close_path();
            let _ = context.stroke();
            context.arc(cx - 0.8, cy, 5.0, -0.7, 0.7);
            context.arc(cx + 1.2, cy, 7.0, -0.7, 0.7);
            let _ = context.stroke();
        }
        ToolbarIcon::Video => {
            rounded_rect_path(context, cx - 8.0, cy - 5.0, 10.5, 10.0, 2.5);
            let _ = context.stroke();
            context.move_to(cx + 2.4, cy - 2.8);
            context.line_to(cx + 7.4, cy - 5.2);
            context.line_to(cx + 7.4, cy + 5.2);
            context.line_to(cx + 2.4, cy + 2.8);
            context.close_path();
            let _ = context.stroke();
        }
        ToolbarIcon::Gif => {
            rounded_rect_path(context, cx - 9.0, cy - 6.0, 18.0, 12.0, 3.0);
            context.set_source_rgba(color.0, color.1, color.2, color.3);
            let _ = context.fill();
            context.select_font_face(
                "Sans",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            context.set_font_size(6.5);
            context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
            if let Ok(extents) = context.text_extents("GIF") {
                let text_x = cx - extents.width() / 2.0 - extents.x_bearing();
                let text_y = cy - extents.height() / 2.0 - extents.y_bearing() + 0.5;
                context.move_to(text_x, text_y);
                let _ = context.show_text("GIF");
            }
        }
    }

    let _ = context.restore();
}

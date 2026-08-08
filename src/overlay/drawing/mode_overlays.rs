use super::super::background::BackgroundFrame;
use super::super::layout::*;
use super::super::recording::state::OverlayIntent;
use std::f64::consts::PI;

fn draw_popup_panel(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) {
    // Same polished glass treatment used by the recording menus: warm glow,
    // soft shadow, frosted fill and subtle rim highlight.
    let _ = context.save();
    let glow_cx = x + width * 0.5;
    let glow_cy = y + height * 0.45;
    let glow_r = width.max(height) * 0.9;
    let glow = gtk4::cairo::RadialGradient::new(glow_cx, glow_cy, 0.0, glow_cx, glow_cy, glow_r);
    glow.add_color_stop_rgba(0.0, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 34.0 / 255.0);
    glow.add_color_stop_rgba(0.65, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 0.0);
    let _ = context.set_source(&glow);
    context.rectangle(x - 42.0, y - 42.0, width + 84.0, height + 84.0);
    let _ = context.fill();
    let _ = context.restore();

    super::draw_frosted_panel(
        context,
        x,
        y,
        width,
        height,
        radius,
        screen_width,
        screen_height,
        background,
    );
}

pub(super) fn draw_countdown_bubble(
    context: &gtk4::cairo::Context,
    _sel_x: f64,
    _sel_y: f64,
    _sel_w: f64,
    _sel_h: f64,
    screen_width: f64,
    screen_height: f64,
    countdown_value: i32,
    hovered_cancel: bool,
    intent: OverlayIntent,
) {
    let _ = context.save();

    if intent != OverlayIntent::Record {
        // Capture-delay countdown: C++-matching pill badge at top-center.
        let pill_w = 112.0;
        let pill_h = 44.0;
        let pill_x = (screen_width - pill_w) / 2.0;
        let pill_y = 28.0;

        // Draw pill background
        if hovered_cancel {
            context.set_source_rgba(0.78, 0.24, 0.16, 0.95);
        } else {
            context.set_source_rgba(0.91, 0.33, 0.13, 0.92);
        }
        super::rounded_rect_path(context, pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
        let _ = context.fill();

        // Draw clock icon on the left
        let icon_cx = pill_x + 22.0;
        let icon_cy = pill_y + pill_h / 2.0;
        let icon_r = 11.0;
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(2.2);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        context.arc(icon_cx, icon_cy, icon_r, 0.0, PI * 2.0);
        let _ = context.stroke();
        // Clock hands
        context.move_to(icon_cx, icon_cy);
        context.line_to(icon_cx, icon_cy - 5.5);
        context.move_to(icon_cx, icon_cy);
        context.line_to(icon_cx + 5.0, icon_cy + 2.0);
        let _ = context.stroke();

        // Draw countdown number
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(if hovered_cancel { 13.0 } else { 22.0 });
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let text = if hovered_cancel {
            "Cancel".to_string()
        } else {
            countdown_value.to_string()
        };
        if let Ok(extents) = context.text_extents(&text) {
            let text_x =
                pill_x + 40.0 + (pill_w - 44.0 - extents.width()) / 2.0 - extents.x_bearing();
            let text_y = pill_y + (pill_h - extents.height()) / 2.0 - extents.y_bearing();
            context.move_to(text_x, text_y);
            let _ = context.show_text(&text);
        }
    } else {
        // Recording countdown: centered circle (3-2-1), matching C++.
        let bubble_size = 184.0;
        let bubble_x = (screen_width - bubble_size) / 2.0;
        let bubble_y = (screen_height - bubble_size) / 2.0;

        if hovered_cancel {
            context.set_source_rgba(132.0 / 255.0, 38.0 / 255.0, 24.0 / 255.0, 242.0 / 255.0);
        } else {
            context.set_source_rgba(0.0, 0.0, 0.0, 240.0 / 255.0);
        }
        context.arc(
            bubble_x + bubble_size / 2.0,
            bubble_y + bubble_size / 2.0,
            bubble_size / 2.0,
            0.0,
            PI * 2.0,
        );
        let _ = context.fill();

        // Draw countdown number or "Cancel"
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(if hovered_cancel { 34.0 } else { 72.0 });
        if hovered_cancel {
            context.set_source_rgba(1.0, 0.89, 0.84, 1.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        }

        let text = if hovered_cancel {
            "Cancel".to_string()
        } else {
            countdown_value.to_string()
        };
        if let Ok(extents) = context.text_extents(&text) {
            let text_x = bubble_x + (bubble_size - extents.width()) / 2.0 - extents.x_bearing();
            let text_y = bubble_y + (bubble_size + extents.height()) / 2.0 - extents.y_bearing();
            context.move_to(text_x, text_y);
            let _ = context.show_text(&text);
        }
    }

    let _ = context.restore();
}

pub(super) fn draw_scroll_popup(
    context: &gtk4::cairo::Context,
    center_x: f64,
    center_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    hovered_close: bool,
    hovered_download: bool,
) -> RectF {
    let _ = context.save();

    let popup_w = 360.0;
    let popup_h = 170.0;
    let popup_x = center_x - popup_w / 2.0;
    let popup_y = center_y - popup_h / 2.0;

    draw_popup_panel(
        context,
        popup_x,
        popup_y,
        popup_w,
        popup_h,
        12.0,
        screen_width,
        screen_height,
        background,
    );

    // Close button
    let close_size = 22.0;
    let close_x = popup_x + popup_w - close_size - 10.0;
    let close_y = popup_y + 10.0;
    if hovered_close {
        context.set_source_rgba(0.8, 0.25, 0.15, 1.0);
    } else {
        context.set_source_rgba(60.0 / 255.0, 60.0 / 255.0, 60.0 / 255.0, 1.0);
    }
    super::rounded_rect_path(context, close_x, close_y, close_size, close_size, 5.0);
    let _ = context.fill();
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(1.5);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(close_x + 6.0, close_y + 6.0);
    context.line_to(close_x + close_size - 6.0, close_y + close_size - 6.0);
    context.move_to(close_x + close_size - 6.0, close_y + 6.0);
    context.line_to(close_x + 6.0, close_y + close_size - 6.0);
    let _ = context.stroke();

    // Title
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(13.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.move_to(popup_x + 20.0, popup_y + 30.0);
    let _ = context.show_text("Scroll Capture");

    // Body text
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 180.0 / 255.0);
    context.move_to(popup_x + 20.0, popup_y + 55.0);
    let _ = context.show_text("Scroll capture requires the ApexShot");
    context.move_to(popup_x + 20.0, popup_y + 73.0);
    let _ = context.show_text("browser extension.");

    // CTA button — match the glass/orange accent style instead of a flat block.
    let btn_w = 182.0;
    let btn_h = 34.0;
    let btn_x = popup_x + (popup_w - btn_w) / 2.0;
    let btn_y = popup_y + 102.0;

    let btn_rect = RectF {
        x: btn_x,
        y: btn_y,
        width: btn_w,
        height: btn_h,
    };

    // Button shadow
    super::rounded_rect_path(context, btn_x, btn_y + 1.5, btn_w, btn_h, 10.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.22);
    let _ = context.fill();

    // Button gradient — brightened on hover
    let btn_grad = gtk4::cairo::LinearGradient::new(0.0, btn_y, 0.0, btn_y + btn_h);
    if hovered_download {
        btn_grad.add_color_stop_rgba(0.0, 1.0, 142.0 / 255.0, 88.0 / 255.0, 0.98);
        btn_grad.add_color_stop_rgba(1.0, 220.0 / 255.0, 112.0 / 255.0, 68.0 / 255.0, 0.96);
    } else {
        btn_grad.add_color_stop_rgba(0.0, 242.0 / 255.0, 116.0 / 255.0, 70.0 / 255.0, 0.96);
        btn_grad.add_color_stop_rgba(1.0, 176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 0.94);
    }
    super::rounded_rect_path(context, btn_x, btn_y, btn_w, btn_h, 10.0);
    let _ = context.set_source(&btn_grad);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 0.34);
    context.set_line_width(1.0);
    let _ = context.stroke();

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(13.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    let btn_text = "Download Extension";
    if let Ok(extents) = context.text_extents(btn_text) {
        let text_x = btn_x + (btn_w - extents.width()) / 2.0 - extents.x_bearing();
        let text_y = btn_y + (btn_h - extents.height()) / 2.0 - extents.y_bearing();
        context.move_to(text_x, text_y);
        let _ = context.show_text(btn_text);
    }

    let _ = context.restore();

    btn_rect
}

pub(super) fn draw_crosshair_mode_bubble(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    label: &str,
    screen_width: f64,
    screen_height: f64,
) {
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(12.0);

    let (text_w, text_h) = context
        .text_extents(label)
        .map(|e| (e.width(), e.height()))
        .unwrap_or((64.0, 14.0));
    let bubble_w = text_w + 22.0;
    let bubble_h = text_h + 14.0;
    let mut bx = x + 14.0;
    let mut by = y + 14.0;

    if bx + bubble_w > screen_width - 8.0 {
        bx = x - bubble_w - 14.0;
    }
    if by + bubble_h > screen_height - 8.0 {
        by = y - bubble_h - 14.0;
    }
    bx = bx.clamp(8.0, (screen_width - bubble_w - 8.0).max(8.0));
    by = by.clamp(8.0, (screen_height - bubble_h - 8.0).max(8.0));

    super::rounded_rect_path(context, bx, by, bubble_w, bubble_h, 6.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
    let _ = context.fill();

    super::rounded_rect_path(
        context,
        bx + 0.5,
        by + 0.5,
        bubble_w - 1.0,
        bubble_h - 1.0,
        6.0,
    );
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.0);
    let _ = context.stroke();

    super::draw_text_centered(
        context,
        RectF {
            x: bx,
            y: by,
            width: bubble_w,
            height: bubble_h,
        },
        label,
        12.0,
        false,
        (1.0, 1.0, 1.0, 1.0),
    );
}

pub(super) fn draw_window_picker(
    context: &gtk4::cairo::Context,
    center_x: f64,
    center_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    windows: &[crate::compositor::WindowInfo],
    hovered_entry: i32,
) {
    if windows.is_empty() {
        return;
    }

    let _ = context.save();
    const MENU_W: f64 = 320.0;
    const ITEM_H: f64 = 28.0;
    const HEADER_H: f64 = 30.0;
    const PAD: f64 = 8.0;

    let n = windows.len();
    let total_h = PAD * 2.0 + HEADER_H + n as f64 * ITEM_H;
    let popup_x = (center_x - MENU_W / 2.0).clamp(10.0, (screen_width - MENU_W - 10.0).max(10.0));
    let popup_y =
        (center_y - total_h / 2.0).clamp(10.0, (screen_height - total_h - 10.0).max(10.0));

    draw_popup_panel(
        context,
        popup_x,
        popup_y,
        MENU_W,
        total_h,
        12.0,
        screen_width,
        screen_height,
        background,
    );

    // Section header "Select a Window"
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
    context.move_to(popup_x + 18.0, popup_y + PAD + 14.0);
    let _ = context.show_text("Select a Window");

    let mut curr_y = popup_y + PAD + HEADER_H;

    for (i, win) in windows.iter().enumerate() {
        let item_rect = RectF {
            x: popup_x + 4.0,
            y: curr_y + 1.0,
            width: MENU_W - 8.0,
            height: ITEM_H - 2.0,
        };
        let hovered = i as i32 == hovered_entry;

        if hovered {
            super::rounded_rect_path(
                context,
                item_rect.x,
                item_rect.y,
                item_rect.width,
                item_rect.height,
                6.0,
            );
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 220.0 / 255.0);
            let _ = context.fill();
        }

        let text_color = if hovered {
            (1.0, 234.0 / 255.0, 214.0 / 255.0, 1.0)
        } else {
            (241.0 / 255.0, 241.0 / 255.0, 243.0 / 255.0, 1.0)
        };

        // Class label (bold)
        let class_label = if win.class.is_empty() {
            "Window"
        } else {
            &win.class
        };
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.0);
        context.set_source_rgba(text_color.0, text_color.1, text_color.2, text_color.3);
        if let Ok(_ext) = context.text_extents(class_label) {
            context.move_to(popup_x + 18.0, curr_y + ITEM_H / 2.0 + 4.5);
            let _ = context.show_text(class_label);
        }

        // Window title (truncated to fit)
        let title = if win.title.is_empty() {
            "Untitled".to_string()
        } else {
            win.title.clone()
        };
        let max_title_w = MENU_W - 200.0;
        let mut display_str = title;
        if let Ok(ext) = context.text_extents(&display_str) {
            if ext.width() > max_title_w {
                let chars: Vec<char> = display_str.chars().collect();
                let mut char_count = chars.len();
                while char_count > 3 {
                    char_count -= 1;
                    let truncated: String = chars[..char_count].iter().collect();
                    let t = format!("{}…", truncated);
                    if let Ok(e) = context.text_extents(&t) {
                        if e.width() <= max_title_w {
                            display_str = t;
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        context.set_font_size(12.0);
        context.set_source_rgba(text_color.0, text_color.1, text_color.2, 0.7);
        if context.text_extents(&display_str).is_ok() {
            context.move_to(popup_x + 190.0, curr_y + ITEM_H / 2.0 + 4.0);
            let _ = context.show_text(&display_str);
        }

        curr_y += ITEM_H;
    }

    let _ = context.restore();
}

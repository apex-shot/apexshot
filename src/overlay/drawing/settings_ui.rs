use super::super::background::BackgroundFrame;
use super::super::layout::*;
use super::super::recording::layout::compute_dropdown_popup_y;
use super::super::recording::state::SettingsTab;
use std::f64::consts::PI;

pub(crate) fn draw_checkbox(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    size: f64,
    checked: bool,
    disabled: bool,
    accent_r: f64,
    accent_g: f64,
    accent_b: f64,
) {
    context.new_path();
    super::rounded_rect_path(context, x, y, size, size, 4.0);
    if checked && !disabled {
        context.set_source_rgba(accent_r, accent_g, accent_b, 1.0);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(2.0);
        context.move_to(x + 4.0, y + size * 0.5);
        context.line_to(x + 8.0, y + size * 0.75);
        context.line_to(x + size - 3.5, y + size * 0.3);
        context.stroke().ok();
    } else {
        let alpha = if disabled { 35.0 / 255.0 } else { 60.0 / 255.0 };
        let bg_alpha = if disabled { 25.0 / 255.0 } else { 40.0 / 255.0 };
        context.set_source_rgba(0.0, 0.0, 0.0, bg_alpha);
        context.fill_preserve().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, alpha);
        context.set_line_width(1.5);
        context.stroke().ok();
    }
}

pub(crate) fn draw_dropdown_button(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    hovered: bool,
) {
    context.set_source_rgba(0.0, 0.0, 0.0, 60.0 / 255.0);
    super::rounded_rect_path(context, x, y, w, h, 6.0);
    if hovered {
        context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
    }
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.0);
    super::rounded_rect_path(context, x, y, w, h, 6.0);
    context.stroke().ok();

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents(label) {
        context.move_to(
            x + 10.0 - extents.x_bearing(),
            y + h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text(label).ok();
    }
    // Chevron
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(1.5);
    context.move_to(x + w - 15.0, y + h / 2.0 - 3.0);
    context.line_to(x + w - 11.0, y + h / 2.0 + 1.0);
    context.line_to(x + w - 7.0, y + h / 2.0 - 3.0);
    context.stroke().ok();
}

pub(crate) fn draw_settings_menu(
    context: &gtk4::cairo::Context,
    panel_x: f64,
    panel_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    tab: SettingsTab,
    hovered_item: i32,
    dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    rec_controls: bool,
    hidpi: bool,
    do_not_disturb: bool,
    remember_selection: bool,
    dim_screen: bool,
    show_countdown: bool,
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
    _accent_rim: (f64, f64, f64),
) {
    let menu_w = 440.0;
    let menu_h = 560.0;
    let menu_x = panel_x.clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = panel_y.clamp(10.0, screen_height - menu_h - 10.0);

    let accent_r = 176.0 / 255.0;
    let accent_g = 92.0 / 255.0;
    let accent_b = 56.0 / 255.0;
    let accent_rim = (1.0, 214.0 / 255.0, 186.0 / 255.0);

    // Glow
    let _ = context.save();
    let glow_cx = menu_x + menu_w / 2.0;
    let glow_cy = menu_y + menu_h / 2.0;
    let glow = gtk4::cairo::RadialGradient::new(glow_cx, glow_cy, 0.0, glow_cx, glow_cy, menu_w);
    glow.add_color_stop_rgba(0.0, accent_r, accent_g, accent_b, 40.0 / 255.0);
    glow.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.0);
    let _ = context.set_source(&glow);
    context.rectangle(menu_x - 40.0, menu_y - 40.0, menu_w + 80.0, menu_h + 80.0);
    let _ = context.fill();
    let _ = context.restore();

    super::draw_frosted_panel(
        context,
        menu_x,
        menu_y,
        menu_w,
        menu_h,
        12.0,
        screen_width,
        screen_height,
        background,
    );

    // Header
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 176.0 / 255.0);
    if let Ok(_ext) = context.text_extents("RECORDING CONTROLS") {
        context.move_to(menu_x + 18.0, menu_y + 28.0);
        context.show_text("RECORDING CONTROLS").ok();
    }
    context.set_font_size(18.7);
    context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
    if let Ok(_ext) = context.text_extents("Recording Setup") {
        context.move_to(menu_x + 18.0, menu_y + 48.0);
        context.show_text("Recording Setup").ok();
    }

    // Tabs
    let tabs = ["General", "Video", "GIF"];
    let tab_w = 78.0;
    let tab_h = 32.0;
    let tab_start_x = menu_x + (menu_w - tabs.len() as f64 * tab_w) / 2.0;
    let tab_y = menu_y + 64.0;

    for (i, tab_label) in tabs.iter().enumerate() {
        let tr = RectF {
            x: tab_start_x + i as f64 * tab_w,
            y: tab_y,
            width: tab_w,
            height: tab_h,
        };
        let is_active_tab = (i == 0 && matches!(tab, SettingsTab::General))
            || (i == 1 && matches!(tab, SettingsTab::Video))
            || (i == 2 && matches!(tab, SettingsTab::Gif));
        let tab_hovered = hovered_item == i as i32;
        if is_active_tab || tab_hovered {
            if is_active_tab {
                context.set_source_rgba(accent_r, accent_g, accent_b, 84.0 / 255.0);
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, 14.0 / 255.0);
            }
            super::rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
            context.fill().ok();
            if is_active_tab {
                let _ = context.save();
                super::rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
                context.clip();
                super::rounded_rect_path(
                    context,
                    tr.x + 0.5,
                    tr.y + 0.5,
                    tr.width - 1.0,
                    tr.height - 1.0,
                    8.5,
                );
                context.set_source_rgba(accent_rim.0, accent_rim.1, accent_rim.2, 1.0);
                context.set_line_width(1.0);
                context.stroke().ok();
                let _ = context.restore();
            }
        }
        let tab_text_color = if is_active_tab || tab_hovered {
            (1.0, 236.0 / 255.0, 220.0 / 255.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, 150.0 / 255.0)
        };
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            if is_active_tab || tab_hovered {
                gtk4::cairo::FontWeight::Bold
            } else {
                gtk4::cairo::FontWeight::Normal
            },
        );
        context.set_font_size(13.7);
        context.set_source_rgba(
            tab_text_color.0,
            tab_text_color.1,
            tab_text_color.2,
            tab_text_color.3,
        );
        if let Ok(extents) = context.text_extents(tab_label) {
            context.move_to(
                tr.x + tr.width / 2.0 - extents.width() / 2.0 - extents.x_bearing(),
                tr.y + tr.height / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(tab_label).ok();
        }
    }

    match tab {
        SettingsTab::General => draw_settings_general_tab(
            context,
            menu_x,
            menu_y,
            menu_w,
            hovered_item,
            rec_controls,
            hidpi,
            do_not_disturb,
            remember_selection,
            dim_screen,
            show_countdown,
            accent_r,
            accent_g,
            accent_b,
        ),
        SettingsTab::Video => draw_settings_video_tab(
            context,
            menu_x,
            menu_y,
            menu_w,
            hovered_item,
            video_max_res,
            video_fps,
            record_mono,
            open_editor,
            accent_r,
            accent_g,
            accent_b,
        ),
        SettingsTab::Gif => draw_settings_gif_tab(
            context,
            menu_x,
            menu_y,
            menu_w,
            hovered_item,
            gif_fps,
            gif_quality,
            optimize_gif,
            gif_size_idx,
            accent_r,
            accent_g,
            accent_b,
        ),
    }

    if let Some(drop_idx) = dropdown_open {
        draw_settings_dropdown_popup(
            context,
            menu_x,
            menu_y,
            menu_w,
            tab,
            drop_idx,
            hovered_item,
            video_max_res,
            video_fps,
            gif_size_idx,
            accent_r,
            accent_g,
            accent_b,
        );
    }
}

pub(crate) fn draw_settings_general_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    menu_w: f64,
    hovered_item: i32,
    rec_controls: bool,
    hidpi: bool,
    do_not_disturb: bool,
    remember_selection: bool,
    dim_screen: bool,
    show_countdown: bool,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
) {
    let label_x = menu_x + 25.0;
    let value_x = menu_x + 140.0;
    let check_area_w = menu_w - (value_x - menu_x) - 20.0; // 280
    let desc_x = value_x + 28.0;
    let row_h = 32.0;
    let mut y = menu_y + 110.0;
    let mut idx = 3;

    macro_rules! s {
        ($label:expr, $desc:expr, $checked:expr) => {{
            draw_general_row(
                context,
                label_x,
                value_x,
                desc_x,
                check_area_w,
                y,
                row_h,
                $label,
                $desc,
                $checked,
                false,
                hovered_item == idx,
            );
            idx += 1;
            y += row_h;
        }};
        ($label:expr, $desc:expr, $checked:expr, $gap:expr) => {{
            y += $gap;
            draw_general_row(
                context,
                label_x,
                value_x,
                desc_x,
                check_area_w,
                y,
                row_h,
                $label,
                $desc,
                $checked,
                false,
                hovered_item == idx,
            );
            idx += 1;
            y += row_h;
        }};
    }

    s!("Controls", "Use keyboard shortcuts", rec_controls);
    s!("HiDPI", "Record at display scale res", hidpi);
    s!("Notifications", "DND while recording", do_not_disturb);
    s!(
        "Recording area",
        "Remember last selection",
        remember_selection,
        10.0
    );
    s!("", "Dim screen while recording", dim_screen);
    s!("", "Show countdown", show_countdown);
    let _ = y;
    let _ = idx;
}

pub(crate) fn draw_general_row(
    context: &gtk4::cairo::Context,
    label_x: f64,
    value_x: f64,
    desc_x: f64,
    _check_area_w: f64,
    y: f64,
    row_h: f64,
    label: &str,
    desc: &str,
    checked: bool,
    disabled: bool,
    hover: bool,
) {
    if !label.is_empty() {
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(
            1.0,
            1.0,
            1.0,
            if disabled {
                110.0 / 255.0
            } else {
                200.0 / 255.0
            },
        );
        if let Ok(extents) = context.text_extents(label) {
            // Right-aligned in 110px area starting at label_x
            let tx = label_x + 110.0 - extents.width() - extents.x_bearing();
            context.move_to(
                tx,
                y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(label).ok();
        }
    }
    if hover {
        super::rounded_rect_path(context, value_x - 5.0, y, 290.0, row_h, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    let cb_size = 18.0;
    draw_checkbox(
        context,
        value_x,
        y + (row_h - cb_size) / 2.0,
        cb_size,
        checked,
        disabled,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, if disabled { 110.0 / 255.0 } else { 1.0 });
    if let Ok(extents) = context.text_extents(desc) {
        // Clip description to available width (252px like C++)
        let max_desc_w = 252.0;
        if extents.width() > max_desc_w {
            let _ = context.save();
            context.rectangle(desc_x, y, max_desc_w, row_h);
            context.clip();
        }
        context.move_to(
            desc_x - extents.x_bearing(),
            y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text(desc).ok();
        if extents.width() > max_desc_w {
            let _ = context.restore();
        }
    }
}

pub(crate) fn draw_settings_video_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    menu_w: f64,
    hovered_item: i32,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
) {
    let label_x = menu_x + 20.0;
    let value_x = menu_x + 130.0;
    let mut curr_y = menu_y + 110.0;

    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 200.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(
                label_x + 100.0 - extents.width() - extents.x_bearing(),
                y + 20.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(txt).ok();
        }
    };

    // Max resolution
    draw_label(context, "Max resolution:", curr_y);
    let res_options = ["Original", "1080p", "720p"];
    draw_dropdown_button(
        context,
        value_x,
        curr_y,
        140.0,
        30.0,
        res_options[video_max_res],
        hovered_item == 3,
    );
    curr_y += 35.0;
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    // Clip subtext to prevent overflow
    let _ = context.save();
    context.rectangle(value_x, curr_y, menu_w - (value_x - menu_x) - 25.0, 80.0);
    context.clip();
    if let Ok(extents) = context.text_extents("Set max res to reduce file size") {
        context.move_to(
            value_x - extents.x_bearing(),
            curr_y + 16.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Set max res to reduce file size").ok();
    }
    let _ = context.restore();
    curr_y += 50.0;

    // Video FPS
    draw_label(context, "Video FPS:", curr_y);
    let fps_options = ["24", "30", "50", "60"];
    draw_dropdown_button(
        context,
        value_x,
        curr_y,
        80.0,
        30.0,
        fps_options[video_fps],
        hovered_item == 4,
    );
    curr_y += 45.0;

    // Record mono
    let mono_hovered = hovered_item == 5;
    if mono_hovered {
        let r = RectF {
            x: value_x,
            y: curr_y,
            width: 200.0,
            height: 30.0,
        };
        super::rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    draw_checkbox(
        context,
        value_x,
        curr_y + (30.0 - 18.0) / 2.0,
        18.0,
        record_mono,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Record audio in mono") {
        context.move_to(
            value_x + 28.0 - extents.x_bearing(),
            curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Record audio in mono").ok();
    }
    curr_y += 50.0;

    // Open editor
    draw_label(context, "Video Encoder:", curr_y);
    let encoder_hovered = hovered_item == 6;
    if encoder_hovered {
        let r = RectF {
            x: value_x,
            y: curr_y,
            width: 250.0,
            height: 30.0,
        };
        super::rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 12.0 / 255.0);
        context.fill().ok();
    }
    draw_checkbox(
        context,
        value_x,
        curr_y + (30.0 - 18.0) / 2.0,
        18.0,
        open_editor,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Open editor after recording") {
        context.move_to(
            value_x + 28.0 - extents.x_bearing(),
            curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Open editor after recording").ok();
    }
    // Clip remaining editor subtext
    curr_y += 35.0;
    let _ = context.save();
    context.rectangle(value_x, curr_y, menu_w - (value_x - menu_x) - 25.0, 80.0);
    context.clip();
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    if let Ok(extents) = context.text_extents("Use editor to change quality and audio") {
        context.move_to(
            value_x - extents.x_bearing(),
            curr_y + 16.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context
            .show_text("Use editor to change quality and audio")
            .ok();
    }
    let _ = context.restore();
    let _ = curr_y;
}

pub(crate) fn draw_settings_gif_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    _menu_w: f64,
    hovered_item: i32,
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
) {
    let label_x = menu_x + 20.0;
    let value_x = menu_x + 130.0;
    let mut curr_y = menu_y + 110.0;

    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 200.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(
                label_x + 100.0 - extents.width() - extents.x_bearing(),
                y + 20.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(txt).ok();
        }
    };

    // GIF FPS
    draw_label(context, "GIF FPS:", curr_y);
    let fps_label = format!("{:.0}", gif_fps);
    context.set_source_rgba(0.0, 0.0, 0.0, 80.0 / 255.0);
    super::rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 28.0 / 255.0);
    context.set_line_width(1.0);
    super::rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
    context.stroke().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    if let Ok(extents) = context.text_extents(&fps_label) {
        context.move_to(
            value_x + 22.5 - extents.width() / 2.0 - extents.x_bearing(),
            curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text(&fps_label).ok();
    }
    // FPS slider
    let slider_x = value_x + 55.0;
    let slider_w = 220.0;
    let track_y = curr_y + (30.0 - 4.0) / 2.0;
    let progress = ((gif_fps - 5.0) / 55.0).clamp(0.0, 1.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    super::rounded_rect_path(context, slider_x, track_y, slider_w, 4.0, 2.0);
    context.fill().ok();
    context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 1.0);
    super::rounded_rect_path(context, slider_x, track_y, slider_w * progress, 4.0, 2.0);
    context.fill().ok();
    let handle_x = slider_x + progress * slider_w;
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.new_path();
    context.arc(handle_x, curr_y + 15.0, 10.0, 0.0, PI * 2.0);
    context.fill().ok();
    curr_y += 50.0;

    // GIF Quality
    draw_label(context, "GIF quality:", curr_y);
    let q_slider_w = 160.0;
    let q_track_y = curr_y + (30.0 - 4.0) / 2.0;
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    super::rounded_rect_path(context, value_x, q_track_y, q_slider_w, 4.0, 2.0);
    context.fill().ok();
    // Ticks
    context.set_source_rgba(1.0, 1.0, 1.0, 60.0 / 255.0);
    context.set_line_width(1.0);
    for i in 0..=8 {
        let tx = value_x + (q_slider_w / 8.0) * i as f64;
        context.move_to(tx, curr_y + 15.0 - 5.0);
        context.line_to(tx, curr_y + 15.0 + 5.0);
        context.stroke().ok();
    }
    // Quality handle
    let q_handle_x = value_x + gif_quality * q_slider_w;
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    super::rounded_rect_path(
        context,
        q_handle_x - 5.0,
        curr_y + (30.0 - 18.0) / 2.0,
        10.0,
        18.0,
        3.0,
    );
    context.fill().ok();
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    if let Ok(_ext) = context.text_extents("Low") {
        context.move_to(value_x, curr_y + 46.0);
        context.show_text("Low").ok();
    }
    if let Ok(_ext) = context.text_extents("High") {
        context.move_to(value_x + q_slider_w - 40.0, curr_y + 46.0);
        context.show_text("High").ok();
    }
    // Optimize checkbox
    let optimize_x = value_x + q_slider_w + 10.0;
    let optimize_y = curr_y;
    draw_checkbox(
        context,
        optimize_x,
        optimize_y + (30.0 - 18.0) / 2.0,
        18.0,
        optimize_gif,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    if let Ok(extents) = context.text_extents("Optimize GIFs") {
        context.move_to(
            optimize_x + 25.0 - extents.x_bearing(),
            optimize_y + 15.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Optimize GIFs").ok();
    }
    curr_y += 115.0;

    // GIF size
    draw_label(context, "GIF size:", curr_y);
    let size_options = ["800 x auto", "640 x auto", "480 x auto", "Original"];
    draw_dropdown_button(
        context,
        value_x,
        curr_y,
        180.0,
        30.0,
        size_options[gif_size_idx],
        hovered_item == 6,
    );
}

pub(crate) fn draw_settings_dropdown_popup(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    _menu_w: f64,
    tab: SettingsTab,
    drop_idx: usize,
    _hovered_item: i32,
    video_max_res: usize,
    video_fps: usize,
    gif_size_idx: usize,
    accent_r: f64,
    accent_g: f64,
    accent_b: f64,
) {
    let (options, current_val): (&[&str], usize) = match (tab, drop_idx) {
        (SettingsTab::Video, 3) => (&["Original", "1080p", "720p"], video_max_res),
        (SettingsTab::Video, 4) => (&["24", "30", "50", "60"], video_fps),
        (SettingsTab::Gif, 6) => (
            &["800 x auto", "640 x auto", "480 x auto", "Original"],
            gif_size_idx,
        ),
        _ => return,
    };
    let value_x = menu_x + 130.0;
    let popup_y = compute_dropdown_popup_y(menu_y, drop_idx, tab);
    let item_h = 30.0;
    let popup_w = 140.0;
    if options.is_empty() {
        return;
    }
    let popup_h = options.len() as f64 * item_h;
    super::draw_frosted_panel(
        context, value_x, popup_y, popup_w, popup_h, 8.0, 0.0, 0.0, None,
    );
    for (i, opt) in options.iter().enumerate() {
        let r = RectF {
            x: value_x,
            y: popup_y + i as f64 * item_h,
            width: popup_w,
            height: item_h,
        };
        if i == current_val {
            let _ = context.save();
            super::rounded_rect_path(
                context,
                r.x + 2.0,
                r.y + 2.0,
                r.width - 4.0,
                r.height - 2.0,
                5.0,
            );
            context.set_source_rgba(accent_r, accent_g, accent_b, 84.0 / 255.0);
            context.fill().ok();
            let _ = context.restore();
        }
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        if let Ok(extents) = context.text_extents(opt) {
            context.move_to(
                r.x + 10.0 - extents.x_bearing(),
                r.y + item_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(opt).ok();
        }
    }
}

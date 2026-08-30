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
        let alpha = if disabled { 26.0 / 255.0 } else { 41.0 / 255.0 };
        let bg_alpha = if disabled { 8.0 / 255.0 } else { 15.0 / 255.0 };
        context.set_source_rgba(1.0, 1.0, 1.0, bg_alpha);
        context.fill_preserve().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, alpha);
        context.set_line_width(1.0);
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
    context.set_source_rgba(1.0, 1.0, 1.0, 15.0 / 255.0);
    super::rounded_rect_path(context, x, y, w, h, 6.0);
    if hovered {
        context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
    }
    context.fill().ok();

    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
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
    _background: Option<&BackgroundFrame>,
    tab: SettingsTab,
    hovered_item: i32,
    hovered_dropdown_item: i32,
    dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    noise_suppression: bool,
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
    let menu_w = crate::overlay::layout::SETTINGS_MENU_WIDTH;
    let menu_h = crate::overlay::layout::SETTINGS_MENU_HEIGHT;
    let menu_x = panel_x.clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = panel_y.clamp(10.0, screen_height - menu_h - 10.0);

    let accent_r = 176.0 / 255.0;
    let accent_g = 92.0 / 255.0;
    let accent_b = 56.0 / 255.0;

    super::draw_frosted_panel(
        context,
        menu_x,
        menu_y,
        menu_w,
        menu_h,
        10.0,
        screen_width,
        screen_height,
        None,
    );

    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(14.0);
    context.set_source_rgba(241.0 / 255.0, 241.0 / 255.0, 243.0 / 255.0, 230.0 / 255.0);
    if let Ok(extents) = context.text_extents("Recording setup") {
        context.move_to(
            menu_x + 18.0 - extents.x_bearing(),
            menu_y + 27.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Recording setup").ok();
    }

    // Tabs
    let tabs = ["General", "Video", "GIF"];
    let tab_container_x = menu_x + 18.0;
    let tab_container_y = menu_y + 50.0;
    let tab_container_w = menu_w - 36.0;
    let tab_w = (tab_container_w - 8.0) / tabs.len() as f64;
    let tab_h = 30.0;
    let tab_start_x = tab_container_x + 4.0;
    let tab_y = tab_container_y + 4.0;
    super::rounded_rect_path(
        context,
        tab_container_x,
        tab_container_y,
        tab_container_w,
        38.0,
        9.0,
    );
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    context.fill_preserve().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
    context.set_line_width(1.0);
    context.stroke().ok();

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
                context.set_source_rgba(accent_r, accent_g, accent_b, 1.0);
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
            }
            super::rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 6.0);
            context.fill().ok();
        }
        let tab_text_color = if is_active_tab || tab_hovered {
            (1.0, 1.0, 1.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, 150.0 / 255.0)
        };
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            if is_active_tab || tab_hovered {
                gtk4::cairo::FontWeight::Bold
            } else {
                gtk4::cairo::FontWeight::Normal
            },
        );
        context.set_font_size(12.0);
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
            noise_suppression,
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
            hovered_dropdown_item,
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
    let label_x = menu_x + 32.0;
    let value_x = menu_x + menu_w - 50.0;
    let check_area_w = menu_w - 36.0;
    let desc_x = label_x + 110.0;
    let row_h = 44.0;
    let mut y = menu_y + 106.0;
    let mut idx = 3;

    super::rounded_rect_path(context, menu_x + 18.0, y, menu_w - 36.0, row_h * 6.0, 10.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 13.0 / 255.0);
    context.set_line_width(1.0);
    for i in 1..6 {
        let divider_y = y + row_h * i as f64;
        context.move_to(menu_x + 32.0, divider_y);
        context.line_to(menu_x + menu_w - 32.0, divider_y);
        context.stroke().ok();
    }

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
    }

    s!("Controls", "Keyboard shortcuts", rec_controls);
    s!("HiDPI", "Display scale resolution", hidpi);
    s!("Notifications", "Do Not Disturb", do_not_disturb);
    s!("Selection", "Remember last area", remember_selection);
    s!("Dim screen", "While recording", dim_screen);
    s!("Countdown", "Before recording", show_countdown);
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
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(12.0);
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
            context.move_to(
                label_x - extents.x_bearing(),
                y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(label).ok();
        }
    }
    if hover {
        super::rounded_rect_path(context, label_x - 14.0, y, _check_area_w, row_h, 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
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
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(11.0);
    context.set_source_rgba(
        1.0,
        1.0,
        1.0,
        if disabled {
            100.0 / 255.0
        } else {
            155.0 / 255.0
        },
    );
    if let Ok(extents) = context.text_extents(desc) {
        // Clip description to available width (252px like C++)
        let max_desc_w = value_x - desc_x - 10.0;
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
    noise_suppression: bool,
    open_editor: bool,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
) {
    let card_x = menu_x + 18.0;
    let card_y = menu_y + 106.0;
    let card_w = menu_w - 36.0;
    let row_heights = [76.0, 52.0, 52.0, 52.0, 76.0];
    let card_h: f64 = row_heights.iter().sum();
    let label_x = card_x + 14.0;
    let control_right = card_x + card_w - 14.0;

    super::rounded_rect_path(context, card_x, card_y, card_w, card_h, 10.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 13.0 / 255.0);
    context.set_line_width(1.0);
    let mut divider_y = card_y;
    for height in row_heights.iter().take(4) {
        divider_y += height;
        context.move_to(card_x + 14.0, divider_y);
        context.line_to(card_x + card_w - 14.0, divider_y);
        context.stroke().ok();
    }

    let draw_text =
        |context: &gtk4::cairo::Context, text: &str, x: f64, y: f64, bold: bool, alpha: f64| {
            context.select_font_face(
                crate::typography::UI_FONT_FAMILY,
                gtk4::cairo::FontSlant::Normal,
                if bold {
                    gtk4::cairo::FontWeight::Bold
                } else {
                    gtk4::cairo::FontWeight::Normal
                },
            );
            context.set_font_size(if bold { 13.0 } else { 11.5 });
            context.set_source_rgba(1.0, 1.0, 1.0, alpha);
            context.move_to(x, y);
            context.show_text(text).ok();
        };

    let row1 = card_y;
    if hovered_item == 3 {
        super::rounded_rect_path(context, card_x, row1, card_w, row_heights[0], 8.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_text(
        context,
        "Maximum resolution",
        label_x,
        row1 + 25.0,
        true,
        0.9,
    );
    draw_text(
        context,
        "Reduce file size and upload time",
        label_x,
        row1 + 48.0,
        false,
        0.55,
    );
    let res_options = ["Original", "1080p", "720p"];
    draw_dropdown_button(
        context,
        control_right - 136.0,
        row1 + 22.0,
        136.0,
        30.0,
        res_options[video_max_res],
        hovered_item == 3,
    );

    let row2 = row1 + row_heights[0];
    if hovered_item == 4 {
        super::rounded_rect_path(context, card_x, row2, card_w, row_heights[1], 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_text(context, "Frame rate", label_x, row2 + 31.0, true, 0.9);
    let fps_options = ["24", "30", "50", "60"];
    draw_dropdown_button(
        context,
        control_right - 76.0,
        row2 + 11.0,
        76.0,
        30.0,
        fps_options[video_fps],
        hovered_item == 4,
    );

    let row3 = row2 + row_heights[1];
    if hovered_item == 5 {
        super::rounded_rect_path(context, card_x, row3, card_w, row_heights[2], 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_text(
        context,
        "Record audio in mono",
        label_x,
        row3 + 31.0,
        true,
        0.9,
    );
    draw_checkbox(
        context,
        control_right - 18.0,
        row3 + 17.0,
        18.0,
        record_mono,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );

    let row4 = row3 + row_heights[2];
    if hovered_item == 6 {
        super::rounded_rect_path(context, card_x, row4, card_w, row_heights[3], 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_text(
        context,
        "Noise suppression",
        label_x,
        row4 + 31.0,
        true,
        0.9,
    );
    draw_text(
        context,
        "Clean up microphone noise",
        label_x + 160.0,
        row4 + 31.0,
        false,
        0.55,
    );
    draw_checkbox(
        context,
        control_right - 18.0,
        row4 + 17.0,
        18.0,
        noise_suppression,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );

    let row5 = row4 + row_heights[3];
    if hovered_item == 7 {
        super::rounded_rect_path(context, card_x, row5, card_w, row_heights[4], 8.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_text(
        context,
        "Open video editor",
        label_x,
        row5 + 27.0,
        true,
        0.9,
    );
    draw_text(
        context,
        "Edit quality, resolution and audio after recording",
        label_x,
        row5 + 50.0,
        false,
        0.55,
    );
    draw_checkbox(
        context,
        control_right - 18.0,
        row5 + 29.0,
        18.0,
        open_editor,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
}

pub(crate) fn draw_settings_gif_tab(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    menu_w: f64,
    hovered_item: i32,
    gif_fps: f64,
    gif_quality: f64,
    optimize_gif: bool,
    gif_size_idx: usize,
    _accent_r: f64,
    _accent_g: f64,
    _accent_b: f64,
) {
    let card_x = menu_x + 18.0;
    let card_y = menu_y + 106.0;
    let card_w = menu_w - 36.0;
    let row_heights = [64.0, 72.0, 52.0, 64.0];
    let card_h: f64 = row_heights.iter().sum();
    let label_x = card_x + 14.0;
    let control_right = card_x + card_w - 14.0;

    super::rounded_rect_path(context, card_x, card_y, card_w, card_h, 10.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 13.0 / 255.0);
    context.set_line_width(1.0);
    let mut divider_y = card_y;
    for height in row_heights.iter().take(3) {
        divider_y += height;
        context.move_to(card_x + 14.0, divider_y);
        context.line_to(card_x + card_w - 14.0, divider_y);
        context.stroke().ok();
    }

    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 200.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(label_x - extents.x_bearing(), y - extents.y_bearing());
            context.show_text(txt).ok();
        }
    };

    let row1 = card_y;
    if hovered_item == 3 {
        super::rounded_rect_path(context, card_x, row1, card_w, row_heights[0], 8.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_label(context, "Frame rate", row1 + 37.0);
    let fps_label = format!("{:.0}", gif_fps);
    context.set_source_rgba(1.0, 1.0, 1.0, 15.0 / 255.0);
    super::rounded_rect_path(context, menu_x + 140.0, row1 + 17.0, 45.0, 30.0, 6.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.3);
    if let Ok(extents) = context.text_extents(&fps_label) {
        context.move_to(
            menu_x + 162.5 - extents.width() / 2.0 - extents.x_bearing(),
            row1 + 32.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text(&fps_label).ok();
    }
    let slider_x = menu_x + 200.0;
    let slider_w = control_right - slider_x;
    let track_y = row1 + 30.0;
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
    context.arc(handle_x, track_y + 2.0, 7.0, 0.0, PI * 2.0);
    context.fill().ok();

    let row2 = row1 + row_heights[0];
    if hovered_item == 4 {
        super::rounded_rect_path(context, card_x, row2, card_w, row_heights[1], 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_label(context, "Quality", row2 + 29.0);
    let q_slider_x = menu_x + 160.0;
    let q_slider_w = control_right - q_slider_x;
    let q_track_y = row2 + 27.0;
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    super::rounded_rect_path(context, q_slider_x, q_track_y, q_slider_w, 4.0, 2.0);
    context.fill().ok();
    context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 1.0);
    super::rounded_rect_path(
        context,
        q_slider_x,
        q_track_y,
        q_slider_w * gif_quality,
        4.0,
        2.0,
    );
    context.fill().ok();
    // Ticks
    context.set_source_rgba(1.0, 1.0, 1.0, 60.0 / 255.0);
    context.set_line_width(1.0);
    for i in 0..=8 {
        let tx = q_slider_x + (q_slider_w / 8.0) * i as f64;
        context.move_to(tx, q_track_y - 4.0);
        context.line_to(tx, q_track_y + 8.0);
        context.stroke().ok();
    }
    // Quality handle
    let q_handle_x = q_slider_x + gif_quality * q_slider_w;
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.new_path();
    context.arc(q_handle_x, q_track_y + 2.0, 7.0, 0.0, PI * 2.0);
    context.fill().ok();
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    if let Ok(_ext) = context.text_extents("Low") {
        context.move_to(q_slider_x, row2 + 57.0);
        context.show_text("Low").ok();
    }
    if let Ok(_ext) = context.text_extents("High") {
        context.move_to(q_slider_x + q_slider_w - 22.0, row2 + 57.0);
        context.show_text("High").ok();
    }
    let row3 = row2 + row_heights[1];
    if hovered_item == 5 {
        super::rounded_rect_path(context, card_x, row3, card_w, row_heights[2], 6.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_label(context, "Optimize GIF", row3 + 32.0);
    draw_checkbox(
        context,
        control_right - 18.0,
        row3 + 17.0,
        18.0,
        optimize_gif,
        false,
        176.0 / 255.0,
        92.0 / 255.0,
        56.0 / 255.0,
    );
    let row4 = row3 + row_heights[2];
    if hovered_item == 6 {
        super::rounded_rect_path(context, card_x, row4, card_w, row_heights[3], 8.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
        context.fill().ok();
    }
    draw_label(context, "Output size", row4 + 38.0);
    let size_options = ["800 x auto", "640 x auto", "480 x auto", "Original"];
    draw_dropdown_button(
        context,
        control_right - 180.0,
        row4 + 17.0,
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
    hovered_item: i32,
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
    let popup_w = if matches!((tab, drop_idx), (SettingsTab::Gif, 6)) {
        180.0
    } else {
        160.0
    };
    let value_x = menu_x + 408.0 - popup_w;
    let popup_y = compute_dropdown_popup_y(menu_y, drop_idx, tab);
    let item_h = 30.0;
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
        if hovered_item == i as i32 {
            super::rounded_rect_path(
                context,
                r.x + 2.0,
                r.y + 2.0,
                r.width - 4.0,
                r.height - 2.0,
                5.0,
            );
            context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
            context.fill().ok();
        }
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
            context.set_source_rgba(accent_r, accent_g, accent_b, 28.0 / 255.0);
            context.fill().ok();
            let _ = context.restore();
        }
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
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
        if i == current_val {
            let cx = r.x + r.width - 16.0;
            let cy = r.y + item_h / 2.0;
            context.set_source_rgba(accent_r, accent_g, accent_b, 1.0);
            context.set_line_width(1.6);
            context.move_to(cx - 4.0, cy);
            context.line_to(cx - 1.0, cy + 3.0);
            context.line_to(cx + 5.0, cy - 4.0);
            context.stroke().ok();
        }
    }
}

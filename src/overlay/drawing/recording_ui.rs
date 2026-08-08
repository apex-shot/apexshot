use super::super::background::BackgroundFrame;
use super::super::icons::{draw_toolbar_icon, ToolbarIcon};
use super::super::layout::*;
use super::super::recording::layout::{
    compute_recording_deck_layout, RecordPanelTile, REC_ACTION_WIDTH,
};
use super::super::recording::state::SettingsTab;
use crate::capture_overlay::RecordingType;

pub(crate) fn draw_recording_panel(
    context: &gtk4::cairo::Context,
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    hover_tile: Option<RecordPanelTile>,
    selected_record_type: Option<RecordingType>,
    crop_menu_open: bool,
    record_aspect_ratio_index: usize,
    hovered_crop_menu_item: i32,
    settings_menu_open: bool,
    settings_tab: SettingsTab,
    hovered_settings_item: i32,
    settings_dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    mic_enabled: bool,
    speaker_enabled: bool,
    mic_level: f64,
    speaker_level: f64,
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
) {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    for panel in [
        deck.left_toggle_rail,
        deck.top_cluster,
        deck.bottom_action_bar,
    ] {
        super::draw_frosted_panel(
            context,
            panel.x,
            panel.y,
            panel.width,
            panel.height,
            10.0,
            screen_width,
            screen_height,
            background,
        );
    }

    let accent = |context: &gtk4::cairo::Context, rect: RectF, active: bool| {
        super::rounded_rect_path(
            context,
            rect.x + 3.0,
            rect.y + 3.0,
            rect.width - 6.0,
            rect.height - 6.0,
            9.0,
        );
        if active {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 0.34);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        let _ = context.fill();
    };

    let top = deck.top_cluster;
    let controls = RectF {
        x: top.x,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let size = RectF {
        x: controls.x + controls.width + ACTION_CARD_GAP,
        y: top.y,
        width: SIZE_CARD_WIDTH,
        height: top.height,
    };
    let crop = RectF {
        x: size.x + size.width + ACTION_CARD_GAP,
        y: top.y,
        width: CROP_CARD_WIDTH,
        height: top.height,
    };
    let rail = deck.left_toggle_rail;
    let rail_tiles = [
        (
            RecordPanelTile::Mic,
            ToolbarIcon::Mic,
            "Mic",
            mic_enabled,
            true,
        ),
        (
            RecordPanelTile::Speaker,
            ToolbarIcon::Speaker,
            "Speaker",
            speaker_enabled,
            false,
        ),
    ];

    if hover_tile == Some(RecordPanelTile::Controls) {
        accent(context, controls, false);
    }
    draw_toolbar_icon(
        context,
        ToolbarIcon::Controls,
        controls.x + controls.width / 2.0 + 0.6,
        controls.y + 28.8,
        (0.0, 0.0, 0.0, 0.42),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Controls,
        controls.x + controls.width / 2.0,
        controls.y + 28.0,
        (1.0, 1.0, 1.0, 0.96),
    );

    if hover_tile == Some(RecordPanelTile::Size) {
        accent(context, size, false);
    }
    super::draw_text_centered(
        context,
        RectF {
            x: size.x,
            y: size.y + 8.0,
            width: size.width,
            height: 12.0,
        },
        "FRAME",
        9.6,
        true,
        (1.0, 224.0 / 255.0, 196.0 / 255.0, 0.80),
    );
    super::draw_text_centered(
        context,
        RectF {
            x: size.x,
            y: size.y + 20.0,
            width: size.width,
            height: 20.0,
        },
        &format!("{}×{}", selection_width as i32, selection_height as i32),
        14.7,
        true,
        (0.96, 0.96, 0.97, 1.0),
    );

    if hover_tile == Some(RecordPanelTile::Crop) {
        accent(context, crop, false);
    }
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop.x + crop.width / 2.0 + 0.6,
        crop.y + 28.8,
        (0.0, 0.0, 0.0, 0.42),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop.x + crop.width / 2.0,
        crop.y + 28.0,
        (1.0, 1.0, 1.0, 0.96),
    );

    for (index, (tile, icon, label, active, warm_meter)) in rail_tiles.iter().enumerate() {
        let rect = RectF {
            x: rail.x,
            y: rail.y + FEATURE_PANEL_HEIGHT * index as f64,
            width: rail.width,
            height: FEATURE_PANEL_HEIGHT,
        };
        let hovered = hover_tile == Some(*tile);
        if hovered || *active {
            accent(context, rect, *active);
        }
        let color = if *active {
            (1.0, 229.0 / 255.0, 206.0 / 255.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, if hovered { 1.0 } else { 0.94 })
        };
        draw_toolbar_icon(
            context,
            *icon,
            rect.x + rect.width / 2.0 + 0.6,
            rect.y + 20.8,
            (0.0, 0.0, 0.0, 0.44),
        );
        draw_toolbar_icon(
            context,
            *icon,
            rect.x + rect.width / 2.0,
            rect.y + 20.0,
            color,
        );
        super::draw_text_centered(
            context,
            RectF {
                x: rect.x,
                y: rect.y + 38.0,
                width: rect.width,
                height: 18.0,
            },
            label,
            10.7,
            hovered || *active,
            color,
        );
        if matches!(*tile, RecordPanelTile::Mic | RecordPanelTile::Speaker) {
            let level = match tile {
                RecordPanelTile::Mic => mic_level,
                RecordPanelTile::Speaker => speaker_level,
                _ => 0.0,
            };
            draw_audio_meter_segments(
                context,
                rect,
                if *active { level } else { 0.0 },
                *warm_meter,
            );
        }
    }

    let actions = deck.bottom_action_bar;
    let video = RectF {
        x: actions.x,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    let gif = RectF {
        x: video.x + video.width + ACTION_CARD_GAP,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    for (rect, tile, icon, label, selected) in [
        (
            video,
            RecordPanelTile::RecordVideo,
            ToolbarIcon::Video,
            "Video",
            selected_record_type == Some(RecordingType::Video),
        ),
        (
            gif,
            RecordPanelTile::RecordGif,
            ToolbarIcon::Gif,
            "GIF",
            selected_record_type == Some(RecordingType::Gif),
        ),
    ] {
        let hovered = hover_tile == Some(tile);
        if hovered {
            let hr = RectF {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            };
            super::rounded_rect_path(context, hr.x, hr.y, hr.width, hr.height, 10.0);
            context.set_source_rgba(1.0, 1.0, 1.0, 0.09);
            let _ = context.fill();
        }
        let (path_x, path_y, path_w, path_h) = (
            rect.x + 3.0,
            rect.y + 3.0,
            rect.width - 6.0,
            rect.height - 6.0,
        );
        super::rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        if selected || hovered {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 88.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
        }
        let _ = context.fill();
        let _ = context.save();
        super::rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        context.clip();
        super::rounded_rect_path(
            context,
            rect.x + 3.8,
            rect.y + 3.8,
            rect.width - 7.6,
            rect.height - 7.6,
            8.4,
        );
        if selected || hovered {
            context.set_source_rgba(1.0, 212.0 / 255.0, 178.0 / 255.0, 152.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
        }
        context.set_line_width(1.1);
        let _ = context.stroke();
        let _ = context.restore();
        let icon_alpha = if hovered || selected { 1.0 } else { 0.94 };
        let shadow_alpha = if hovered {
            0.24
        } else if selected {
            0.32
        } else {
            0.50
        };
        let icon_y = rect.y + rect.height / 2.0 - if hovered { 0.5 } else { 0.0 };
        draw_toolbar_icon(
            context,
            icon,
            rect.x + 28.6,
            icon_y + 0.8,
            (0.0, 0.0, 0.0, shadow_alpha),
        );
        draw_toolbar_icon(
            context,
            icon,
            rect.x + 28.0,
            icon_y,
            (1.0, 1.0, 1.0, icon_alpha),
        );
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(15.7);
        context.set_source_rgba(0.0, 0.0, 0.0, shadow_alpha);
        context.move_to(rect.x + 50.6, rect.y + 30.8);
        let _ = context.show_text(label);
        if selected {
            context.set_source_rgba(1.0, 232.0 / 255.0, 214.0 / 255.0, icon_alpha);
        } else {
            context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, icon_alpha);
        }
        context.move_to(rect.x + 50.0, rect.y + 30.0);
        let _ = context.show_text(label);
    }

    // Recording crop menu dropdown
    if crop_menu_open {
        draw_recording_crop_menu(
            context,
            crop,
            hovered_crop_menu_item,
            record_aspect_ratio_index,
            screen_width,
            screen_height,
            background,
        );
    }

    // Settings menu (replaces panel content) — positioned like C++:
    // contextualX = clamp(selX + (selW - 440) / 2, 10, screenW - 450)
    // contextualY = clamp(selY + 24, 10, screenH - 570)
    if settings_menu_open {
        let panel_x =
            (selection_x + (selection_width - 440.0) / 2.0).clamp(10.0, screen_width - 450.0);
        let panel_y = (selection_y + 24.0).clamp(10.0, screen_height - 570.0);
        super::settings_ui::draw_settings_menu(
            context,
            panel_x,
            panel_y,
            screen_width,
            screen_height,
            background,
            settings_tab,
            hovered_settings_item,
            settings_dropdown_open,
            video_max_res,
            video_fps,
            record_mono,
            open_editor,
            rec_controls,
            hidpi,
            do_not_disturb,
            remember_selection,
            dim_screen,
            show_countdown,
            gif_fps,
            gif_quality,
            optimize_gif,
            gif_size_idx,
            176.0 / 255.0,
            92.0 / 255.0,
            56.0 / 255.0,
            (1.0, 214.0 / 255.0, 186.0 / 255.0),
        );
    }
}

fn draw_audio_meter_segments(context: &gtk4::cairo::Context, rect: RectF, level: f64, warm: bool) {
    let num_segments = 4;
    let segment_w = 10.0;
    let segment_h = 4.5;
    let spacing = 3.0;
    let total_w = num_segments as f64 * segment_w + (num_segments - 1) as f64 * spacing;
    let base_x = rect.x + rect.width / 2.0 - total_w / 2.0;
    let base_y = rect.y + rect.height - 17.0;
    let level = level.clamp(0.0, 1.0);

    super::rounded_rect_path(
        context,
        base_x - 4.0,
        base_y - 1.0,
        total_w + 8.0,
        segment_h + 2.0,
        5.0,
    );
    context.set_source_rgba(0.0, 0.0, 0.0, 24.0 / 255.0);
    let _ = context.fill();

    for b in 0..num_segments {
        let threshold = (b + 1) as f64 / num_segments as f64;
        let lit = level >= threshold - 0.18;
        let x = base_x + b as f64 * (segment_w + spacing);
        super::rounded_rect_path(context, x, base_y, segment_w, segment_h, 2.2);
        if lit {
            if warm {
                context.set_source_rgba(1.0, 134.0 / 255.0, 52.0 / 255.0, 0.92);
            } else {
                context.set_source_rgba(76.0 / 255.0, 154.0 / 255.0, 1.0, 0.92);
            }
            let _ = context.fill();
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 42.0 / 255.0);
            let _ = context.fill_preserve();
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
            context.set_line_width(0.9);
            let _ = context.stroke();
        }
    }
}

pub(crate) fn draw_recording_crop_menu(
    context: &gtk4::cairo::Context,
    crop_tile_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    super::draw_aspect_ratio_menu(
        context,
        crop_tile_rect,
        hovered_item,
        selected_index,
        screen_width,
        screen_height,
        background,
    )
}

pub(crate) fn draw_volume_popup(
    context: &gtk4::cairo::Context,
    panel_x: f64,
    panel_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    volume: f64,
    title: &str,
    dragging: bool,
) {
    let menu_w = 280.0;
    let menu_h = 130.0;
    let menu_x = panel_x.clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = panel_y.clamp(10.0, screen_height - menu_h - 10.0);

    let accent_r = 176.0 / 255.0;
    let accent_g = 92.0 / 255.0;
    let accent_b = 56.0 / 255.0;

    // Warm radial glow — same treatment as settings / click-options menus
    {
        let _ = context.save();
        let glow_cx = menu_x + menu_w / 2.0;
        let glow_cy = menu_y + menu_h / 2.0;
        let glow =
            gtk4::cairo::RadialGradient::new(glow_cx, glow_cy, 0.0, glow_cx, glow_cy, menu_w);
        glow.add_color_stop_rgba(0.0, accent_r, accent_g, accent_b, 40.0 / 255.0);
        glow.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.0);
        let _ = context.set_source(&glow);
        context.rectangle(menu_x - 40.0, menu_y - 40.0, menu_w + 80.0, menu_h + 80.0);
        let _ = context.fill();
        let _ = context.restore();
    }

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

    // Two-line header matching settings / click-options menus
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(10.7);
    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 176.0 / 255.0);
    if let Ok(_ext) = context.text_extents("RECORDING DEVICE") {
        context.move_to(menu_x + 18.0, menu_y + 28.0);
        context.show_text("RECORDING DEVICE").ok();
    }
    context.set_font_size(18.7);
    context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
    if let Ok(_ext) = context.text_extents(title) {
        context.move_to(menu_x + 18.0, menu_y + 48.0);
        context.show_text(title).ok();
    }

    // Slider row — compact single-row layout fitting 280px panel
    let row_y = menu_y + 78.0;
    let row_h = 46.0;

    // Label: left-aligned
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(13.3);
    context.set_source_rgba(1.0, 1.0, 1.0, 210.0 / 255.0);
    let label_text = "Volume:";
    let label_x = menu_x + 18.0;
    if let Ok(extents) = context.text_extents(label_text) {
        context.move_to(
            label_x,
            row_y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text(label_text).ok();
    }

    // Percentage badge: right-aligned at the panel edge
    let pct = (volume * 100.0).round() as i32;
    let pct_text = format!("{}%", pct);
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0);
    context.set_source_rgba(1.0, 232.0 / 255.0, 214.0 / 255.0, 220.0 / 255.0);
    if let Ok(extents) = context.text_extents(&pct_text) {
        let px = menu_x + menu_w - 12.0 - extents.width() - extents.x_bearing();
        let py = row_y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing();
        context.move_to(px, py);
        context.show_text(&pct_text).ok();
    }

    // Slider between label and percentage badge
    let slider_x = menu_x + 83.0;
    let slider_w = 140.0;
    let slider_track_h = 6.0;
    let track_y = row_y + (row_h - slider_track_h) / 2.0;

    // Track background
    context.set_source_rgba(1.0, 1.0, 1.0, if dragging { 36.0 } else { 28.0 } / 255.0);
    super::rounded_rect_path(context, slider_x, track_y, slider_w, slider_track_h, 3.0);
    let _ = context.fill();

    // Filled portion
    let filled_w = volume.clamp(0.0, 1.0) * slider_w;
    if filled_w > 1.0 {
        let _ = context.save();
        let fill_grad = gtk4::cairo::LinearGradient::new(slider_x, 0.0, slider_x + slider_w, 0.0);
        fill_grad.add_color_stop_rgba(
            0.0,
            204.0 / 255.0,
            122.0 / 255.0,
            80.0 / 255.0,
            235.0 / 255.0,
        );
        fill_grad.add_color_stop_rgba(1.0, 1.0, 178.0 / 255.0, 122.0 / 255.0, 235.0 / 255.0);
        let _ = context.set_source(&fill_grad);
        super::rounded_rect_path(context, slider_x, track_y, filled_w, slider_track_h, 3.0);
        let _ = context.fill();
        let _ = context.restore();
    }

    // Handle
    let handle_w = if dragging { 18.0 } else { 14.0 };
    let handle_h = 26.0;
    let handle_x = slider_x + filled_w - handle_w / 2.0;
    let handle_y = track_y + slider_track_h / 2.0 - handle_h / 2.0;
    context.set_source_rgba(0.0, 0.0, 0.0, 90.0 / 255.0);
    super::rounded_rect_path(
        context,
        handle_x + 0.6,
        handle_y + 1.4,
        handle_w,
        handle_h,
        6.0,
    );
    let _ = context.fill();
    let _ = context.save();
    let handle_grad = gtk4::cairo::LinearGradient::new(0.0, handle_y, 0.0, handle_y + handle_h);
    handle_grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 1.0);
    handle_grad.add_color_stop_rgba(1.0, 225.0 / 255.0, 225.0 / 255.0, 230.0 / 255.0, 1.0);
    let _ = context.set_source(&handle_grad);
    super::rounded_rect_path(context, handle_x, handle_y, handle_w, handle_h, 6.0);
    let _ = context.fill();
    let _ = context.restore();
}

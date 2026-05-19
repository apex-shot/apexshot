use super::background::{paint_surface_clipped, paint_surface_fullscreen, BackgroundFrame};
use super::geometry::current_selection_rect;
use super::icons::{draw_toolbar_icon, ToolbarIcon, TOOLBAR_ICONS, TOOLBAR_LABELS};
use super::layout::*;
use super::recording::layout::{
    compute_dropdown_popup_y, compute_recording_deck_layout, RecordPanelTile, REC_ACTION_WIDTH,
};
use super::recording::state::{OverlayIntent, SettingsTab};
use super::state::{OverlayMode, SelectorState};
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

pub(crate) fn draw_resize_markers(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let half = HANDLE_MARKER_LENGTH / 2.0;

    context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 0.96);
    context.set_line_width(HANDLE_MARKER_THICKNESS);
    context.set_line_cap(gtk4::cairo::LineCap::Round);

    // Corner L-markers
    context.move_to(x, y + half);
    context.line_to(x, y);
    context.line_to(x + half, y);

    context.move_to(x + width - half, y);
    context.line_to(x + width, y);
    context.line_to(x + width, y + half);

    context.move_to(x, y + height - half);
    context.line_to(x, y + height);
    context.line_to(x + half, y + height);

    context.move_to(x + width - half, y + height);
    context.line_to(x + width, y + height);
    context.line_to(x + width, y + height - half);

    // Mid-edge line markers
    context.move_to(x + width / 2.0 - half, y);
    context.line_to(x + width / 2.0 + half, y);

    context.move_to(x + width / 2.0 - half, y + height);
    context.line_to(x + width / 2.0 + half, y + height);

    context.move_to(x, y + height / 2.0 - half);
    context.line_to(x, y + height / 2.0 + half);

    context.move_to(x + width, y + height / 2.0 - half);
    context.line_to(x + width, y + height / 2.0 + half);

    let _ = context.stroke();
}

pub(crate) fn rounded_rect_path(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let r = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(x + width - r, y + r, r, -PI / 2.0, 0.0);
    context.arc(x + width - r, y + height - r, r, 0.0, PI / 2.0);
    context.arc(x + r, y + height - r, r, PI / 2.0, PI);
    context.arc(x + r, y + r, r, PI, PI * 1.5);
    context.close_path();
}

pub(crate) fn draw_feature_toolbar(
    context: &gtk4::cairo::Context,
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    active_tool_index: usize,
    hover_tool_index: Option<usize>,
    hover_size_panel: bool,
    hover_crop_panel: bool,
    capture_crop_menu_open: bool,
    capture_aspect_ratio_index: usize,
    hovered_capture_crop_menu_item: i32,
) {
    let layout = compute_toolbar_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );

    let size_panel_x = layout.size_panel.x;
    let size_panel_y = layout.size_panel.y;
    let size_panel_width = layout.size_panel.width;
    let crop_panel = layout.crop_panel;
    let active_tool_index = active_tool_index.min(TOOLBAR_ICONS.len().saturating_sub(1));
    let crop_active = capture_crop_menu_open || capture_aspect_ratio_index > 0;

    draw_frosted_panel(
        context,
        layout.tools_panel.x,
        layout.tools_panel.y,
        layout.tools_panel.width,
        layout.tools_panel.height,
        FEATURE_PANEL_RADIUS,
        screen_width,
        screen_height,
        background,
    );

    // Single combined panel for size + crop (matches C++ topCluster)
    let top_cluster_x = layout.size_panel.x;
    let top_cluster_y = layout.size_panel.y;
    let top_cluster_w = layout.size_panel.width + ACTION_CARD_GAP + layout.crop_panel.width;
    let top_cluster_h = layout.size_panel.height;
    draw_frosted_panel(
        context,
        top_cluster_x,
        top_cluster_y,
        top_cluster_w,
        top_cluster_h,
        FEATURE_PANEL_RADIUS,
        screen_width,
        screen_height,
        background,
    );

    let draw_accent = |context: &gtk4::cairo::Context, rect: RectF, active: bool| {
        rounded_rect_path(
            context,
            rect.x + 4.0,
            rect.y + 4.0,
            rect.width - 8.0,
            rect.height - 8.0,
            10.0,
        );
        let alpha = if active { 76.0 / 255.0 } else { 22.0 / 255.0 };
        let (r, g, b) = if active {
            (176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0)
        } else {
            (1.0, 1.0, 1.0)
        };
        context.set_source_rgba(r, g, b, alpha);
        let _ = context.fill();
    };

    draw_accent(context, layout.item_cells[active_tool_index], true);
    if let Some(index) = hover_tool_index {
        if let Some(cell) = layout.item_cells.get(index) {
            if index != active_tool_index {
                draw_accent(context, *cell, false);
            }
        }
    }
    if hover_size_panel {
        draw_accent(context, layout.size_panel, false);
    }
    if hover_crop_panel || crop_active {
        draw_accent(context, crop_panel, crop_active);
    }

    // Icons + labels
    for (index, icon) in TOOLBAR_ICONS.iter().enumerate() {
        let cell = layout.item_cells[index];
        let center_x = cell.x + cell.width / 2.0;
        let label = TOOLBAR_LABELS[index];
        let is_hovered = hover_tool_index == Some(index);
        let is_active = index == active_tool_index;

        // Icon: brighter + reduced shadow on hover
        let icon_alpha = if is_hovered || is_active { 1.0 } else { 0.94 };
        let shadow_alpha = if is_hovered {
            0.24
        } else if is_active {
            0.32
        } else {
            0.50
        };
        let icon_y = if is_hovered || is_active {
            cell.y + 23.5
        } else {
            cell.y + 24.0
        };
        draw_toolbar_icon(
            context,
            *icon,
            center_x + 0.6,
            icon_y + 0.8,
            (0.0, 0.0, 0.0, shadow_alpha),
        );
        draw_toolbar_icon(
            context,
            *icon,
            center_x,
            icon_y,
            if is_active {
                (1.0, 229.0 / 255.0, 206.0 / 255.0, icon_alpha)
            } else {
                (244.0 / 255.0, 244.0 / 255.0, 244.0 / 255.0, icon_alpha)
            },
        );

        // Label: bold + brighter on hover
        let font_weight = if is_hovered || is_active {
            gtk4::cairo::FontWeight::Bold
        } else {
            gtk4::cairo::FontWeight::Normal
        };
        context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, font_weight);
        context.set_font_size(9.5);
        context.set_source_rgba(0.0, 0.0, 0.0, shadow_alpha);
        if let Ok(extents) = context.text_extents(label) {
            let text_x = center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
            let text_y = cell.y + 50.0 + 0.8;
            context.move_to(text_x, text_y);
            let _ = context.show_text(label);
        }

        if is_active {
            context.set_source_rgba(1.0, 229.0 / 255.0, 206.0 / 255.0, icon_alpha);
        } else {
            context.set_source_rgba(244.0 / 255.0, 244.0 / 255.0, 244.0 / 255.0, icon_alpha);
        }
        if let Ok(extents) = context.text_extents(label) {
            let text_x = center_x - extents.width() / 2.0 - extents.x_bearing();
            let text_y = cell.y + 50.0;
            context.move_to(text_x, text_y);
            let _ = context.show_text(label);
        }
    }

    let size_text = format!("{}×{}", selection_width as i32, selection_height as i32);
    let size_center_x = size_panel_x + size_panel_width / 2.0;

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(9.6);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.50);
    if let Ok(extents) = context.text_extents("FRAME") {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
        let text_y = size_panel_y + 17.0 + 0.8;
        context.move_to(text_x, text_y);
        let _ = context.show_text("FRAME");
    }

    context.set_source_rgba(1.0, 224.0 / 255.0, 196.0 / 255.0, 0.84);
    if let Ok(extents) = context.text_extents("FRAME") {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing();
        let text_y = size_panel_y + 17.0;
        context.move_to(text_x, text_y);
        let _ = context.show_text("FRAME");
    }

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(12.5);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    if let Ok(extents) = context.text_extents(&size_text) {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing() + 0.6;
        let text_y = size_panel_y + 39.0 + 0.8;
        context.move_to(text_x, text_y);
        let _ = context.show_text(&size_text);
    }

    context.set_source_rgba(1.0, 1.0, 1.0, 0.98);
    if let Ok(extents) = context.text_extents(&size_text) {
        let text_x = size_center_x - extents.width() / 2.0 - extents.x_bearing();
        let text_y = size_panel_y + 39.0;
        context.move_to(text_x, text_y);
        let _ = context.show_text(&size_text);
    }

    let crop_center_x = crop_panel.x + crop_panel.width / 2.0;
    let crop_y = crop_panel.y
        + if hover_crop_panel || crop_active {
            27.0
        } else {
            27.5
        };
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop_center_x + 0.6,
        crop_y + 0.8,
        (
            0.0,
            0.0,
            0.0,
            if hover_crop_panel {
                62.0 / 255.0
            } else {
                118.0 / 255.0
            },
        ),
    );
    draw_toolbar_icon(
        context,
        ToolbarIcon::Crop,
        crop_center_x,
        crop_y,
        if crop_active {
            (1.0, 229.0 / 255.0, 206.0 / 255.0, 0.95)
        } else {
            (1.0, 1.0, 1.0, 242.0 / 255.0)
        },
    );

    if capture_crop_menu_open {
        draw_capture_crop_menu(
            context,
            crop_panel,
            hovered_capture_crop_menu_item,
            capture_aspect_ratio_index,
            screen_width,
            screen_height,
            background,
        );
    }
}

pub(crate) fn draw_frosted_panel(
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
    // Drop shadow
    rounded_rect_path(context, x, y + 3.0, width, height, radius);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.30);
    let _ = context.fill();

    // Frosted-glass fill: clip to the panel shape, then layer:
    //   1. Real blurred screenshot (scaled from the pre-blurred 1/4-res surface)
    //   2. Dark tint  — ensures readability on any background (incl. white)
    //   3. Subtle white highlight — gives the "glass" sheen
    let _ = context.save();
    rounded_rect_path(context, x, y, width, height, radius);
    context.clip();

    if let Some(background) = background {
        // Scale the blurred surface so it maps to screen coordinates.
        // The blur surface is 1/4 the original image size, so we scale by
        // (screen / blur_surface_size) to fill the screen, then the clip
        // reveals only the portion behind this panel.
        let blur_w = background.toolbar_blur_surface.width().max(1) as f64;
        let blur_h = background.toolbar_blur_surface.height().max(1) as f64;
        let scale_x = screen_width / blur_w;
        let scale_y = screen_height / blur_h;

        let _ = context.save();
        context.scale(scale_x, scale_y);
        if context
            .set_source_surface(&background.toolbar_blur_surface, 0.0, 0.0)
            .is_ok()
        {
            let _ = context.paint();
        }
        let _ = context.restore();

        // Dark glass tint matching editor root background (#141414 at ~90% opacity)
        context.set_source_rgba(20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0, 230.0 / 255.0);
        let _ = context.paint();
    } else {
        // No screenshot (X11 transparent overlay): solid dark base.
        context.set_source_rgba(0.08, 0.08, 0.08, 1.0);
        let _ = context.paint();
    }

    // Subtle white sheen (0.04 alpha) for a polished feel
    context.set_source_rgba(1.0, 1.0, 1.0, 10.0 / 255.0);
    let _ = context.paint();

    // Panel border (matching editor's .editor-root border: 1px solid rgba(255, 255, 255, 0.10))
    // Drawn inside clip so the outer half is clipped away (C++ behavior)
    context.set_source_rgba(1.0, 1.0, 1.0, 0.10);
    context.set_line_width(1.0);
    context.set_antialias(gtk4::cairo::Antialias::Default);
    rounded_rect_path(context, x, y, width, height, radius);
    let _ = context.stroke();
    let _ = context.restore();
}

pub(crate) fn draw_text_centered(
    context: &gtk4::cairo::Context,
    rect: RectF,
    text: &str,
    size: f64,
    bold: bool,
    rgba: (f64, f64, f64, f64),
) {
    let weight = if bold {
        gtk4::cairo::FontWeight::Bold
    } else {
        gtk4::cairo::FontWeight::Normal
    };
    context.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, weight);
    context.set_font_size(size);
    context.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    if let Ok(extents) = context.text_extents(text) {
        let x = rect.x + rect.width / 2.0 - extents.width() / 2.0 - extents.x_bearing();
        let y = rect.y + rect.height / 2.0 - extents.height() / 2.0 - extents.y_bearing();
        context.move_to(x, y);
        let _ = context.show_text(text);
    }
}

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
    crop_menu_open: bool,
    record_aspect_ratio_index: usize,
    hovered_crop_menu_item: i32,
    settings_menu_open: bool,
    settings_tab: SettingsTab,
    hovered_settings_item: i32,
    settings_dropdown_open: Option<usize>,
    video_max_res: usize,
    video_fps: usize,
    record_mono: bool,
    open_editor: bool,
    rec_controls: bool,
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    rec_webcam: bool,
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
        draw_frosted_panel(
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
        rounded_rect_path(
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
        (RecordPanelTile::Mic, ToolbarIcon::Mic, "Mic", true),
        (
            RecordPanelTile::Speaker,
            ToolbarIcon::Speaker,
            "Speaker",
            false,
        ),
        (
            RecordPanelTile::Webcam,
            ToolbarIcon::Webcam,
            "Cam",
            rec_webcam,
        ),
        (
            RecordPanelTile::Clicks,
            ToolbarIcon::Clicks,
            "Clicks",
            rec_clicks,
        ),
        (
            RecordPanelTile::Keystrokes,
            ToolbarIcon::Keystrokes,
            "Keys",
            rec_keystrokes,
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
    draw_text_centered(
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
    draw_text_centered(
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

    for (index, (tile, icon, label, active)) in rail_tiles.iter().enumerate() {
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
        draw_text_centered(
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
    for (rect, tile, icon, label, primary) in [
        (
            video,
            RecordPanelTile::RecordVideo,
            ToolbarIcon::Video,
            "Video",
            true,
        ),
        (
            gif,
            RecordPanelTile::RecordGif,
            ToolbarIcon::Gif,
            "GIF",
            false,
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
            rounded_rect_path(context, hr.x, hr.y, hr.width, hr.height, 10.0);
            context.set_source_rgba(1.0, 1.0, 1.0, 0.09);
            let _ = context.fill();
        }
        let (path_x, path_y, path_w, path_h) = (
            rect.x + 3.0,
            rect.y + 3.0,
            rect.width - 6.0,
            rect.height - 6.0,
        );
        rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        if primary || hovered {
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 88.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
        }
        let _ = context.fill();
        let _ = context.save();
        rounded_rect_path(context, path_x, path_y, path_w, path_h, 9.0);
        context.clip();
        rounded_rect_path(
            context,
            rect.x + 3.8,
            rect.y + 3.8,
            rect.width - 7.6,
            rect.height - 7.6,
            8.4,
        );
        if primary || hovered {
            context.set_source_rgba(1.0, 212.0 / 255.0, 178.0 / 255.0, 152.0 / 255.0);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
        }
        context.set_line_width(1.1);
        let _ = context.stroke();
        let _ = context.restore();
        let icon_alpha = if hovered || primary { 1.0 } else { 0.94 };
        let shadow_alpha = if hovered {
            0.24
        } else if primary {
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
        if primary {
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
        draw_settings_menu(
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
            display_rec_time,
            hidpi,
            do_not_disturb,
            show_cursor,
            rec_clicks,
            rec_keystrokes,
            rec_webcam,
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

pub(crate) fn draw_aspect_ratio_menu(
    context: &gtk4::cairo::Context,
    anchor_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    let item_h = 34.0;
    let menu_w = 196.0;
    let menu_h = (ASPECT_RATIO_OPTIONS.len() as f64 * item_h) + 10.0;
    let menu_x = (anchor_rect.x + anchor_rect.width / 2.0 - menu_w / 2.0)
        .clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y =
        (anchor_rect.y + anchor_rect.height + 8.0).clamp(10.0, screen_height - menu_h - 10.0);

    draw_frosted_panel(
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

    let mut item_rects = Vec::with_capacity(ASPECT_RATIO_OPTIONS.len());
    for i in 0..ASPECT_RATIO_OPTIONS.len() {
        let item_rect = RectF {
            x: menu_x + 5.0,
            y: menu_y + 5.0 + i as f64 * item_h,
            width: menu_w - 10.0,
            height: item_h,
        };
        let indicator_x = item_rect.x + 8.0;

        if i as i32 == hovered_item {
            rounded_rect_path(
                context,
                item_rect.x,
                item_rect.y,
                item_rect.width,
                item_rect.height,
                7.0,
            );
            context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
            let _ = context.fill();
        }

        let selected = i == selected_index;
        if selected {
            rounded_rect_path(
                context,
                item_rect.x + 1.0,
                item_rect.y + 1.0,
                item_rect.width - 2.0,
                item_rect.height - 2.0,
                7.0,
            );
            context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 94.0 / 255.0);
            let _ = context.fill();
            context.set_source_rgba(1.0, 238.0 / 255.0, 224.0 / 255.0, 1.0);
            context.set_line_width(1.5);
            let cy = item_rect.y + item_rect.height / 2.0;
            context.move_to(indicator_x + 3.5, cy);
            context.line_to(indicator_x + 6.5, cy + 3.0);
            context.line_to(indicator_x + 12.5, cy - 4.0);
            context.stroke().ok();
        }

        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            if selected {
                gtk4::cairo::FontWeight::Bold
            } else {
                gtk4::cairo::FontWeight::Normal
            },
        );
        context.set_font_size(13.3);
        let label = ASPECT_RATIO_OPTIONS[i];
        if let Ok(extents) = context.text_extents(label) {
            let label_x = item_rect.x + 30.0 - extents.x_bearing();
            let label_y =
                item_rect.y + item_rect.height / 2.0 - extents.height() / 2.0 - extents.y_bearing();
            let label_color = if selected {
                (1.0, 240.0 / 255.0, 226.0 / 255.0, 1.0)
            } else {
                (242.0 / 255.0, 242.0 / 255.0, 244.0 / 255.0, 1.0)
            };
            context.set_source_rgba(label_color.0, label_color.1, label_color.2, label_color.3);
            context.move_to(label_x, label_y);
            let _ = context.show_text(label);
        }

        item_rects.push(item_rect);
    }
    item_rects
}

pub(crate) fn draw_capture_crop_menu(
    context: &gtk4::cairo::Context,
    crop_card_rect: RectF,
    hovered_item: i32,
    selected_index: usize,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
) -> Vec<RectF> {
    draw_aspect_ratio_menu(
        context,
        crop_card_rect,
        hovered_item,
        selected_index,
        screen_width,
        screen_height,
        background,
    )
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
    draw_aspect_ratio_menu(
        context,
        crop_tile_rect,
        hovered_item,
        selected_index,
        screen_width,
        screen_height,
        background,
    )
}

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
    rounded_rect_path(context, x, y, size, size, 4.0);
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
        context.fill().ok();
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
    rounded_rect_path(context, x, y, w, h, 6.0);
    if hovered {
        context.set_source_rgba(1.0, 1.0, 1.0, 20.0 / 255.0);
    }
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.0);
    rounded_rect_path(context, x, y, w, h, 6.0);
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
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    rec_webcam: bool,
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

    draw_frosted_panel(
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
            rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
            context.fill().ok();
            if is_active_tab {
                let _ = context.save();
                rounded_rect_path(context, tr.x, tr.y, tr.width, tr.height, 9.0);
                context.clip();
                rounded_rect_path(
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
            display_rec_time,
            hidpi,
            do_not_disturb,
            show_cursor,
            rec_clicks,
            rec_keystrokes,
            rec_webcam,
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
    display_rec_time: bool,
    hidpi: bool,
    do_not_disturb: bool,
    show_cursor: bool,
    rec_clicks: bool,
    rec_keystrokes: bool,
    _rec_webcam: bool,
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
    s!("Menu bar", "Display time in top bar", display_rec_time);
    s!("HiDPI", "Record at display scale res", hidpi);
    s!("Notifications", "DND while recording", do_not_disturb);
    s!("Cursor", "Show cursor", show_cursor, 10.0);
    s!("", "Highlight clicks", rec_clicks);
    s!("Keyboard", "Show keystrokes", rec_keystrokes, 10.0);
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
        rounded_rect_path(context, value_x - 5.0, y, 290.0, row_h, 6.0);
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
        rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
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
        rounded_rect_path(context, r.x - 5.0, r.y, r.width + 10.0, r.height, 6.0);
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
    rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
    context.fill().ok();
    context.set_source_rgba(1.0, 1.0, 1.0, 28.0 / 255.0);
    context.set_line_width(1.0);
    rounded_rect_path(context, value_x, curr_y, 45.0, 30.0, 6.0);
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
    rounded_rect_path(context, slider_x, track_y, slider_w, 4.0, 2.0);
    context.fill().ok();
    context.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 1.0);
    rounded_rect_path(context, slider_x, track_y, slider_w * progress, 4.0, 2.0);
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
    rounded_rect_path(context, value_x, q_track_y, q_slider_w, 4.0, 2.0);
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
    rounded_rect_path(
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
    curr_y += 60.0;

    // Optimize checkbox
    draw_checkbox(
        context,
        value_x,
        curr_y + (30.0 - 18.0) / 2.0,
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
            value_x + 25.0 - extents.x_bearing(),
            curr_y + 15.0 - extents.height() / 2.0 - extents.y_bearing(),
        );
        context.show_text("Optimize GIFs").ok();
    }
    curr_y += 45.0;

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
    draw_frosted_panel(
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
            rounded_rect_path(
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

static CLICK_COLORS: &[(f64, f64, f64)] = &[
    (0.71, 0.71, 0.71), // Gray
    (0.48, 0.39, 1.0),  // Indigo
    (1.0, 0.24, 0.24),  // Red
    (0.24, 0.47, 1.0),  // Blue
    (0.24, 0.78, 0.31), // Green
    (1.0, 0.82, 0.20),  // Yellow
    (1.0, 0.59, 0.12),  // Orange
    (0.71, 0.24, 0.86), // Purple
    (1.0, 1.0, 1.0),    // White
];

static CLICK_COLOR_NAMES: &[&str] = &[
    "Gray", "Indigo", "Red", "Blue", "Green", "Yellow", "Orange", "Purple", "White",
];

pub(crate) const CLICK_COLORS_LEN: usize = 9;

pub(crate) fn draw_click_options(
    context: &gtk4::cairo::Context,
    panel_x: f64,
    panel_y: f64,
    screen_width: f64,
    screen_height: f64,
    background: Option<&BackgroundFrame>,
    hovered_item: i32,
    click_size: f64,
    click_color: usize,
    click_style: usize,
    click_animate: bool,
) {
    let menu_w = 440.0;
    let menu_h = 500.0;
    let menu_x = panel_x.clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = panel_y.clamp(10.0, screen_height - menu_h - 10.0);

    let accent_r = 176.0 / 255.0;
    let accent_g = 92.0 / 255.0;
    let accent_b = 56.0 / 255.0;
    let accent_rim = (1.0, 214.0 / 255.0, 186.0 / 255.0);

    let glow_cx = menu_x + menu_w / 2.0;
    let glow_cy = menu_y + menu_h / 2.0;
    let glow = gtk4::cairo::RadialGradient::new(glow_cx, glow_cy, 0.0, glow_cx, glow_cy, menu_w);
    glow.add_color_stop_rgba(0.0, accent_r, accent_g, accent_b, 42.0 / 255.0);
    glow.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.0);
    let _ = context.save();
    let _ = context.set_source(&glow);
    context.rectangle(menu_x - 40.0, menu_y - 40.0, menu_w + 80.0, menu_h + 80.0);
    let _ = context.fill();
    let _ = context.restore();

    draw_frosted_panel(
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
    if let Ok(_ext) = context.text_extents("CLICK HIGHLIGHTS") {
        context.move_to(menu_x + 18.0, menu_y + 28.0);
        context.show_text("CLICK HIGHLIGHTS").ok();
    }
    context.set_font_size(18.7);
    context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
    if let Ok(_ext) = context.text_extents("Click Overlay") {
        context.move_to(menu_x + 18.0, menu_y + 48.0);
        context.show_text("Click Overlay").ok();
    }

    let label_x = menu_x + 25.0;
    let value_x = menu_x + 130.0;
    let control_w = 280.0;
    let row_h = 46.0;
    let mut curr_y = menu_y + 78.0;

    let style_names = ["Outline", "Filled"];
    let style_name = style_names[click_style.min(1)];

    let click_color_rgb = CLICK_COLORS[click_color.min(CLICK_COLORS_LEN - 1)];

    // ── Helper: draw label ──
    let draw_label = |context: &gtk4::cairo::Context, txt: &str, y: f64| {
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 210.0 / 255.0);
        if let Ok(extents) = context.text_extents(txt) {
            context.move_to(
                label_x + 90.0 - extents.width() - extents.x_bearing(),
                y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(txt).ok();
        }
    };

    // ── Helper: draw chevron ──
    let draw_chevron = |context: &gtk4::cairo::Context, cx: f64, cy: f64| {
        context.new_path();
        context.move_to(cx - 4.0, cy - 2.0);
        context.line_to(cx + 4.0, cy - 2.0);
        context.line_to(cx, cy + 3.0);
        context.close_path();
        context.set_source_rgba(1.0, 1.0, 1.0, 215.0 / 255.0);
        let _ = context.fill();
    };

    // ── 1. Size slider ──
    {
        draw_label(context, "Size:", curr_y);
        let slider_x = value_x;
        let slider_w = control_w;
        let slider_track_h = 6.0;
        let track_y = curr_y + (row_h - slider_track_h) / 2.0;

        // Track background
        context.set_source_rgba(
            1.0,
            1.0,
            1.0,
            if hovered_item == 0 { 36.0 } else { 28.0 } / 255.0,
        );
        rounded_rect_path(context, slider_x, track_y, slider_w, slider_track_h, 3.0);
        let _ = context.fill();

        // Filled portion
        let filled_w = click_size.clamp(0.0, 1.0) * slider_w;
        if filled_w > 1.0 {
            let _ = context.save();
            let fill_grad =
                gtk4::cairo::LinearGradient::new(slider_x, 0.0, slider_x + slider_w, 0.0);
            fill_grad.add_color_stop_rgba(
                0.0,
                204.0 / 255.0,
                122.0 / 255.0,
                80.0 / 255.0,
                235.0 / 255.0,
            );
            fill_grad.add_color_stop_rgba(1.0, 1.0, 178.0 / 255.0, 122.0 / 255.0, 235.0 / 255.0);
            let _ = context.set_source(&fill_grad);
            rounded_rect_path(context, slider_x, track_y, filled_w, slider_track_h, 3.0);
            let _ = context.fill();
            let _ = context.restore();
        }

        // Preview dot
        let preview_r = 4.0 + click_size * 10.0;
        let preview_cx = slider_x + filled_w;
        let preview_cy = track_y + slider_track_h / 2.0;
        context.set_source_rgba(click_color_rgb.0, click_color_rgb.1, click_color_rgb.2, 1.0);
        context.new_path();
        context.arc(preview_cx, preview_cy, preview_r, 0.0, PI * 2.0);
        let _ = context.fill();
        context.set_source_rgba(0.0, 0.0, 0.0, 90.0 / 255.0);
        context.set_line_width(1.0);
        context.new_path();
        context.arc(preview_cx, preview_cy, preview_r, 0.0, PI * 2.0);
        let _ = context.stroke();

        // Slider handle
        let handle_w = if hovered_item == 0 { 18.0 } else { 14.0 };
        let handle_h = 26.0;
        let handle_x = slider_x + filled_w - handle_w / 2.0;
        let handle_y = curr_y + (row_h - handle_h) / 2.0;
        context.set_source_rgba(0.0, 0.0, 0.0, 90.0 / 255.0);
        rounded_rect_path(
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
        rounded_rect_path(context, handle_x, handle_y, handle_w, handle_h, 6.0);
        let _ = context.fill();
        let _ = context.restore();

        // Percentage badge
        let pct = (click_size * 100.0).round() as i32;
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(11.0);
        context.set_source_rgba(1.0, 232.0 / 255.0, 214.0 / 255.0, 220.0 / 255.0);
        let pct_text = format!("{}%", pct);
        if let Ok(extents) = context.text_extents(&pct_text) {
            context.move_to(
                slider_x + slider_w - extents.width() - extents.x_bearing(),
                curr_y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(&pct_text).ok();
        }
    }
    curr_y += row_h;

    // ── 2. Color dropdown ──
    {
        draw_label(context, "Color:", curr_y);
        let color_btn_x = value_x;
        let color_btn_y = curr_y + (row_h - 32.0) / 2.0;
        let color_btn_w = 168.0;
        let color_btn_h = 32.0;
        let hovered = hovered_item == 1;

        // Dropdown button bg
        let _ = context.save();
        let bg_grad =
            gtk4::cairo::LinearGradient::new(0.0, color_btn_y, 0.0, color_btn_y + color_btn_h);
        bg_grad.add_color_stop_rgba(
            0.0,
            1.0,
            1.0,
            1.0,
            if hovered { 32.0 } else { 20.0 } / 255.0,
        );
        bg_grad.add_color_stop_rgba(
            1.0,
            0.0,
            0.0,
            0.0,
            if hovered { 70.0 } else { 92.0 } / 255.0,
        );
        let _ = context.set_source(&bg_grad);
        context.set_line_width(1.0);
        context.set_source_rgba(1.0, 1.0, 1.0, if hovered { 70.0 } else { 38.0 } / 255.0);
        rounded_rect_path(
            context,
            color_btn_x,
            color_btn_y,
            color_btn_w,
            color_btn_h,
            8.0,
        );
        let _ = context.stroke_preserve();
        let _ = context.set_source(&bg_grad);
        let _ = context.fill();
        let _ = context.restore();

        // Color swatch
        let sc_x = color_btn_x + 16.0;
        let sc_y = color_btn_y + color_btn_h / 2.0;
        context.set_source_rgba(0.0, 0.0, 0.0, 110.0 / 255.0);
        context.set_line_width(1.0);
        context.new_path();
        context.arc(sc_x, sc_y, 7.5, 0.0, PI * 2.0);
        let _ = context.stroke();
        context.set_source_rgba(click_color_rgb.0, click_color_rgb.1, click_color_rgb.2, 1.0);
        context.new_path();
        context.arc(sc_x, sc_y, 7.5, 0.0, PI * 2.0);
        let _ = context.fill();
        // Inner highlight
        context.set_source_rgba(1.0, 1.0, 1.0, 60.0 / 255.0);
        context.new_path();
        context.arc(sc_x - 1.6, sc_y - 1.6, 2.6, 0.0, PI * 2.0);
        let _ = context.fill();

        // Color name text
        let color_name = CLICK_COLOR_NAMES[click_color.min(CLICK_COLORS_LEN - 1)];
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
        if let Ok(extents) = context.text_extents(color_name) {
            context.move_to(
                color_btn_x + 32.0 - extents.x_bearing(),
                color_btn_y + color_btn_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(color_name).ok();
        }

        // Chevron
        draw_chevron(
            context,
            color_btn_x + color_btn_w - 13.0,
            color_btn_y + color_btn_h / 2.0 + 1.0,
        );
    }
    curr_y += row_h;

    // ── 3. Style dropdown ──
    {
        draw_label(context, "Style:", curr_y);
        let style_btn_x = value_x;
        let style_btn_y = curr_y + (row_h - 32.0) / 2.0;
        let style_btn_w = 110.0;
        let style_btn_h = 32.0;
        let hovered = hovered_item == 2;

        let _ = context.save();
        let bg_grad =
            gtk4::cairo::LinearGradient::new(0.0, style_btn_y, 0.0, style_btn_y + style_btn_h);
        bg_grad.add_color_stop_rgba(
            0.0,
            1.0,
            1.0,
            1.0,
            if hovered { 32.0 } else { 20.0 } / 255.0,
        );
        bg_grad.add_color_stop_rgba(
            1.0,
            0.0,
            0.0,
            0.0,
            if hovered { 70.0 } else { 92.0 } / 255.0,
        );
        let _ = context.set_source(&bg_grad);
        context.set_line_width(1.0);
        context.set_source_rgba(1.0, 1.0, 1.0, if hovered { 70.0 } else { 38.0 } / 255.0);
        rounded_rect_path(
            context,
            style_btn_x,
            style_btn_y,
            style_btn_w,
            style_btn_h,
            8.0,
        );
        let _ = context.stroke_preserve();
        let _ = context.set_source(&bg_grad);
        let _ = context.fill();
        let _ = context.restore();

        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
        if let Ok(extents) = context.text_extents(style_name) {
            context.move_to(
                style_btn_x + 14.0 - extents.x_bearing(),
                style_btn_y + style_btn_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(style_name).ok();
        }
        draw_chevron(
            context,
            style_btn_x + style_btn_w - 13.0,
            style_btn_y + style_btn_h / 2.0 + 1.0,
        );
    }
    curr_y += row_h;

    // ── 4. Animation toggle ──
    {
        draw_label(context, "Animation:", curr_y);
        let anim_x = value_x;
        let anim_w = control_w;
        let hovered = hovered_item == 3;
        if hovered {
            rounded_rect_path(
                context,
                anim_x - 4.0,
                curr_y + 4.0,
                anim_w + 8.0,
                row_h - 8.0,
                8.0,
            );
            context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
            let _ = context.fill();
        }
        draw_checkbox(
            context,
            anim_x,
            curr_y + (row_h - 20.0) / 2.0,
            20.0,
            click_animate,
            false,
            accent_r,
            accent_g,
            accent_b,
        );
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(245.0 / 255.0, 245.0 / 255.0, 246.0 / 255.0, 1.0);
        let anim_text = if click_animate {
            "Animate clicks  ·  ON"
        } else {
            "Animate clicks"
        };
        if let Ok(extents) = context.text_extents(anim_text) {
            context.move_to(
                anim_x + 30.0 - extents.x_bearing(),
                curr_y + row_h / 2.0 - extents.height() / 2.0 - extents.y_bearing(),
            );
            context.show_text(anim_text).ok();
        }
    }
    curr_y += row_h + 10.0;

    // ── 5. Preview area ──
    let preview_area = RectF {
        x: menu_x + 20.0,
        y: curr_y,
        width: menu_w - 40.0,
        height: 138.0,
    };
    {
        let _ = context.save();
        let bg_grad = gtk4::cairo::LinearGradient::new(
            0.0,
            preview_area.y,
            0.0,
            preview_area.y + preview_area.height,
        );
        bg_grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 14.0 / 255.0);
        bg_grad.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 96.0 / 255.0);
        let _ = context.set_source(&bg_grad);
        context.set_line_width(1.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 36.0 / 255.0);
        rounded_rect_path(
            context,
            preview_area.x,
            preview_area.y,
            preview_area.width,
            preview_area.height,
            12.0,
        );
        let _ = context.stroke_preserve();
        let _ = context.set_source(&bg_grad);
        let _ = context.fill();

        // Dotted grid
        context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
        let grid_step = 22.0;
        let mut gy = preview_area.y + grid_step;
        while gy < preview_area.y + preview_area.height {
            let mut gx = preview_area.x + grid_step;
            while gx < preview_area.x + preview_area.width {
                context.new_path();
                context.arc(gx, gy, 1.0, 0.0, PI * 2.0);
                let _ = context.fill();
                gx += grid_step;
            }
            gy += grid_step;
        }

        // Placeholder text
        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 160.0 / 255.0);
        if let Ok(ext) = context.text_extents("Click anywhere to preview") {
            context.move_to(
                preview_area.x + preview_area.width / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
                preview_area.y + preview_area.height / 2.0 - ext.height() / 2.0 - ext.y_bearing(),
            );
            context.show_text("Click anywhere to preview").ok();
        }
        context.set_font_size(10.7);
        context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
        if let Ok(_ext) = context.text_extents("Preview updates live") {
            context.move_to(
                preview_area.x + preview_area.width / 2.0 - 60.0,
                preview_area.y + preview_area.height / 2.0 + 18.0,
            );
            context.show_text("Preview updates live").ok();
        }
        let _ = context.restore();
    }

    // ── 6. OK button ──
    {
        let ok_x = menu_x + menu_w - 96.0;
        let ok_y = menu_y + menu_h - 48.0;
        let ok_w = 76.0;
        let ok_h = 32.0;
        let hovered = hovered_item == 5;

        // Shadow
        context.set_source_rgba(0.0, 0.0, 0.0, 110.0 / 255.0);
        rounded_rect_path(context, ok_x + 0.6, ok_y + 1.6, ok_w, ok_h, 8.0);
        let _ = context.fill();

        // Button gradient
        let _ = context.save();
        let btn_grad = gtk4::cairo::LinearGradient::new(0.0, ok_y, 0.0, ok_y + ok_h);
        if hovered {
            btn_grad.add_color_stop_rgba(0.0, 220.0 / 255.0, 132.0 / 255.0, 84.0 / 255.0, 1.0);
            btn_grad.add_color_stop_rgba(1.0, 178.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 1.0);
        } else {
            btn_grad.add_color_stop_rgba(0.0, 196.0 / 255.0, 110.0 / 255.0, 70.0 / 255.0, 1.0);
            btn_grad.add_color_stop_rgba(1.0, 150.0 / 255.0, 76.0 / 255.0, 44.0 / 255.0, 1.0);
        }
        let _ = context.set_source(&btn_grad);
        context.set_line_width(1.0);
        context.set_source_rgba(accent_rim.0, accent_rim.1, accent_rim.2, 1.0);
        rounded_rect_path(context, ok_x, ok_y, ok_w, ok_h, 8.0);
        let _ = context.stroke_preserve();
        let _ = context.set_source(&btn_grad);
        let _ = context.fill();
        let _ = context.restore();

        context.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(13.3);
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        if let Ok(ext) = context.text_extents("Done") {
            context.move_to(
                ok_x + ok_w / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
                ok_y + ok_h / 2.0 - ext.height() / 2.0 - ext.y_bearing(),
            );
            context.show_text("Done").ok();
        }
    }
}

pub(crate) fn draw_webcam_options(
    context: &gtk4::cairo::Context,
    menu_x: f64,
    menu_y: f64,
    screen_width: f64,
    screen_height: f64,
    _background: Option<&BackgroundFrame>,
    hovered_item: i32,
    webcam_device: i32,
    webcam_size: usize,
    webcam_shape: usize,
    webcam_flip: bool,
) {
    let menu_w = 320.0;
    let item_h = 28.0;
    let header_h = 30.0;
    let pad = 8.0;

    let sections: &[&[(&str, bool, i32)]] = &[
        &[("Camera", false, -1), ("None", true, 0)],
        &[
            ("Size", false, -1),
            ("Small", true, 1),
            ("Medium", true, 2),
            ("Large", true, 3),
            ("Huge", true, 4),
        ],
        &[
            ("Click on camera to toggle Full Screen", false, -1),
            ("Full Screen", true, 5),
        ],
        &[
            ("Shape", false, -1),
            ("Circle", true, 6),
            ("Square", true, 7),
            ("Rectangle", true, 8),
            ("Vertical", true, 9),
        ],
        &[("Options", false, -1), ("Flip Camera", true, 10)],
    ];

    let mut total_h = pad * 2.0;
    for section in sections {
        for (ii, _) in section.iter().enumerate() {
            total_h += if ii == 0 { header_h } else { item_h };
        }
    }

    let popup_x = menu_x.clamp(10.0, screen_width - menu_w - 10.0);
    let popup_y = menu_y.clamp(10.0, screen_height - total_h - 10.0);

    // Solid dark background matching Qt QMenu style
    rounded_rect_path(context, popup_x, popup_y, menu_w, total_h, 12.0);
    context.set_source_rgba(30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0, 235.0 / 255.0);
    let _ = context.fill();
    rounded_rect_path(context, popup_x, popup_y, menu_w, total_h, 12.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.0);
    let _ = context.stroke();

    let mut curr_y = popup_y + pad;
    for section in sections {
        for (ii, (label, _is_clickable, item_idx)) in section.iter().enumerate() {
            if ii == 0 {
                // Section header — dimmed, bold, smaller text
                context.select_font_face(
                    "Sans",
                    gtk4::cairo::FontSlant::Normal,
                    gtk4::cairo::FontWeight::Bold,
                );
                context.set_font_size(11.0);
                context.set_source_rgba(1.0, 1.0, 1.0, 110.0 / 255.0);
                context.move_to(popup_x + 18.0, curr_y + 14.0);
                context.show_text(label).ok();
                curr_y += header_h;
            } else {
                // Clickable item
                let item_rect = RectF {
                    x: popup_x + 4.0,
                    y: curr_y + 1.0,
                    width: menu_w - 8.0,
                    height: item_h - 2.0,
                };
                let hovered = *item_idx == hovered_item;

                if hovered {
                    rounded_rect_path(
                        context,
                        item_rect.x,
                        item_rect.y,
                        item_rect.width,
                        item_rect.height,
                        6.0,
                    );
                    context.set_source_rgba(
                        176.0 / 255.0,
                        92.0 / 255.0,
                        56.0 / 255.0,
                        220.0 / 255.0,
                    );
                    let _ = context.fill();
                }

                let is_selected = match *item_idx {
                    0 => webcam_device == -1,
                    1 => webcam_size == 0,
                    2 => webcam_size == 1,
                    3 => webcam_size == 2,
                    4 => webcam_size == 3,
                    5 => webcam_size == 4,
                    6 => webcam_shape == 0,
                    7 => webcam_shape == 1,
                    8 => webcam_shape == 2,
                    9 => webcam_shape == 3,
                    10 => webcam_flip,
                    _ => false,
                };

                let text_color = if hovered {
                    (1.0, 234.0 / 255.0, 214.0 / 255.0, 1.0)
                } else {
                    (241.0 / 255.0, 241.0 / 255.0, 243.0 / 255.0, 1.0)
                };

                // Checkmark
                if is_selected {
                    context.new_path();
                    context.move_to(popup_x + 18.0, curr_y + item_h / 2.0 + 1.0);
                    context.rel_line_to(3.0, 3.0);
                    context.rel_line_to(6.0, -6.0);
                    context.set_source_rgba(text_color.0, text_color.1, text_color.2, text_color.3);
                    context.set_line_width(1.8);
                    let _ = context.stroke();
                }

                // Item text
                context.select_font_face(
                    "Sans",
                    gtk4::cairo::FontSlant::Normal,
                    gtk4::cairo::FontWeight::Normal,
                );
                context.set_font_size(13.0);
                context.set_source_rgba(text_color.0, text_color.1, text_color.2, text_color.3);
                context.move_to(popup_x + 32.0, curr_y + item_h / 2.0 + 4.5);
                context.show_text(label).ok();

                curr_y += item_h;
            }
        }
    }
}

fn webcam_preview_size(
    sel_w: f64,
    sel_h: f64,
    webcam_size: usize,
    webcam_shape: usize,
) -> (f64, f64) {
    const MARGIN: f64 = 10.0;
    let (mut preview_w, mut preview_h) = match webcam_size {
        0 => (120.0, 160.0),
        2 => (280.0, 370.0),
        3 => (360.0, 480.0),
        4 => (
            (sel_w - 2.0 * MARGIN).max(1.0),
            (sel_h - 2.0 * MARGIN).max(1.0),
        ),
        _ => (200.0, 260.0),
    };

    match webcam_shape {
        0 | 1 => preview_h = preview_w,
        2 => preview_h = preview_w * 0.75,
        _ => {}
    }

    preview_w = preview_w.min((sel_w - 2.0 * MARGIN).max(1.0));
    preview_h = preview_h.min((sel_h - 2.0 * MARGIN).max(1.0));
    (preview_w, preview_h)
}

fn webcam_preview_rect(
    sel_x: f64,
    sel_y: f64,
    sel_w: f64,
    sel_h: f64,
    webcam_size: usize,
    webcam_shape: usize,
    webcam_rel_x: f64,
    webcam_rel_y: f64,
) -> RectF {
    const MARGIN: f64 = 10.0;
    let (preview_w, preview_h) = webcam_preview_size(sel_w, sel_h, webcam_size, webcam_shape);
    let min_x = sel_x + MARGIN;
    let max_x = min_x.max(sel_x + sel_w - preview_w - MARGIN);
    let min_y = sel_y + MARGIN;
    let max_y = min_y.max(sel_y + sel_h - preview_h - MARGIN);
    RectF {
        x: min_x + (max_x - min_x) * webcam_rel_x.clamp(0.0, 1.0),
        y: min_y + (max_y - min_y) * (1.0 - webcam_rel_y.clamp(0.0, 1.0)),
        width: preview_w,
        height: preview_h,
    }
}

fn draw_webcam_preview(
    context: &gtk4::cairo::Context,
    sel_x: f64,
    sel_y: f64,
    sel_w: f64,
    sel_h: f64,
    webcam_size: usize,
    webcam_shape: usize,
    webcam_rel_x: f64,
    webcam_rel_y: f64,
    webcam_device: i32,
) {
    let rect = webcam_preview_rect(
        sel_x,
        sel_y,
        sel_w,
        sel_h,
        webcam_size,
        webcam_shape,
        webcam_rel_x,
        webcam_rel_y,
    );
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);

    context.new_path();
    if webcam_shape == 0 {
        context.arc(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            rect.width.min(rect.height) / 2.0,
            0.0,
            PI * 2.0,
        );
    } else {
        let radius = if webcam_shape == 1 { 8.0 } else { 12.0 };
        rounded_rect_path(context, rect.x, rect.y, rect.width, rect.height, radius);
    }
    context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    context.set_line_width(1.5);
    let _ = context.stroke();

    let label = if webcam_device >= 0 {
        format!("Camera {}", webcam_device)
    } else {
        "Webcam".to_string()
    };
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(10.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 120.0 / 255.0);
    context.move_to(rect.x + 8.0, rect.y + rect.height - 8.0);
    let _ = context.show_text(&label);

    let _ = context.restore();
}

pub(crate) fn draw_overlay(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    state: &Arc<Mutex<SelectorState>>,
    background: Option<&BackgroundFrame>,
) {
    let st = state.lock().unwrap();

    let screen_width = width.max(1) as f64;
    let screen_height = height.max(1) as f64;

    if st.overlay_mode == OverlayMode::CrosshairCapture {
        if let Some(bg) = background {
            paint_surface_fullscreen(
                context,
                &bg.surface,
                bg.width,
                bg.height,
                screen_width,
                screen_height,
            );
        }

        let guide_x = st.current_x.clamp(0.0, screen_width);
        let guide_y = st.current_y.clamp(0.0, screen_height);

        context.set_source_rgba(
            BRAND_ORANGE_R,
            BRAND_ORANGE_G,
            BRAND_ORANGE_B,
            200.0 / 255.0,
        );
        context.set_line_width(1.0);
        context.move_to(0.0, guide_y);
        context.line_to(screen_width, guide_y);
        context.move_to(guide_x, 0.0);
        context.line_to(guide_x, screen_height);
        let _ = context.stroke();

        let label = if st.is_dragging || st.completed {
            let rect = current_selection_rect(&st);
            let x = rect.left;
            let y = rect.top;
            let sel_w = rect.width();
            let sel_h = rect.height();

            context.set_source_rgba(
                BRAND_ORANGE_R,
                BRAND_ORANGE_G,
                BRAND_ORANGE_B,
                240.0 / 255.0,
            );
            context.set_line_width(2.0);
            context.rectangle(
                x + 0.5,
                y + 0.5,
                (sel_w - 1.0).max(0.0),
                (sel_h - 1.0).max(0.0),
            );
            let _ = context.stroke();

            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 40.0 / 255.0);
            context.rectangle(x, y, sel_w, sel_h);
            let _ = context.fill();

            format!("{} × {}", sel_w as i32, sel_h as i32)
        } else {
            format!("{}, {}", guide_x as i32, guide_y as i32)
        };

        draw_crosshair_mode_bubble(
            context,
            guide_x,
            guide_y,
            &label,
            screen_width,
            screen_height,
        );
        return;
    }

    // ── Step 1: paint the background across the entire screen ──
    // Paint the original screenshot (if available) then darken it with a
    // semi-transparent overlay. No blur — this keeps opening instant.
    if let Some(bg) = background {
        paint_surface_fullscreen(
            context,
            &bg.surface,
            bg.width,
            bg.height,
            screen_width,
            screen_height,
        );
        // Dark tint over the full screen; the selection area will be
        // revealed sharp in Step 2 by painting the original on top.
        context.set_source_rgba(0.0, 0.0, 0.0, 140.0 / 255.0);
        let _ = context.paint();
    } else {
        // No pre-captured background (capture-after-selection / live overlay path).
        // Use a very light tint so the desktop remains clearly visible.
        // The selection rectangle will be fully cleared (Operator::Clear) to show
        // the desktop at 100% brightness inside the selection.
        context.set_source_rgba(0.0, 0.0, 0.0, 0.20);
        let _ = context.paint();
    }

    if st.fullscreen_mode {
        // ── Fullscreen mode: the whole screen IS the selection ──
        // The darkened background is already painted; add a very subtle extra
        // vignette so the corner markers and toolbar stand out.
        if background.is_some() {
            context.set_source_rgba(0.0, 0.0, 0.0, 0.10);
            let _ = context.paint();
        } else {
            // Full-screen mode with live background means the full desktop is
            // selected, so clear the dimming tint entirely.
            let _ = context.save();
            context.set_operator(gtk4::cairo::Operator::Clear);
            let _ = context.paint();
            let _ = context.restore();
        }

        // Corner markers at screen edges
        draw_resize_markers(context, 0.0, 0.0, screen_width, screen_height);

        // Toolbar (auto-positioned below / above the full-screen rect)
        draw_feature_toolbar(
            context,
            0.0,
            0.0,
            screen_width,
            screen_height,
            screen_width,
            screen_height,
            background,
            st.active_tool_index,
            st.hover_tool_index,
            st.hover_size_panel,
            st.hover_crop_panel,
            st.capture_crop_menu_open,
            st.capture_aspect_ratio_index,
            st.hovered_capture_crop_menu_item,
        );
    } else if st.is_dragging || st.completed {
        // ── Normal area-selection mode ──
        let rect = current_selection_rect(&st);
        let x = rect.left;
        let y = rect.top;
        let sel_w = rect.width();
        let sel_h = rect.height();

        // ── Step 2: reveal the original (sharp) image inside the selection ──
        if let Some(bg) = background {
            paint_surface_clipped(
                context,
                &bg.surface,
                bg.width,
                bg.height,
                screen_width,
                screen_height,
                x,
                y,
                sel_w,
                sel_h,
            );
        } else {
            // Live selector path: reveal the selected rectangle from the real
            // desktop by clearing the tint in that region.
            let _ = context.save();
            context.set_operator(gtk4::cairo::Operator::Clear);
            context.rectangle(x, y, sel_w, sel_h);
            let _ = context.fill();
            let _ = context.restore();
        }

        if st.is_dragging {
            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 30.0 / 255.0);
            context.rectangle(x, y, sel_w, sel_h);
            let _ = context.fill();
        }

        draw_resize_markers(context, x, y, sel_w, sel_h);

        // ── Step 3: toolbar + resize markers on top ──
        if st.recording.panel_open {
            draw_recording_panel(
                context,
                x,
                y,
                sel_w,
                sel_h,
                screen_width,
                screen_height,
                background,
                st.recording.hover_record_tile,
                st.recording.crop_menu_open,
                st.recording.record_aspect_ratio_index,
                st.recording.hovered_crop_menu_item,
                st.recording.settings_menu_open,
                st.recording.settings_tab,
                st.recording.hovered_settings_item,
                st.recording.settings_dropdown_open,
                st.recording.video_max_res,
                st.recording.video_fps,
                st.recording.record_mono,
                st.recording.open_editor,
                st.recording.rec_controls,
                st.recording.display_rec_time,
                st.recording.hidpi,
                st.recording.do_not_disturb,
                st.recording.show_cursor,
                st.recording.rec_clicks,
                st.recording.rec_keystrokes,
                st.recording.rec_webcam,
                st.recording.remember_selection,
                st.recording.dim_screen,
                st.recording.show_countdown,
                st.recording.gif_fps,
                st.recording.gif_quality,
                st.recording.optimize_gif,
                st.recording.gif_size_idx,
            );
            if st.recording.rec_webcam {
                draw_webcam_preview(
                    context,
                    x,
                    y,
                    sel_w,
                    sel_h,
                    st.recording.webcam_size,
                    st.recording.webcam_shape,
                    st.recording.webcam_rel_x,
                    st.recording.webcam_rel_y,
                    st.recording.webcam_device,
                );
            }
            // Click options menu (on top of recording panel)
            if st.recording.click_options_open {
                let panel_x = (x + (sel_w - 440.0) / 2.0).clamp(10.0, screen_width - 450.0);
                let panel_y = (y + 24.0).clamp(10.0, screen_height - 530.0);
                draw_click_options(
                    context,
                    panel_x,
                    panel_y,
                    screen_width,
                    screen_height,
                    background,
                    st.recording.hovered_click_item,
                    st.recording.click_size,
                    st.recording.click_color,
                    st.recording.click_style,
                    st.recording.click_animate,
                );
            }
            // Countdown bubble for timer capture
            if st.countdown_active {
                draw_countdown_bubble(
                    context,
                    x,
                    y,
                    sel_w,
                    sel_h,
                    screen_width,
                    screen_height,
                    st.countdown_value,
                    st.hovered_countdown_cancel,
                    st.intent,
                );
            }
            // Scroll capture Chrome extension popup
            if st.scroll_popup_open {
                draw_scroll_popup(
                    context,
                    x,
                    y,
                    sel_w,
                    sel_h,
                    screen_width,
                    screen_height,
                    st.hovered_scroll_popup_close,
                );
            }
            // Webcam options menu
            if st.recording.webcam_options_open {
                let panel_x = (x + (sel_w - 320.0) / 2.0).clamp(10.0, screen_width - 330.0);
                let panel_y = (y + 24.0).clamp(10.0, screen_height - 350.0);
                draw_webcam_options(
                    context,
                    panel_x,
                    panel_y,
                    screen_width,
                    screen_height,
                    background,
                    st.recording.hovered_webcam_item,
                    st.recording.webcam_device,
                    st.recording.webcam_size,
                    st.recording.webcam_shape,
                    st.recording.webcam_flip,
                );
            }
        }
        // Always draw toolbar (even when recording panel is open for Timer/Scroll access)
        draw_feature_toolbar(
            context,
            x,
            y,
            sel_w,
            sel_h,
            screen_width,
            screen_height,
            background,
            st.active_tool_index,
            st.hover_tool_index,
            st.hover_size_panel,
            st.hover_crop_panel,
            st.capture_crop_menu_open,
            st.capture_aspect_ratio_index,
            st.hovered_capture_crop_menu_item,
        );
    }
    // else: idle state — the darkened background painted in Step 1 is enough.
}

fn draw_countdown_bubble(
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

    if intent == OverlayIntent::Record {
        // Pill-shaped bubble for recording (like C++)
        let pill_w = 100.0;
        let pill_h = 38.0;
        let pill_x = (screen_width - pill_w) / 2.0;
        let pill_y = 28.0;

        // Draw pill background
        if hovered_cancel {
            context.set_source_rgba(0.78, 0.24, 0.16, 0.95);
        } else {
            context.set_source_rgba(0.91, 0.33, 0.13, 0.92);
        }
        rounded_rect_path(context, pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
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
        let text = countdown_value.to_string();
        if let Ok(extents) = context.text_extents(&text) {
            let text_x =
                pill_x + 40.0 + (pill_w - 44.0 - extents.width()) / 2.0 - extents.x_bearing();
            let text_y = pill_y + (pill_h + extents.height()) / 2.0 - extents.y_bearing();
            context.move_to(text_x, text_y);
            let _ = context.show_text(&text);
        }
    } else {
        // Circle bubble for capture (like C++)
        let bubble_size = 120.0;
        let bubble_x = (screen_width - bubble_size) / 2.0;
        let bubble_y = (screen_height - bubble_size) / 2.0;

        // Draw circle background
        if hovered_cancel {
            context.set_source_rgba(0.52, 0.15, 0.09, 0.95);
        } else {
            context.set_source_rgba(0.0, 0.0, 0.0, 0.94);
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

fn draw_scroll_popup(
    context: &gtk4::cairo::Context,
    _sel_x: f64,
    _sel_y: f64,
    _sel_w: f64,
    _sel_h: f64,
    screen_width: f64,
    screen_height: f64,
    hovered_close: bool,
) {
    let _ = context.save();

    let popup_w = 400.0;
    let popup_h = 200.0;
    let popup_x = (screen_width - popup_w) / 2.0;
    let popup_y = (screen_height - popup_h) / 2.0;

    // Draw popup background
    context.set_source_rgba(0.08, 0.08, 0.09, 0.96);
    rounded_rect_path(context, popup_x, popup_y, popup_w, popup_h, 12.0);
    let _ = context.fill();

    // Draw border
    context.set_source_rgba(1.0, 1.0, 1.0, 0.15);
    context.set_line_width(1.0);
    rounded_rect_path(
        context,
        popup_x + 0.5,
        popup_y + 0.5,
        popup_w - 1.0,
        popup_h - 1.0,
        11.5,
    );
    let _ = context.stroke();

    // Draw close button
    let close_size = 24.0;
    let close_x = popup_x + popup_w - close_size - 12.0;
    let close_y = popup_y + 12.0;
    if hovered_close {
        context.set_source_rgba(0.8, 0.25, 0.15, 1.0);
    } else {
        context.set_source_rgba(0.4, 0.4, 0.4, 1.0);
    }
    rounded_rect_path(context, close_x, close_y, close_size, close_size, 6.0);
    let _ = context.fill();
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(2.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(close_x + 7.0, close_y + 7.0);
    context.line_to(close_x + close_size - 7.0, close_y + close_size - 7.0);
    context.move_to(close_x + close_size - 7.0, close_y + 7.0);
    context.line_to(close_x + 7.0, close_y + close_size - 7.0);
    let _ = context.stroke();

    // Draw title
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(16.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    let title = "Scroll Capture";
    if let Ok(extents) = context.text_extents(title) {
        let text_x = popup_x + 20.0;
        let text_y = popup_y + 30.0 - extents.y_bearing();
        context.move_to(text_x, text_y);
        let _ = context.show_text(title);
    }

    // Draw description
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    context.set_font_size(13.0);
    context.set_source_rgba(0.9, 0.9, 0.9, 0.9);
    let desc = "Scroll capture requires a browser extension.";
    if let Ok(extents) = context.text_extents(desc) {
        let text_x = popup_x + 20.0;
        let text_y = popup_y + 60.0 - extents.y_bearing();
        context.move_to(text_x, text_y);
        let _ = context.show_text(desc);
    }

    // Draw extension info
    context.set_font_size(12.0);
    context.set_source_rgba(0.7, 0.7, 0.7, 0.8);
    let info = "Install the ApexShot Chrome extension to enable scroll capture.";
    if let Ok(extents) = context.text_extents(info) {
        let text_x = popup_x + 20.0;
        let text_y = popup_y + 85.0 - extents.y_bearing();
        context.move_to(text_x, text_y);
        let _ = context.show_text(info);
    }

    // Draw download button
    let btn_w = 140.0;
    let btn_h = 36.0;
    let btn_x = popup_x + (popup_w - btn_w) / 2.0;
    let btn_y = popup_y + 120.0;
    context.set_source_rgba(0.91, 0.33, 0.13, 0.92);
    rounded_rect_path(context, btn_x, btn_y, btn_w, btn_h, 8.0);
    let _ = context.fill();

    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(14.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    let btn_text = "Download Extension";
    if let Ok(extents) = context.text_extents(btn_text) {
        let text_x = btn_x + (btn_w - extents.width()) / 2.0 - extents.x_bearing();
        let text_y = btn_y + (btn_h + extents.height()) / 2.0 - extents.y_bearing();
        context.move_to(text_x, text_y);
        let _ = context.show_text(btn_text);
    }

    let _ = context.restore();
}

fn draw_crosshair_mode_bubble(
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

    rounded_rect_path(context, bx, by, bubble_w, bubble_h, 6.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
    let _ = context.fill();

    rounded_rect_path(
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

    draw_text_centered(
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

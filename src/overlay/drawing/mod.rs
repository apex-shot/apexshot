use super::background::{paint_surface_clipped, paint_surface_fullscreen, BackgroundFrame};
use super::geometry::current_selection_rect;
use super::icons::{draw_toolbar_icon, ToolbarIcon, TOOLBAR_ICONS, TOOLBAR_LABELS};
use super::layout::*;
use super::recording::state::OverlayIntent;
use super::state::{OverlayMode, SelectorState};
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

mod mode_overlays;
mod recording_ui;
mod settings_ui;

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
    timer_delay_active: bool,
    capture_delay_seconds: i32,
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
    let timer_tool_active = timer_delay_active && capture_delay_seconds > 0;

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
    if timer_tool_active && active_tool_index != super::icons::TOOLBAR_TIMER_INDEX {
        draw_accent(
            context,
            layout.item_cells[super::icons::TOOLBAR_TIMER_INDEX],
            true,
        );
    }
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
        let is_active = index == active_tool_index
            || (index == super::icons::TOOLBAR_TIMER_INDEX && timer_tool_active);

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

        if index == 4 && timer_tool_active {
            let badge_text = format!("{}s", capture_delay_seconds);
            context.select_font_face(
                "Sans",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            context.set_font_size(8.8);
            if let Ok(extents) = context.text_extents(&badge_text) {
                let badge_w = (extents.width() + 10.0).max(22.0);
                let badge_h = 14.0;
                let badge_x = cell.x + cell.width - badge_w - 6.0;
                let badge_y = cell.y + 6.0;
                rounded_rect_path(context, badge_x, badge_y, badge_w, badge_h, 7.0);
                context.set_source_rgba(178.0 / 255.0, 84.0 / 255.0, 42.0 / 255.0, 230.0 / 255.0);
                let _ = context.fill();
                context.set_source_rgba(1.0, 1.0, 1.0, 248.0 / 255.0);
                let text_x = badge_x + (badge_w - extents.width()) / 2.0 - extents.x_bearing();
                let text_y = badge_y + (badge_h - extents.height()) / 2.0 - extents.y_bearing();
                context.move_to(text_x, text_y);
                let _ = context.show_text(&badge_text);
            }
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

pub(super) fn draw_text_centered(
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

pub(super) fn draw_aspect_ratio_menu(
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
    for (i, _label) in ASPECT_RATIO_OPTIONS.iter().enumerate() {
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

pub(crate) fn draw_overlay(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    state: &Arc<Mutex<SelectorState>>,
    background: Option<&BackgroundFrame>,
) {
    let mut st = state.lock().unwrap();

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

        mode_overlays::draw_crosshair_mode_bubble(
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

    // ── Window Hover Highlight ──
    if let Some(win_idx) = st.hovered_window {
        if !st.completed && !st.is_dragging {
            let win = &st.windows[win_idx];
            let x = win.x as f64;
            let y = win.y as f64;
            let w = win.width as f64;
            let h = win.height as f64;

            // Reveal the window area (remove the dark tint)
            if let Some(bg) = background {
                let _ = context.save();
                context.rectangle(x, y, w, h);
                context.clip();
                paint_surface_fullscreen(
                    context,
                    &bg.surface,
                    bg.width,
                    bg.height,
                    screen_width,
                    screen_height,
                );
                let _ = context.restore();
            } else {
                let _ = context.save();
                context.set_operator(gtk4::cairo::Operator::Clear);
                context.rectangle(x, y, w, h);
                let _ = context.fill();
                let _ = context.restore();
            }

            // Draw a subtle border around the hovered window
            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 0.8);
            context.set_line_width(2.0);
            context.rectangle(x + 1.0, y + 1.0, w - 2.0, h - 2.0);
            let _ = context.stroke();

            // Add a very subtle inner glow
            context.set_source_rgba(BRAND_ORANGE_R, BRAND_ORANGE_G, BRAND_ORANGE_B, 0.1);
            context.rectangle(x, y, w, h);
            let _ = context.fill();
        }
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

        if st.recording.panel_open {
            recording_ui::draw_recording_panel(
                context,
                0.0,
                0.0,
                screen_width,
                screen_height,
                screen_width,
                screen_height,
                background,
                st.recording.hover_record_tile,
                st.recording.selected_record_type,
                st.recording.crop_menu_open,
                st.recording.record_aspect_ratio_index,
                st.recording.hovered_crop_menu_item,
                st.recording.settings_menu_open,
                st.recording.settings_tab,
                st.recording.hovered_settings_item,
                st.recording.settings_dropdown_open,
                st.recording.video_max_res,
                st.recording.video_fps,
                st.recording.mic_toggle,
                st.recording.speaker_toggle,
                st.recording.mic_level,
                st.recording.speaker_level,
                st.recording.record_mono,
                st.recording.open_editor,
                st.recording.rec_controls,
                st.recording.hidpi,
                st.recording.do_not_disturb,
                st.recording.remember_selection,
                st.recording.dim_screen,
                st.recording.show_countdown,
                st.recording.gif_fps,
                st.recording.gif_quality,
                st.recording.optimize_gif,
                st.recording.gif_size_idx,
            );
        } else {
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
                st.timer_delay_active,
                st.capture_delay_seconds,
            );
        }
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
            recording_ui::draw_recording_panel(
                context,
                x,
                y,
                sel_w,
                sel_h,
                screen_width,
                screen_height,
                background,
                st.recording.hover_record_tile,
                st.recording.selected_record_type,
                st.recording.crop_menu_open,
                st.recording.record_aspect_ratio_index,
                st.recording.hovered_crop_menu_item,
                st.recording.settings_menu_open,
                st.recording.settings_tab,
                st.recording.hovered_settings_item,
                st.recording.settings_dropdown_open,
                st.recording.video_max_res,
                st.recording.video_fps,
                st.recording.mic_toggle,
                st.recording.speaker_toggle,
                st.recording.mic_level,
                st.recording.speaker_level,
                st.recording.record_mono,
                st.recording.open_editor,
                st.recording.rec_controls,
                st.recording.hidpi,
                st.recording.do_not_disturb,
                st.recording.remember_selection,
                st.recording.dim_screen,
                st.recording.show_countdown,
                st.recording.gif_fps,
                st.recording.gif_quality,
                st.recording.optimize_gif,
                st.recording.gif_size_idx,
            );
            // Volume popup menus (positioned top-centre of selection like other menus)
            {
                let popup_x = (x + (sel_w - 280.0) / 2.0).clamp(10.0, screen_width - 290.0);
                let popup_y = (y + 24.0).clamp(10.0, screen_height - 140.0);
                if st.recording.mic_volume_popup_open {
                    recording_ui::draw_volume_popup(
                        context,
                        popup_x,
                        popup_y,
                        screen_width,
                        screen_height,
                        background,
                        st.recording.mic_volume,
                        "Microphone Volume",
                        st.recording.volume_slider_dragging,
                    );
                }
                if st.recording.speaker_volume_popup_open {
                    recording_ui::draw_volume_popup(
                        context,
                        popup_x,
                        popup_y,
                        screen_width,
                        screen_height,
                        background,
                        st.recording.speaker_volume,
                        "Speaker Volume",
                        st.recording.volume_slider_dragging,
                    );
                }
            }
        } else {
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
                st.timer_delay_active,
                st.capture_delay_seconds,
            );
        }

        // Popups rendered on top of everything (both recording and area modes)
        if st.window_picker_open {
            mode_overlays::draw_window_picker(
                context,
                x + sel_w / 2.0,
                y + sel_h / 2.0,
                screen_width,
                screen_height,
                background,
                &st.windows,
                st.hovered_window_picker_entry,
            );
        }
    }
    // else: idle state — the darkened background painted in Step 1 is enough.

    // Timer capture countdown stays on top and centered on screen while the
    // selection remains movable/resizable underneath.
    if st.countdown_active {
        let r = current_selection_rect(&st);
        let (bx, by, bw, bh) = if st.intent != OverlayIntent::Record {
            let pill_w = 112.0;
            let pill_h = 44.0;
            let pill_x = (screen_width - pill_w) / 2.0;
            let pill_y = 28.0;
            (pill_x, pill_y, pill_w, pill_h)
        } else {
            let bubble_size = 184.0;
            let bubble_x = (screen_width - bubble_size) / 2.0;
            let bubble_y = (screen_height - bubble_size) / 2.0;
            (bubble_x, bubble_y, bubble_size, bubble_size)
        };
        st.countdown_bubble_x = bx;
        st.countdown_bubble_y = by;
        st.countdown_bubble_w = bw;
        st.countdown_bubble_h = bh;
        mode_overlays::draw_countdown_bubble(
            context,
            r.left,
            r.top,
            r.width(),
            r.height(),
            screen_width,
            screen_height,
            st.countdown_value,
            st.hovered_countdown_cancel,
            st.intent,
        );
    }

    // Scroll popup: centered on selection when available, otherwise screen-centered
    if st.scroll_popup_open {
        let (cx, cy) = if st.completed || st.is_dragging {
            let r = current_selection_rect(&st);
            (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
        } else {
            (screen_width / 2.0, screen_height / 2.0)
        };
        mode_overlays::draw_scroll_popup(
            context,
            cx,
            cy,
            screen_width,
            screen_height,
            background,
            st.hovered_scroll_popup_close,
            st.hovered_scroll_download,
        );
    }
}

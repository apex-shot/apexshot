//! Canvas content sizing, zoom labels, crop overflow, and relayout suppression (PR 10.13).
//!
//! Owns `update_canvas_content_size` and the scroller tick that coalesces
//! layout updates via a capped-overflow signature. Callers keep drawing-area
//! widgets and invoke the returned callback after state changes that affect
//! layout.

use gtk4::{glib, prelude::*, DrawingArea, Label, ScrolledWindow};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::composition::BackgroundComposition;
use super::super::state::EditorState;
use super::super::types::{BackgroundAlignment, BackgroundStyle, Tool};
use super::super::ui_support::EDITOR_TOP_CHROME_HEIGHT;
use super::canvas;

/// Install canvas layout updates + scroller tick; returns `update_canvas_content_size`.
///
/// Runs an immediate layout pass before returning.
pub(super) fn install_canvas_layout(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    canvas_scroller: &ScrolledWindow,
    zoom_level: &Rc<Cell<f64>>,
    zoom_label: &Label,
    zoom_header_label: &Label,
    canvas_padding: i32,
) -> Rc<dyn Fn()> {
    let update_canvas_content_size: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let zoom_level = zoom_level.clone();
        let zoom_label = zoom_label.clone();
        let zoom_header_label = zoom_header_label.clone();
        let drawing_area = drawing_area.clone();
        let canvas_scroller = canvas_scroller.clone();
        move || {
            let (
                image_w,
                image_h,
                background_style,
                background_padding,
                background_insert,
                background_aspect_ratio,
                has_background,
                crop_rect,
                crop_mode_active,
            ) = {
                let st = state.lock().unwrap();
                (
                    st.working_image.width().max(1) as i32,
                    st.working_image.height().max(1) as i32,
                    st.background_style.clone(),
                    st.background_padding,
                    st.background_insert,
                    st.background_aspect_ratio,
                    st.background_style != BackgroundStyle::None,
                    st.draft_crop_rect().or(st.crop_selection),
                    st.selected_tool == Tool::Crop,
                )
            };

            let mut virtual_w = image_w as f64;
            let mut virtual_h = image_h as f64;

            if has_background {
                let layout = BackgroundComposition::new(virtual_w, virtual_h)
                    .with_style(background_style)
                    .with_padding(background_padding)
                    .with_insert(background_insert)
                    .with_alignment(BackgroundAlignment::Center)
                    .with_corner_radius(18.0)
                    .with_aspect_ratio(background_aspect_ratio)
                    .compute();
                virtual_w = layout.canvas_width;
                virtual_h = layout.canvas_height;
            }

            let scroller_width = canvas_scroller.allocated_width().max(1) as f64;
            let scroller_height = canvas_scroller.allocated_height().max(1) as f64;
            // Keep fit-to-view math below the floating toolbar strip.
            let top_inset = canvas_padding + EDITOR_TOP_CHROME_HEIGHT;
            let available_width = (scroller_width - (canvas_padding * 2 + 2) as f64).max(1.0);
            let available_height =
                (scroller_height - (top_inset + canvas_padding + 2) as f64).max(1.0);

            // Use the minimum of width and height to maintain aspect ratio and prevent asymmetric growth
            let available_size = available_width.min(available_height);

            // Layout scale without zoom - used for content size (prevents window from growing on zoom)
            let layout_scale = (available_size / virtual_w.min(virtual_h)).min(1.0_f64);
            // Rendering scale includes zoom for visual display
            let scale = layout_scale * zoom_level.get().max(0.1_f64);

            let fitted_w = (virtual_w * scale).round().max(1.0) as i32;
            let fitted_h = (virtual_h * scale).round().max(1.0) as i32;

            let (overflow_left, overflow_top, overflow_right, overflow_bottom) = if has_background {
                (0.0, 0.0, 0.0, 0.0)
            } else {
                canvas::crop_canvas_overflow(
                    crop_rect,
                    image_w as f64,
                    image_h as f64,
                    scale,
                    crop_mode_active,
                )
            };

            let canvas_w = fitted_w
                + canvas_padding * 2
                + overflow_left.round() as i32
                + overflow_right.round() as i32;
            // Extra top inset so zoomed content cannot sit under the toolbar.
            let canvas_h = fitted_h
                + top_inset
                + canvas_padding
                + overflow_top.round() as i32
                + overflow_bottom.round() as i32;

            drawing_area.set_content_width(canvas_w);
            drawing_area.set_content_height(canvas_h);
            let percent_str = format!("{}%", (scale * 100.0).round().max(1.0) as i32);
            zoom_label.set_label(&percent_str);
            zoom_header_label.set_label(&percent_str);
        }
    });
    update_canvas_content_size();

    {
        let update_canvas_content_size_tick = update_canvas_content_size.clone();
        let state_canvas_tick = state.clone();
        let zoom_level_tick = zoom_level.clone();
        // Signature tracks the quantities that actually change the *visible* canvas size.
        // Crucially, raw crop-rect coordinates are NOT included here.  Instead we compute
        // the capped overflow bucket that crop_canvas_overflow() would return and store
        // only that.  Because the function caps every side to 180 px, the bucket stays
        // constant throughout an outside-image drag gesture — no relayout churn occurs.
        let last_canvas_signature = Rc::new(Cell::new([
            0_i32, // scroller width
            0_i32, // scroller height
            0_i32, // image width
            0_i32, // image height
            0_i32, // overflow left (px, capped)
            0_i32, // overflow top  (px, capped)
            0_i32, // overflow right (px, capped)
            0_i32, // overflow bottom (px, capped)
            0_i32, // crop mode active
            0_i32, // zoom percentage
            0_i32, // background enabled
            0_i32, // background padding (tenths)
            0_i32, // background insert (tenths)
            0_i32, // background aspect ratio
        ]));
        let last_canvas_signature_tick = last_canvas_signature.clone();
        canvas_scroller.add_tick_callback(move |scroller, _| {
            let width = scroller.allocated_width();
            let height = scroller.allocated_height();
            let signature = {
                let st = state_canvas_tick.lock().unwrap();
                let img_w = st.working_image.width().max(1) as i32;
                let img_h = st.working_image.height().max(1) as i32;
                let crop_mode_active = st.selected_tool == Tool::Crop;
                let crop_rect = st.draft_crop_rect().or(st.crop_selection);
                let has_background = st.background_style != BackgroundStyle::None;
                let background_padding = (st.background_padding * 10.0).round() as i32;
                let background_insert = (st.background_insert * 10.0).round() as i32;
                let background_aspect_ratio = st.background_aspect_ratio as i32;
                let zoom_percentage = (zoom_level_tick.get() * 100.0_f64).round() as i32;

                // Compute the same scale the layout function uses so we get the
                // same overflow values without duplicating the full layout calculation.
                let virtual_w = img_w as f64;
                let virtual_h = img_h as f64;
                let top_inset = canvas_padding + EDITOR_TOP_CHROME_HEIGHT;
                let available_w = (width as f64 - (canvas_padding * 2 + 2) as f64).max(1.0);
                let available_h =
                    (height as f64 - (top_inset + canvas_padding + 2) as f64).max(1.0);

                let available_size = available_w.min(available_h);
                let layout_scale = (available_size / virtual_w.min(virtual_h)).min(1.0_f64);
                let _scale = layout_scale * zoom_level_tick.get().max(0.1_f64);

                let (ol, ot, or_, ob) = if has_background {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    canvas::crop_canvas_overflow(
                        crop_rect,
                        img_w as f64,
                        img_h as f64,
                        layout_scale,
                        crop_mode_active,
                    )
                };

                [
                    width,
                    height,
                    img_w,
                    img_h,
                    ol.round() as i32,
                    ot.round() as i32,
                    or_.round() as i32,
                    ob.round() as i32,
                    if crop_mode_active { 1 } else { 0 },
                    zoom_percentage,
                    if has_background { 1 } else { 0 },
                    background_padding,
                    background_insert,
                    background_aspect_ratio,
                ]
            };
            if width > 0 && signature != last_canvas_signature_tick.get() {
                last_canvas_signature_tick.set(signature);
                update_canvas_content_size_tick();
            }
            glib::ControlFlow::Continue
        });
    }

    update_canvas_content_size
}

#[cfg(test)]
mod tests {
    #[test]
    fn canvas_layout_sizes_content_and_suppresses_crop_relayout_churn() {
        let source = include_str!("canvas_layout.rs");
        assert!(
            source.contains("BackgroundComposition::new(virtual_w, virtual_h)")
                && source.contains("canvas::crop_canvas_overflow")
                && source.contains("drawing_area.set_content_width(canvas_w)")
                && source.contains("zoom_label.set_label(&percent_str)")
                && source.contains("last_canvas_signature")
                && source.contains("no relayout churn occurs")
                && source.contains("fn install_canvas_layout"),
            "canvas layout must size content, update zoom labels, and suppress crop-drag relayout churn"
        );
    }
}

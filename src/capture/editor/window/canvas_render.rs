//! Canvas draw function and render caches (PR 10.19).
//!
//! Owns working-image / background surface caches and the
//! `DrawingArea::set_draw_func` body. Snapshot `EditorState` under the lock,
//! then release before Cairo work. Session/bootstrap entry points stay in
//! `window/mod.rs`.

use gtk4::cairo::ImageSurface;
use gtk4::{prelude::*, Button, DrawingArea};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::capture::editor::{
    color::selection_hit_padding_for_scale,
    composition::BackgroundComposition,
    render::{
        blur_background_surface, draw_active_text_input, draw_annotation_action,
        draw_arrow_control_handles, draw_arrow_selection_outline,
        draw_canvas_checkerboard_background, draw_draft_action, draw_rgba_to_context,
        draw_selection_handles, draw_selection_outline, draw_text_edit_border,
        draw_text_edit_handles, paint_background_noise, rgba_image_to_surface, text_action_bounds,
        BACKGROUND_BLUR_MAX_RADIUS,
    },
    selection::{action_bounds_with_padding, action_resize_handles},
    state::EditorState,
    types::{frame_needs_canvas, AnnotationAction, BackgroundStyle, Rect, Tool, ViewTransform},
    ui_support::{DockedBarInset, EDITOR_TOP_CHROME_HEIGHT},
};
use crate::recording::editor::model::background_render::render_gradient;
use crate::recording::editor::window::custom_wallpaper_popover::bitmap_to_surface;

use super::motion_mode::MotionRuntime;
use super::motion_render::{paint_card_shadow, paint_motion_scene_shadow, MotionStage};

/// Working width for the on-screen gradient fill. A gradient is smooth, so the
/// paint stretches this across the canvas; matching the video editor's preview
/// texture keeps a stop drag cheap, and a drag rebuilds it every frame.
const GRADIENT_PREVIEW_WIDTH: u32 = 480;

/// Cache key for the background surface: the fill style, the blurred-screenshot
/// revision it was built from, the Appearance blur, and which wallpaper pixels
/// the surface came from (thumbnail flag plus size), so a landed full-size
/// decode rebuilds instead of leaving the thumbnail stretched. The blur's
/// quality tier is tracked separately, as the edge the surface was built at, so
/// a burst can bake a cheap blur without invalidating an already-crisp surface.
type BackgroundSignature = (BackgroundStyle, Option<u64>, u64, Option<(bool, i32, i32)>);

/// Surface caches shared by the canvas draw path (working image, background).
#[derive(Clone)]
pub(super) struct CanvasRenderCaches {
    pub working_surface: Rc<RefCell<Option<ImageSurface>>>,
    pub working_revision: Rc<Cell<u64>>,
    pub background_surface: Rc<RefCell<Option<ImageSurface>>>,
    pub background_signature: Rc<RefCell<Option<BackgroundSignature>>>,
    /// Long edge the cached background surface was blurred at. `f64::INFINITY`
    /// marks a full-resolution/unblurred surface. A burst only upgrades it, so
    /// a drag never re-blurs a surface that is already crisp enough.
    pub background_edge: Rc<Cell<f64>>,
}

impl CanvasRenderCaches {
    pub(super) fn new() -> Self {
        Self {
            working_surface: Rc::new(RefCell::new(None)),
            working_revision: Rc::new(Cell::new(0)),
            background_surface: Rc::new(RefCell::new(None)),
            background_signature: Rc::new(RefCell::new(None)),
            background_edge: Rc::new(Cell::new(f64::INFINITY)),
        }
    }
}

/// How long the canvas keeps repainting with the cheap image filter after the
/// last Appearance interaction. Long enough to cover the gaps between pointer
/// events in a drag, short enough that the crisp resting frame lands
/// immediately after the pointer stops.
const INTERACTIVE_BURST_TAIL: Duration = Duration::from_millis(150);

/// A burst of pointer-rate Appearance edits.
///
/// The canvas already paints with a cheap filter while a canvas drag is in
/// flight (`EditorState`'s drag fields). A gradient-stop drag, an Appearance
/// slider, or a preset click goes through the inspector instead, so nothing in
/// `EditorState` moves and every repaint paid the quality resample of the
/// working image — the same tens-of-milliseconds blit that made canvas drags
/// stutter before the interactive filter existed. The burst counts those
/// inspector edits: `touch` on every interaction keeps the cheap filter up,
/// and one scheduled wake-up repaints once the edits stop, so the resting
/// frame is crisp again.
#[derive(Clone)]
pub(super) struct InteractivePreview {
    /// While this is in the future the canvas is interactive.
    deadline: Rc<Cell<Option<Instant>>>,
    /// One wake-up is in flight; further touches only push the deadline out.
    scheduled: Rc<Cell<bool>>,
    tail: Duration,
    /// Runs the resting quality repaint when the burst goes quiet.
    queue_draw: Rc<dyn Fn()>,
}

impl InteractivePreview {
    pub(super) fn new(queue_draw: Rc<dyn Fn()>) -> Self {
        Self::with_tail(INTERACTIVE_BURST_TAIL, queue_draw)
    }

    /// The testable seat: a short tail keeps the expiry test quick.
    fn with_tail(tail: Duration, queue_draw: Rc<dyn Fn()>) -> Self {
        Self {
            deadline: Rc::new(Cell::new(None)),
            scheduled: Rc::new(Cell::new(false)),
            tail,
            queue_draw,
        }
    }

    /// Record an edit that repaints the preview. The caller still queues its
    /// own draw; this only keeps that draw on the cheap filter and arranges
    /// the quality repaint that follows.
    pub(super) fn touch(&self) {
        self.deadline.set(Some(Instant::now() + self.tail));
        if !self.scheduled.replace(true) {
            schedule_burst_tail(
                self.deadline.clone(),
                self.scheduled.clone(),
                self.tail,
                self.queue_draw.clone(),
            );
        }
    }

    /// Whether a burst is in flight. Read from the canvas draw function.
    pub(super) fn active(&self) -> bool {
        self.deadline.get().is_some_and(|at| at > Instant::now())
    }
}

/// Wake up once the burst goes quiet. While edits keep arriving the deadline
/// moves, so the callback re-arms instead of painting: during a drag the
/// interaction itself is already queueing frames, and this timer only owes the
/// one quality frame that follows the last edit.
fn schedule_burst_tail(
    deadline: Rc<Cell<Option<Instant>>>,
    scheduled: Rc<Cell<bool>>,
    tail: Duration,
    queue_draw: Rc<dyn Fn()>,
) {
    gtk4::glib::timeout_add_local_once(tail, move || {
        let now = Instant::now();
        match deadline.get() {
            Some(at) if at > now => {
                schedule_burst_tail(deadline, scheduled, at - now, queue_draw);
            }
            _ => {
                scheduled.set(false);
                queue_draw();
            }
        }
    });
}

/// Inputs for installing the canvas `set_draw_func`.
pub(super) struct CanvasDrawInputs<'a> {
    pub state: &'a Arc<Mutex<EditorState>>,
    pub transform: &'a Arc<Mutex<ViewTransform>>,
    pub docked_inset: &'a DockedBarInset,
    pub drawing_area: &'a DrawingArea,
    pub zoom_level: &'a Rc<Cell<f64>>,
    pub undo_btn: &'a Button,
    pub redo_btn: &'a Button,
    pub delete_selected_btn: &'a Button,
    pub canvas_padding: i32,
    pub prefers_dark: bool,
    pub caches: &'a CanvasRenderCaches,
    /// The Appearance burst: keeps pointer-rate inspector edits on the cheap
    /// image filter, then repaints once they stop.
    pub interactive_preview: &'a InteractivePreview,
    pub wallpaper_cache: &'a Rc<RefCell<HashMap<PathBuf, ImageSurface>>>,
    /// The shared Motion runtime; its `background_surface` is where the
    /// Appearance inspector already decoded the selected wallpaper.
    pub motion_runtime: &'a Rc<RefCell<MotionRuntime>>,
}

/// Install the canvas draw function. Releases `EditorState` before Cairo work.
pub(super) fn install_canvas_draw_func(input: CanvasDrawInputs<'_>) {
    let CanvasDrawInputs {
        state,
        transform,
        docked_inset,
        drawing_area,
        zoom_level,
        undo_btn,
        redo_btn,
        delete_selected_btn,
        canvas_padding,
        prefers_dark,
        caches,
        interactive_preview,
        wallpaper_cache,
        motion_runtime,
    } = input;

    let state_draw = state.clone();
    let transform_draw = transform.clone();
    let docked_inset_draw = docked_inset.clone();
    let zoom_level_draw = zoom_level.clone();
    let undo_btn_draw = undo_btn.clone();
    let redo_btn_draw = redo_btn.clone();
    let delete_selected_btn_draw = delete_selected_btn.clone();
    let working_surface = caches.working_surface.clone();
    let working_revision = caches.working_revision.clone();
    let background_surface = caches.background_surface.clone();
    let background_signature_cache = caches.background_signature.clone();
    let background_edge = caches.background_edge.clone();
    let canvas_padding_draw = canvas_padding as f64;
    let interactive_preview = interactive_preview.clone();
    let wallpaper_cache = wallpaper_cache.clone();
    let motion_runtime = motion_runtime.clone();
    drawing_area.set_draw_func(move |_, context, width, height| {
        // IMPORTANT: do not hold the state mutex while performing cairo drawing.
        // The async effects pipeline also locks this mutex on the GTK thread to apply results;
        // holding it here can cause UI stalls/deadlocks.
        let (
            can_undo,
            can_redo,
            can_delete,
            working_image,
            working_image_revision,
            actions,
            draft_action,
            background_style,
            background_padding,
            background_aspect_ratio,
            background_insert,
            background_alignment,
            background_shadow,
            shadow_opacity,
            shadow_blur,
            shadow_offset_x,
            shadow_offset_y,
            background_corner_radius,
            background_noise,
            background_blur,
            border_thickness,
            border_color,
            frame_style,
            scene_shadow,
            selected_tool,
            selected_action,
            select_resize_handle,
            active_text_bounds,
            active_text_input,
            active_text_drag_handle,
            text_font_family,
            text_size,
            hovered_text_action_index,
            arrow_editing_controls,
            interactive_frame,
        ) = {
            let st = state_draw.lock().unwrap();
            let (can_undo, can_redo) = st.history_availability();
            (
                can_undo,
                can_redo,
                st.can_remove_selected_action(),
                Arc::clone(&st.working_image),
                st.working_image_revision,
                st.actions.clone(),
                st.draft_action(),
                st.background_style.clone(),
                st.background_padding,
                st.background_aspect_ratio,
                st.background_insert,
                st.background_alignment,
                st.background_shadow,
                st.shadow_opacity,
                st.shadow_blur,
                st.shadow_offset_x,
                st.shadow_offset_y,
                st.background_corner_radius,
                st.background_noise,
                st.background_blur,
                st.border_thickness,
                st.border_color,
                st.frame_style,
                st.scene_shadow.clone(),
                st.selected_tool,
                st.selected_action().cloned(),
                st.select_resize_handle,
                st.active_text_bounds.clone(),
                st.active_text_input.clone(),
                st.active_text_drag_handle.clone(),
                st.text_font_family.clone(),
                st.text_size,
                st.hovered_text_action_index,
                st.arrow_editing_controls,
                // A drag or draft is repainting at pointer rate: that frame has to
                // stay cheap, so it cannot use the quality image resample. An
                // Appearance burst (a stop drag, a slider, a preset click) is
                // the same kind of frame even though no canvas drag is running.
                st.select_drag_anchor.is_some()
                    || st.drag_start.is_some()
                    || st.active_text_is_dragging
                    || st.arrow_control_dragging.is_some()
                    || interactive_preview.active(),
            )
        };

        undo_btn_draw.set_sensitive(can_undo);
        redo_btn_draw.set_sensitive(can_redo);
        delete_selected_btn_draw.set_sensitive(can_delete);

        let image_width = working_image.width() as f64;
        let image_height = working_image.height() as f64;
        let mut virtual_w = image_width;
        let mut virtual_h = image_height;
        let mut draw_scale_factor = 1.0;
        let mut background_scale_factor = 1.0;
        let mut background_layout = None;

        let has_background = background_style != BackgroundStyle::None;
        // Frame styles (Retro window, outside borders) need canvas beyond the
        // bare screenshot even with no wallpaper, on a transparent surround.
        let frame_without_background =
            !has_background && frame_needs_canvas(frame_style, border_thickness);
        if has_background {
            let layout = BackgroundComposition::new(image_width, image_height)
                .with_style(background_style.clone())
                .with_padding(background_padding)
                .with_shadow(background_shadow)
                .with_shadow_profile(
                    shadow_opacity,
                    shadow_blur,
                    shadow_offset_x,
                    shadow_offset_y,
                )
                .with_insert(background_insert)
                .with_alignment(background_alignment)
                .with_corner_radius(background_corner_radius)
                .with_aspect_ratio(background_aspect_ratio)
                .with_frame_style(frame_style)
                .with_frame_border_thickness(border_thickness)
                .compute();
            virtual_w = layout.canvas_width;
            virtual_h = layout.canvas_height;
            draw_scale_factor = layout.draw_scale;
            background_scale_factor = layout.scale_factor;
            background_layout = Some(layout);
        } else if frame_without_background {
            let layout = BackgroundComposition::new(image_width, image_height)
                .with_style(BackgroundStyle::None)
                .with_padding(0.0)
                .with_alignment(background_alignment)
                .with_corner_radius(background_corner_radius)
                .with_aspect_ratio(background_aspect_ratio)
                .with_frame_style(frame_style)
                .with_frame_border_thickness(border_thickness)
                .compute();
            virtual_w = layout.canvas_width;
            virtual_h = layout.canvas_height;
            draw_scale_factor = layout.draw_scale;
            background_scale_factor = layout.scale_factor;
            background_layout = Some(layout);
        }

        // Docked tool bars reserve real space: the image is drawn below them so a
        // bar can never sit on top of the canvas (see `DockedBarInset`).
        let toolbar_clearance = f64::from(EDITOR_TOP_CHROME_HEIGHT) + docked_inset_draw.px();
        let top_pad = canvas_padding_draw + toolbar_clearance;
        let side_pad = canvas_padding_draw;
        let (overflow_left, overflow_top, overflow_right, overflow_bottom) = (0.0, 0.0, 0.0, 0.0);

        let view_width = (width as f64 - side_pad * 2.0 - overflow_left - overflow_right).max(1.0);
        let view_height =
            (height as f64 - top_pad - side_pad - overflow_top - overflow_bottom).max(1.0);

        let scale = (view_width / virtual_w)
            .min(view_height / virtual_h)
            .min(1.0_f64)
            * zoom_level_draw.get().max(0.1_f64);
        let draw_width = virtual_w * scale;
        let draw_height = virtual_h * scale;
        // Centre the preview in the pane the user sees. `top_pad` is a floor it
        // may not rise above, not the line it is centred on, so spare vertical
        // space is split evenly instead of piling up between the toolbar and the
        // image. Docked tool bars still push the image down, but only when it
        // cannot stay centred without riding under them.
        let canvas_offset_x = side_pad
            + super::canvas::centred_viewport_x(draw_width, view_width, 0.0)
            + overflow_left;
        let canvas_offset_y =
            super::canvas::centred_canvas_top(f64::from(height), draw_height, top_pad, side_pad)
                + overflow_top;
        let mut t = ViewTransform {
            scale,
            offset_x: canvas_offset_x,
            offset_y: canvas_offset_y,
            image_width,
            image_height,
            has_background,
            canvas_offset_x,
            canvas_offset_y,
            canvas_scale: scale,
            canvas_width: virtual_w,
            canvas_height: virtual_h,
            image_rect_x: 0.0,
            image_rect_y: 0.0,
            canvas_draw_scale: draw_scale_factor,
        };

        let canvas_t = t;

        // The rectangle the background fill covers is the static scene, the
        // same rectangle the Motion stage bounds the card in. Scene Shadows
        // shade it in both passes.
        let scene_stage = MotionStage::rect_at(
            canvas_t.offset_x,
            canvas_t.offset_y,
            virtual_w * canvas_t.scale,
            virtual_h * canvas_t.scale,
        );

        // Quality filter while the canvas rests, cheap one while a drag or draft is
        // repainting at pointer rate (see `editor_interactive_image_filter`).
        let pick_image_filter = |scale: f64| {
            if interactive_frame {
                crate::capture::editor::render::editor_interactive_image_filter()
            } else {
                crate::capture::editor::render::editor_image_filter_for_scale(scale)
            }
        };

        context.set_operator(gtk4::cairo::Operator::Source);
        draw_canvas_checkerboard_background(context, width, height, None, !prefers_dark);

        if has_background || frame_without_background {
            context.set_operator(gtk4::cairo::Operator::Over);
            // Wallpaper fill only exists with a background; frame-only mode
            // keeps the checkerboard surround and just adds the window.
            if has_background {
                let current_style = background_style.clone();
                // Pixels the Motion runtime already decoded for this wallpaper.
                // Reusing them keeps a multi-megapixel decode off the UI thread;
                // carrying which pixels they are in the signature means a landed
                // full-size decode replaces the cached thumbnail instead of
                // leaving it stretched (and blurred) over the whole canvas.
                let motion_wallpaper = match &current_style {
                    BackgroundStyle::Wallpaper(path) => {
                        let runtime = motion_runtime.borrow();
                        let is_selected = runtime.background_surface_path.as_deref()
                            == Some(path.to_string_lossy().as_ref());
                        is_selected
                            .then(|| {
                                runtime
                                    .background_surface
                                    .clone()
                                    .map(|surface| (surface, runtime.background_surface_is_preview))
                            })
                            .flatten()
                    }
                    _ => None,
                };
                let current_background_signature = (
                    current_style.clone(),
                    if matches!(current_style, BackgroundStyle::Blurred(_)) {
                        Some(working_image_revision)
                    } else {
                        None
                    },
                    background_blur.to_bits(),
                    motion_wallpaper.as_ref().map(|(surface, is_preview)| {
                        (*is_preview, surface.width(), surface.height())
                    }),
                );
                // The blur is baked into the cached surface, so the working
                // resolution only has to satisfy what the canvas shows on screen.
                // It is also the one expensive step here: the box blur costs
                // O(pixels) and the fill is then upscaled across the canvas, so
                // the preview bounds it rather than paying the export's
                // full-resolution cost on every edit. The burst bakes a small
                // one; the resting frame a crisper one.
                let canvas_edge = virtual_w * canvas_t.scale;
                let blur_work_edge = if interactive_frame {
                    canvas_edge.clamp(96.0, 128.0)
                } else {
                    canvas_edge.clamp(256.0, 512.0)
                };
                let needs_background_surface = !matches!(
                    current_style,
                    BackgroundStyle::None | BackgroundStyle::PlainColor(_)
                );
                // A burst may bake the blur smaller than the resting frame needs;
                // the burst-tail repaint upgrades it. But a surface already crisp
                // enough for this frame is never rebuilt, so dragging a control
                // that does not touch the blur (padding, radius, noise) costs
                // nothing here.
                let background_edge_draw = background_edge.clone();
                let blur_upgrade =
                    needs_background_surface && blur_work_edge > background_edge_draw.get() + 0.5;
                // Edge the surface we are about to build actually ends up at:
                // the source's own long edge caps the blur, and a zero blur
                // keeps the source as-is (full resolution, nothing to upgrade).
                let edge_for = |source: &ImageSurface| -> f64 {
                    if background_blur <= 0.001 {
                        f64::INFINITY
                    } else {
                        blur_work_edge.min(f64::from(source.width().max(source.height())))
                    }
                };
                let mut built_edge = background_edge_draw.get();
                let mut bg_cache = background_surface.borrow_mut();
                let mut bg_signature_cache = background_signature_cache.borrow_mut();

                if bg_signature_cache.as_ref() != Some(&current_background_signature)
                    || (needs_background_surface && bg_cache.is_none())
                    || blur_upgrade
                {
                    // A Wallpaper whose pixels are not decoded yet keeps the
                    // previous surface and leaves the signature stale, so the next
                    // draw retries instead of caching a blank background.
                    let mut background_resolved = true;
                    if let BackgroundStyle::Gradient(gradient) = &current_style {
                        // Rasterize the shared spec, then blur it like any
                        // other fill. The fill covers the whole scene, so the
                        // raster takes the scene's aspect and the paint below
                        // stretches it across the canvas.
                        let aspect = virtual_h / virtual_w.max(1.0);
                        let height = ((f64::from(GRADIENT_PREVIEW_WIDTH) * aspect).round() as u32)
                            .clamp(1, 960);
                        let surface = bitmap_to_surface(&render_gradient(
                            gradient,
                            GRADIENT_PREVIEW_WIDTH,
                            height,
                        ));
                        *bg_cache = Some(blurred_preview_background(
                            &surface,
                            background_blur,
                            virtual_w,
                            blur_work_edge,
                        ));
                        built_edge = edge_for(&surface);
                    } else if let BackgroundStyle::Wallpaper(path) = &current_style {
                        // The Appearance inspector decoded this wallpaper for its
                        // own preview; reuse those pixels rather than running a
                        // multi-megapixel JPEG decode on the UI thread. Its
                        // thumbnail paints immediately and the signature above
                        // rebuilds once the full decode lands.
                        if let Some((surface, _)) = motion_wallpaper.as_ref() {
                            *bg_cache = Some(blurred_preview_background(
                                surface,
                                background_blur,
                                virtual_w,
                                blur_work_edge,
                            ));
                            built_edge = edge_for(surface);
                        } else if let Some(surface) = wallpaper_cache.borrow().get(path) {
                            *bg_cache = Some(blurred_preview_background(
                                surface,
                                background_blur,
                                virtual_w,
                                blur_work_edge,
                            ));
                            built_edge = edge_for(surface);
                        } else {
                            // Still decoding off-thread; it queues a redraw when it
                            // lands. ponytail: never sync-decode on the UI thread.
                            background_resolved = false;
                        }
                    } else if let BackgroundStyle::PlainColor(_color) = &current_style {
                        *bg_cache = None;
                        built_edge = f64::INFINITY;
                    } else if let BackgroundStyle::Blurred(blur_idx) = &current_style {
                        let (bw, bh) = working_image.dimensions();

                        // Optimization: Downsample for background blur to save CPU.
                        // For very long webpage screenshots, resize directly from the
                        // source image to avoid cloning the full-size buffer first.
                        let max_dim = 800u32;
                        let mut blurred_bg = if bw > max_dim || bh > max_dim {
                            let scale = max_dim as f64 / (bw.max(bh) as f64);
                            image::imageops::resize(
                                &*working_image,
                                (bw as f64 * scale) as u32,
                                (bh as f64 * scale) as u32,
                                image::imageops::FilterType::Triangle,
                            )
                        } else {
                            (*working_image).clone()
                        };

                        let (nbw, nbh) = blurred_bg.dimensions();
                        // The style picks the base level; the Appearance slider
                        // adds to it, in canvas pixels converted to this working
                        // image.
                        let blur_radius = match blur_idx {
                            0 => 10.0,
                            1 => 35.0,
                            2 => 80.0,
                            _ => 20.0,
                        } + background_blur.clamp(0.0, 1.0)
                            * BACKGROUND_BLUR_MAX_RADIUS
                            * (nbw as f64 / virtual_w.max(1.0));
                        crate::capture::editor::render::apply_blur_rect(
                            &mut blurred_bg,
                            Rect {
                                x: 0,
                                y: 0,
                                width: nbw as i32,
                                height: nbh as i32,
                            },
                            blur_radius,
                            false,
                        );
                        *bg_cache = rgba_image_to_surface(&blurred_bg);
                        // The screenshot blur is not tiered; treat it as crisp.
                        built_edge = f64::INFINITY;
                    }
                    if background_resolved {
                        *bg_signature_cache = Some(current_background_signature);
                        background_edge_draw.set(built_edge);
                    }
                }

                if let Some(surface) = bg_cache.as_ref() {
                    let _ = context.save();
                    let sw = surface.width() as f64;
                    let sh = surface.height() as f64;
                    context.translate(canvas_t.offset_x, canvas_t.offset_y);
                    context.scale(
                        (virtual_w * canvas_t.scale) / sw,
                        (virtual_h * canvas_t.scale) / sh,
                    );
                    context.set_source_surface(surface, 0.0, 0.0).unwrap();
                    context
                        .source()
                        .set_filter(pick_image_filter(canvas_t.scale));
                    let _ = context.paint();
                    let _ = context.restore();
                } else if let BackgroundStyle::PlainColor(color) = &current_style {
                    context.set_source_rgba(color.r, color.g, color.b, color.a);
                    context.rectangle(
                        canvas_t.offset_x,
                        canvas_t.offset_y,
                        virtual_w * canvas_t.scale,
                        virtual_h * canvas_t.scale,
                    );
                    let _ = context.fill();
                }
            }

            // Background grain belongs to the fill, so it lands under the card,
            // its backings, and its shadow: the screenshot stays clean while the
            // fill reads as film grain, matching the Motion preview and export.
            if has_background {
                paint_background_noise(
                    &context,
                    canvas_t.offset_x,
                    canvas_t.offset_y,
                    virtual_w * canvas_t.scale,
                    virtual_h * canvas_t.scale,
                    background_noise,
                );
                // Scene Shadows underlay: over the fill, under the card, its
                // backings and its drop shadow. No fill means no scene to read
                // the shading against (the inspector says so), so it is skipped.
                paint_motion_scene_shadow(&context, scene_stage, &scene_shadow, true);
            }

            if let Some(layout) = background_layout.as_ref() {
                t.offset_x = canvas_t.offset_x + layout.image_rect.x * canvas_t.scale;
                t.offset_y = canvas_t.offset_y + layout.image_rect.y * canvas_t.scale;
                t.scale = canvas_t.scale * layout.draw_scale;
                t.image_rect_x = layout.image_rect.x;
                t.image_rect_y = layout.image_rect.y;
                t.canvas_draw_scale = layout.draw_scale;
                t.canvas_offset_x = canvas_t.offset_x;
                t.canvas_offset_y = canvas_t.offset_y;
                t.canvas_scale = canvas_t.scale;
                t.canvas_width = layout.canvas_width;
                t.canvas_height = layout.canvas_height;
                t.has_background = has_background;

                // Stack presets: flat backing sheets behind the card, offset
                // up-left like stacked prints. Farthest sheet first.
                {
                    let spec = frame_style.spec();
                    let backings = [spec.backing1, spec.backing2];
                    if backings.iter().any(|b| b.is_some()) {
                        let bw = image_width * t.scale;
                        let bh = image_height * t.scale;
                        let br = background_corner_radius * background_scale_factor * t.scale;
                        // Backing offsets are fixed canvas px (same at any image
                        // size); canvas px -> view px via the canvas scale.
                        let unit = canvas_t.scale;
                        for backing in backings.into_iter().flatten() {
                            if bw <= 1.0 || bh <= 1.0 {
                                continue;
                            }
                            // Center-pivot sheets fan diagonally (Stack);
                            // the rest pivot at the hidden bottom-right corner
                            // and fan top-left (Stack2).
                            let _ = context.save();
                            if backing.center_pivot {
                                context.translate(
                                    t.offset_x + bw / 2.0 + backing.offset_x * unit,
                                    t.offset_y + bh / 2.0 + backing.offset_y * unit,
                                );
                                context.rotate(backing.rotation_deg.to_radians());
                                context.translate(-bw / 2.0, -bh / 2.0);
                            } else {
                                context.translate(
                                    t.offset_x + backing.offset_x * unit + bw,
                                    t.offset_y + backing.offset_y * unit + bh,
                                );
                                context.rotate(backing.rotation_deg.to_radians());
                                context.translate(-bw, -bh);
                            }
                            draw_rounded_rect_path(context, bw, bh, br.max(0.0), 0.0);
                            context.set_source_rgba(
                                backing.color.r,
                                backing.color.g,
                                backing.color.b,
                                backing.color.a,
                            );
                            let _ = context.fill();
                            let _ = context.restore();
                        }
                    }
                }

                if layout.shadow.is_some() {
                    // Shared with the Motion preview/export: one painter, and
                    // the Appearance Shadow controls are stage/screen pixels in
                    // both, so the card drop shadow looks the same in either
                    // mode. The static canvas draws at canvas resolution and
                    // scales to the view, so the card corners are converted to
                    // view space here instead of rasterizing the shadow at
                    // canvas resolution and scaling it down with everything
                    // else (which is what made the two previews diverge).
                    let card_w = image_width * t.scale;
                    let card_h = image_height * t.scale;
                    let corners = [
                        (t.offset_x, t.offset_y),
                        (t.offset_x + card_w, t.offset_y),
                        (t.offset_x + card_w, t.offset_y + card_h),
                        (t.offset_x, t.offset_y + card_h),
                    ];
                    paint_card_shadow(
                        context,
                        scene_stage,
                        corners,
                        background_corner_radius * background_scale_factor * t.scale,
                        shadow_opacity,
                        shadow_blur,
                        (shadow_offset_x, shadow_offset_y),
                    );
                }
            } else {
                t.scale = canvas_t.scale * draw_scale_factor;
            }

            let rect_w = image_width * t.scale;
            let rect_h = image_height * t.scale;
            let corner_r = background_corner_radius * background_scale_factor * t.scale;

            let _ = context.save();
            context.translate(t.offset_x, t.offset_y);
            draw_rounded_rect_path(context, rect_w, rect_h, corner_r, 0.0);
            context.clip();
            context.translate(-t.offset_x, -t.offset_y);
        }
        context.set_operator(gtk4::cairo::Operator::Over);
        *transform_draw.lock().unwrap() = t;

        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);

        if working_revision.get() != working_image_revision || working_surface.borrow().is_none() {
            *working_surface.borrow_mut() = rgba_image_to_surface(&working_image);
            working_revision.set(working_image_revision);
        }

        if let Some(surface) = working_surface.borrow().as_ref() {
            crate::capture::editor::render::paint_surface_with_filter(
                context,
                surface,
                0.0,
                0.0,
                pick_image_filter(t.scale),
            );
        } else {
            draw_rgba_to_context(context, &working_image);
        }
        let _ = context.restore();

        // Annotations paint above the background/wallpaper (not clipped to the
        // screenshot rounded rect) so text/shapes placed on the padding stay
        // visible instead of slipping underneath the wallpaper. Same for a
        // frameless surround: annotations sit above the window and border.
        if has_background || frame_without_background {
            let _ = context.restore();
        }
        // Frame style preset: outside border drawn *around* the screenshot on
        // top of any background/wallpaper, below the annotations. The stroke is
        // centered on an expanded path so the inner edge aligns with the image
        // edge and the outer edge carries the radius outward.
        {
            let bw = image_width * t.scale;
            let bh = image_height * t.scale;
            let br = background_corner_radius * background_scale_factor * t.scale;
            let unit = background_scale_factor * t.scale;
            let spec = frame_style.spec();
            if let Some(liquid) =
                crate::capture::editor::render::LiquidFrame::resolve(&spec, border_thickness, unit)
            {
                // Liquid Glass: gradients instead of flat strokes.
                let path = |path_context: &gtk4::cairo::Context, expand: f64| {
                    crate::capture::editor::render::rounded_rect_path(
                        path_context,
                        t.offset_x - expand,
                        t.offset_y - expand,
                        bw + expand * 2.0,
                        bh + expand * 2.0,
                        if br <= 0.0 { 0.0 } else { br + expand },
                    );
                };
                liquid.paint(context, t.offset_y, t.offset_y + bh, path);
            } else {
                // Inset borders paint fully inside the image edge.
                if spec.inset_border && border_thickness > 0.0 {
                    let lw = (border_thickness * unit).max(0.0);
                    if lw > 0.01 && bw - lw > 1.0 && bh - lw > 1.0 {
                        let e = lw / 2.0;
                        let _ = context.save();
                        context.translate(t.offset_x + e, t.offset_y + e);
                        draw_rounded_rect_path(
                            context,
                            bw - e * 2.0,
                            bh - e * 2.0,
                            (br - e).max(0.0),
                            0.0,
                        );
                        context.set_source_rgba(
                            border_color.r,
                            border_color.g,
                            border_color.b,
                            border_color.a,
                        );
                        context.set_line_width(lw);
                        let _ = context.stroke();
                        let _ = context.restore();
                    }
                }
                let mut expand = 0.0;
                let stroke_outside = |thickness: f64,
                                      radius: f64,
                                      r: f64,
                                      g: f64,
                                      b: f64,
                                      a: f64,
                                      extra: f64| {
                    let lw = (thickness * unit).max(0.0);
                    if lw <= 0.01 {
                        return extra;
                    }
                    let e = extra + lw / 2.0;
                    if bw + e * 2.0 <= 1.0 || bh + e * 2.0 <= 1.0 {
                        return extra;
                    }
                    let _ = context.save();
                    context.translate(t.offset_x - e, t.offset_y - e);
                    // A zero radius stays a sharp mitered frame: the expanded
                    // path must not inherit half the line width as rounding.
                    let path_radius = if radius <= 0.0 { 0.0 } else { radius + e };
                    draw_rounded_rect_path(context, bw + e * 2.0, bh + e * 2.0, path_radius, 0.0);
                    context.set_source_rgba(r, g, b, a);
                    context.set_line_width(lw);
                    let _ = context.stroke();
                    let _ = context.restore();
                    extra + lw
                };
                if border_thickness > 0.0 && !spec.inset_border {
                    expand = stroke_outside(
                        border_thickness,
                        br,
                        border_color.r,
                        border_color.g,
                        border_color.b,
                        border_color.a,
                        expand,
                    );
                }
                for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
                    // Preset main border already applied via border_* fields;
                    // only paint accent strokes that extend beyond it.
                    let main_matches = (outer.thickness - spec.border_thickness).abs()
                        < f64::EPSILON
                        && outer.gap == 0.0;
                    if main_matches {
                        continue;
                    }
                    expand += outer.gap * unit;
                    expand = stroke_outside(
                        outer.thickness,
                        br,
                        outer.color.r,
                        outer.color.g,
                        outer.color.b,
                        outer.color.a,
                        expand,
                    );
                }
            }
        }
        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);

        let editing_action_index = active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index);
        for (index, action) in actions.iter().enumerate() {
            if Some(index) == editing_action_index {
                continue;
            }
            if matches!(
                action,
                AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
            ) {
                continue;
            }
            draw_annotation_action(context, action);
        }

        if let Some(draft) = draft_action {
            draw_draft_action(context, &draft);
        }

        // Scene Shadows overlay: above the card and its annotations, matching
        // the Motion overlay pass. Handles and edit outlines are editor
        // chrome, not content, so they must not dim under the shade: drop back
        // to view space for the pass, then carry on in image space.
        let _ = context.restore();
        if has_background {
            paint_motion_scene_shadow(&context, scene_stage, &scene_shadow, false);
        }
        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);

        // In Text tool mode: draw hover outline for the text action under the cursor.
        if selected_tool == Tool::Text && active_text_bounds.is_none() {
            if let Some(hover_idx) = hovered_text_action_index {
                if let Some(AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                }) = actions.get(hover_idx)
                {
                    let available_width = max_width.unwrap_or_else(|| {
                        (working_image.width() as f64 - position.x).max(font.size * 1.8)
                    });
                    let mut text_bounds =
                        text_action_bounds(context, *position, text, font, Some(available_width));
                    if has_background {
                        let (min_bx, min_by, max_bx, max_by) = t.canvas_bounds_in_image_coords();
                        let min_ix = min_bx.round() as i32;
                        let min_iy = min_by.round() as i32;
                        let max_ix = (max_bx.round() as i32 - text_bounds.rect.width).max(min_ix);
                        let max_iy = (max_by.round() as i32 - text_bounds.rect.height).max(min_iy);
                        text_bounds.rect.x = text_bounds.rect.x.clamp(min_ix, max_ix);
                        text_bounds.rect.y = text_bounds.rect.y.clamp(min_iy, max_iy);
                    } else {
                        text_bounds.rect.x = text_bounds.rect.x.clamp(
                            0,
                            (working_image.width() as i32 - text_bounds.rect.width).max(0),
                        );
                        text_bounds.rect.y = text_bounds.rect.y.clamp(
                            0,
                            (working_image.height() as i32 - text_bounds.rect.height).max(0),
                        );
                    }
                    text_bounds.sync_handles();
                    draw_text_edit_border(context, &text_bounds, t.scale);
                }
            }
        }

        if let Some(selected_action) = selected_action.as_ref() {
            // Draw border + handles for a selected Text action in both
            // Select tool mode and Text tool mode (e.g. during drag-to-move).
            let show_text_handles = (selected_tool == Tool::Select || selected_tool == Tool::Text)
                && active_text_bounds.is_none();

            if show_text_handles {
                if let AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                } = selected_action
                {
                    let available_width = max_width.unwrap_or_else(|| {
                        (working_image.width() as f64 - position.x).max(font.size * 1.8)
                    });
                    let mut text_bounds =
                        text_action_bounds(context, *position, text, font, Some(available_width));
                    if has_background {
                        let (min_bx, min_by, max_bx, max_by) = t.canvas_bounds_in_image_coords();
                        let min_ix = min_bx.round() as i32;
                        let min_iy = min_by.round() as i32;
                        let max_ix = (max_bx.round() as i32 - text_bounds.rect.width).max(min_ix);
                        let max_iy = (max_by.round() as i32 - text_bounds.rect.height).max(min_iy);
                        text_bounds.rect.x = text_bounds.rect.x.clamp(min_ix, max_ix);
                        text_bounds.rect.y = text_bounds.rect.y.clamp(min_iy, max_iy);
                    } else {
                        text_bounds.rect.x = text_bounds.rect.x.clamp(
                            0,
                            (working_image.width() as i32 - text_bounds.rect.width).max(0),
                        );
                        text_bounds.rect.y = text_bounds.rect.y.clamp(
                            0,
                            (working_image.height() as i32 - text_bounds.rect.height).max(0),
                        );
                    }
                    text_bounds.sync_handles();
                    draw_text_edit_border(context, &text_bounds, t.scale);
                    draw_text_edit_handles(context, &text_bounds, None, t.scale);
                }
            }

            if selected_tool == Tool::Select
                || selected_tool == Tool::Arrow
                || (matches!(selected_tool, Tool::Box | Tool::Circle)
                    && matches!(
                        selected_action,
                        AnnotationAction::Box { .. } | AnnotationAction::Circle { .. }
                    ))
                || (selected_tool == Tool::Obfuscate
                    && matches!(selected_action, AnnotationAction::Obfuscate { .. }))
                || (selected_tool == Tool::Focus
                    && matches!(selected_action, AnnotationAction::Focus { .. }))
                || (selected_tool == Tool::Number
                    && matches!(selected_action, AnnotationAction::Number { .. }))
            {
                if let AnnotationAction::Text { .. } = selected_action {
                    // Already handled above.
                } else if let AnnotationAction::Arrow {
                    start,
                    end,
                    stroke_size,
                    style,
                    control_points,
                    ..
                } = selected_action
                {
                    draw_arrow_selection_outline(
                        context,
                        *start,
                        *end,
                        *stroke_size,
                        *style,
                        control_points.clone(),
                        t.scale,
                    );
                } else if matches!(selected_action, AnnotationAction::Line { .. }) {
                    // Intentionally show no crop-like selection outline or handles for lines.
                } else {
                    let selection_padding = if matches!(
                        selected_action,
                        AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                    ) {
                        0.0
                    } else {
                        selection_hit_padding_for_scale(t.scale)
                    };
                    if let Some(bounds) =
                        action_bounds_with_padding(selected_action, selection_padding)
                    {
                        if !matches!(
                            selected_action,
                            AnnotationAction::Box { .. } | AnnotationAction::Circle { .. }
                        ) {
                            draw_selection_outline(context, bounds, t.scale);
                        }
                    }

                    let handles = action_resize_handles(selected_action);
                    if !handles.is_empty() {
                        draw_selection_handles(context, &handles, select_resize_handle, t.scale);
                    }
                }
            }

            // The active text edit overlay (border + handles) is drawn by the
            // unconditional block below, which also handles clamping and cursor
            // rendering. Do NOT draw it here a second time.
        }

        // Draw arrow control handles when: (a) editing controls are active, OR
        // (b) Arrow or Select tool is selected and an existing arrow is selected.
        let show_handles = arrow_editing_controls
            || ((selected_tool == Tool::Arrow || selected_tool == Tool::Select)
                && selected_action
                    .as_ref()
                    .map(|a| matches!(a, AnnotationAction::Arrow { .. }))
                    .unwrap_or(false));

        if show_handles {
            if let Some(AnnotationAction::Arrow {
                control_points: Some(handles),
                color,
                ..
            }) = selected_action.as_ref()
            {
                draw_arrow_control_handles(context, handles.clone(), *color, t.scale);
            }
        }

        // Draw active text edit overlay (border + handles)
        if let Some(bounds) = active_text_bounds.as_ref() {
            let mut bounds = bounds.clone();
            if has_background {
                let (min_bx, min_by, max_bx, max_by) = t.canvas_bounds_in_image_coords();
                let min_ix = min_bx.round() as i32;
                let min_iy = min_by.round() as i32;
                let max_ix = (max_bx.round() as i32 - bounds.rect.width).max(min_ix);
                let max_iy = (max_by.round() as i32 - bounds.rect.height).max(min_iy);
                bounds.rect.x = bounds.rect.x.clamp(min_ix, max_ix);
                bounds.rect.y = bounds.rect.y.clamp(min_iy, max_iy);
            } else {
                bounds.rect.x = bounds
                    .rect
                    .x
                    .clamp(0, (working_image.width() as i32 - bounds.rect.width).max(0));
                bounds.rect.y = bounds.rect.y.clamp(
                    0,
                    (working_image.height() as i32 - bounds.rect.height).max(0),
                );
            }
            bounds.sync_handles();
            if let Some(input) = active_text_input.as_ref() {
                let font = crate::capture::editor::types::FontSettings {
                    family: text_font_family.clone(),
                    size: text_size,
                    style: crate::capture::editor::types::FontStyle::Normal,
                    decoration: crate::capture::editor::types::TextDecoration::None,
                    alignment: crate::capture::editor::types::TextAlignment::Left,
                };
                draw_active_text_input(
                    context,
                    &bounds,
                    &input.text,
                    input.cursor_position,
                    input.cursor_visible,
                    input.color,
                    &font,
                );
            }
            draw_text_edit_border(context, &bounds, t.scale);
            draw_text_edit_handles(context, &bounds, active_text_drag_handle.clone(), t.scale);
        }
        let _ = context.restore();
    });
}

/// Blur the background layer for the Appearance slider. The source may be a
/// surface the Motion runtime also holds, so it is never borrowed in place; if
/// the blur cannot be produced the unblurred fill is kept rather than dropping
/// the background entirely.
fn blurred_preview_background(
    source: &ImageSurface,
    amount: f64,
    canvas_width: f64,
    work_edge: f64,
) -> ImageSurface {
    if amount <= 0.001 {
        return source.clone();
    }
    blur_background_surface(source, amount, canvas_width, work_edge)
        .unwrap_or_else(|| source.clone())
}

fn draw_rounded_rect_path(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    corner_radius: f64,
    expansion: f64,
) {
    let r = corner_radius + expansion;
    let x = -expansion;
    let y = -expansion;
    let w = width + expansion * 2.0;
    let h = height + expansion * 2.0;

    // Same smooth outline as export; zero/negative radius stays a sharp rect.
    crate::capture::editor::render::rounded_rect_path(context, x, y, w, h, r);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_render_owns_draw_func_caches_and_lock_release() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("pub(super) struct CanvasRenderCaches")
                && production.contains("pub(super) fn install_canvas_draw_func(")
                && production.contains("drawing_area.set_draw_func(move |_, context, width, height| {")
                && production.contains("IMPORTANT: do not hold the state mutex while performing cairo drawing")
                && production.contains("draw_canvas_checkerboard_background")
                && production.contains("draw_annotation_action")
                && production.contains("draw_arrow_control_handles")
                && production.contains("AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }")
                && production.contains("0.0")
                && production.contains("paint_card_shadow(")
                && production.contains("fn draw_rounded_rect_path"),
            "canvas_render.rs must own render caches, set_draw_func, lock-release snapshot, and rounded-rect helper"
        );
    }

    /// The preview centres in the pane, and the toolbar/dock strip is a floor
    /// rather than the line it is centred on. Centring in the strip below the
    /// chrome is what left the lopsided gap the taller image editor exposed.
    #[test]
    fn static_canvas_centres_the_preview_in_the_pane_not_under_the_chrome() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("super::canvas::centred_canvas_top(")
                && production.contains("super::canvas::centred_viewport_x(")
                && !production.contains("initial_viewport_offset(")
                && !production.contains("canvas_offset_y = top_pad"),
            "the static canvas must centre the preview in the pane, clamping only to the \
             toolbar/dock floor",
        );
    }

    #[test]
    fn static_canvas_grains_the_fill_from_editor_state() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("st.background_noise")
                && production.contains("paint_background_noise("),
            "the static canvas must paint the fill grain from EditorState, like the Motion preview",
        );
    }

    #[test]
    fn static_canvas_blurs_the_fill_from_editor_state() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("st.background_blur")
                && production.contains("blur_background_surface("),
            "the static canvas must blur the fill from EditorState, like the Motion preview",
        );
    }

    /// Re-blurring the whole fill per slider tick stuttered next to the cheap
    /// controls around it. A live burst bakes the blur small and the tail
    /// upgrades it. The tier must ride the surface's edge, not the cache key, or
    /// every unrelated slider drag (padding, radius, noise) would re-blur the
    /// fill.
    #[test]
    fn background_blur_drops_resolution_during_a_live_burst() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("let blur_work_edge = if interactive_frame {")
                && production.contains("canvas_edge.clamp(96.0, 128.0)")
                && production.contains("canvas_edge.clamp(256.0, 512.0)")
                && production.contains("blur_work_edge > background_edge_draw.get() + 0.5")
                && production.contains("background_edge_draw.set(built_edge);"),
            "the blur tier must ride the surface edge, not invalidate the cache key",
        );
    }

    /// The card drop shadow is one painter shared with the Motion preview, so
    /// the Appearance Shadow controls look the same in both modes. Rasterizing
    /// the static shadow at canvas resolution and scaling it down was what made
    /// the two previews diverge; the static path must go through the shared
    /// painter in view space.
    #[test]
    fn static_canvas_uses_the_shared_motion_card_shadow() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("paint_card_shadow(")
                && production.contains("scene_stage")
                && production.contains("image_width * t.scale")
                && production
                    .contains("background_corner_radius * background_scale_factor * t.scale,")
                && !production.contains("render_shadow_layer("),
            "the static preview must paint its card shadow with the same painter Motion uses, \
             passing the card's corner radius so the shadow is rounded too",
        );
    }

    /// A stop drag repaints the static preview every frame, so the gradient it
    /// rasters must stay small. It carried the whole canvas edge to begin with,
    /// and that is what made the drag stutter next to the video editor.
    #[test]
    fn static_canvas_rasters_the_gradient_at_the_video_editors_preview_size() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("const GRADIENT_PREVIEW_WIDTH: u32 = 480;")
                && production.contains("render_gradient(")
                && production.contains("GRADIENT_PREVIEW_WIDTH,"),
            "the static gradient raster must be sized by GRADIENT_PREVIEW_WIDTH, matching the video editor's preview texture",
        );
    }

    #[test]
    fn interactive_drags_repaint_with_the_cheap_image_filter() {
        let source = include_str!("canvas_render.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("st.select_drag_anchor.is_some()")
                && production.contains("st.drag_start.is_some()")
                && production.contains("st.active_text_is_dragging")
                && production.contains("st.arrow_control_dragging.is_some()")
                && production.contains("interactive_preview.active()")
                && production.contains("editor_interactive_image_filter()")
                && production.contains("editor_image_filter_for_scale(scale)")
                && production.contains("pick_image_filter(canvas_t.scale)")
                && production.contains("pick_image_filter(t.scale)"),
            "Pointer-rate frames must blit with the cheap image filter; the resting frame keeps the quality one"
        );
    }

    /// The Appearance burst is what keeps a stop drag cheap: no `EditorState`
    /// drag field moves, so without it every drag frame paid the quality
    /// resample the interactive filter exists to avoid.
    #[test]
    fn an_appearance_burst_keeps_frames_cheap_until_the_tail_expires() {
        let draws = Rc::new(Cell::new(0));
        let preview = InteractivePreview::with_tail(
            Duration::from_millis(20),
            Rc::new({
                let draws = draws.clone();
                move || draws.set(draws.get() + 1)
            }),
        );
        assert!(!preview.active(), "a resting canvas is not interactive");
        preview.touch();
        assert!(preview.active(), "an interaction turns the burst on");
        // The burst has expired on its own, and the one quality repaint it
        // owes has landed.
        std::thread::sleep(Duration::from_millis(60));
        while gtk4::glib::MainContext::default().iteration(false) {}
        assert!(!preview.active(), "the burst must expire on its own");
        assert_eq!(draws.get(), 1, "the burst owes exactly one resting repaint");
    }
}

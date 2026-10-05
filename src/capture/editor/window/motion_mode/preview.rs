use gtk4::cairo::{Context, Format, ImageSurface};
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use super::{MotionHoverTrack, MotionRuntime};
use crate::capture::editor::window::motion_render::{
    motion_canvas, motion_preview_scene_rect, motion_source_size,
};
use crate::recording::editor::model::{MotionAppearance, MotionState};

pub(super) fn draw_motion_preview(
    context: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    prefers_dark: bool,
) {
    let has_card = runtime.borrow().card.is_some();
    if !has_card {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
        return;
    }
    // Paused scrub/hover stays off the UI thread (blit the last finished
    // frame while a worker renders): pointer motion paints stay cheap no
    // matter how heavy a frame is. Playback instead composites inline on
    // every miss — blitting the previous frame while the playhead advances
    // is what reads as a frozen preview with only the clock moving.
    let backdrop = {
        let mut guard = runtime.borrow_mut();
        cached_backdrop(&mut guard, width, height, prefers_dark)
    };
    let (hit, surface, live) = {
        let mut runtime = runtime.borrow_mut();
        let time = preview_time(&runtime);
        let live = runtime.live_preview || runtime.playing;
        let gen = runtime.preview_content_gen;
        if preview_cache_hit(
            runtime.preview_frame.as_ref(),
            width,
            height,
            time,
            live,
            gen,
        ) {
            (
                true,
                runtime
                    .preview_frame
                    .as_ref()
                    .map(|frame| frame.surface.clone()),
                live,
            )
        } else {
            if !live {
                schedule_preview_job_locked(
                    &mut runtime,
                    width,
                    height,
                    time,
                    live,
                    gen,
                    backdrop.clone(),
                );
            }
            (
                false,
                runtime
                    .preview_frame
                    .as_ref()
                    .map(|frame| frame.surface.clone()),
                live,
            )
        }
    };
    if hit {
        if let Some(surface) = surface {
            blit_preview_frame(context, width, height, &surface);
            return;
        }
    } else if let Some(surface) = surface.as_ref() {
        // Same-size frame still rendering: keep showing the previous one —
        // but only while paused. While playing the stale frame is the
        // frozen preview; fall through and composite the current time.
        // A resized frame must NOT be stretched to cover — entering Motion
        // shrinks the preview (timeline dock takes space), so the old tall
        // frame would keep the card at its old size, touching scene edges.
        // Fall through and composite inline at the current size instead.
        if !live && surface.width() == width.max(1) && surface.height() == height.max(1) {
            blit_preview_frame(context, width, height, surface);
            return;
        }
    }
    // First frame with nothing cached yet (or any playing frame): composite
    // inline so the widget never paints empty — or stale — then keep the
    // result as the cache.
    let stale = surface;
    let mut runtime = runtime.borrow_mut();
    let Some(backdrop) = backdrop else {
        return;
    };
    let (card, card_scale) = match runtime.card_preview.as_ref() {
        Some(preview) => (preview.clone(), runtime.card_scale),
        None => match runtime.card.as_ref() {
            Some(card) => (card.clone(), 1.0),
            None => return,
        },
    };
    let inputs = PreviewInputs {
        width,
        height,
        time: preview_time(&runtime),
        live_preview: runtime.live_preview || runtime.playing,
        card_scale,
        backdrop: &backdrop,
        card: &card,
        motion: &runtime.motion,
        watermark: runtime.watermark_surface.as_ref(),
    };
    if let Some(surface) = composite_preview_frame(&inputs) {
        blit_preview_frame(context, width, height, &surface);
        let time = preview_time(&runtime);
        let live = runtime.live_preview || runtime.playing;
        let gen = runtime.preview_content_gen;
        runtime.preview_frame = Some(super::session::PreviewFrame {
            width,
            height,
            time,
            live_preview: live,
            content_gen: gen,
            surface,
        });
    } else if let Some(stale) = stale {
        blit_preview_frame(context, width, height, &stale);
    }
}

/// Effective preview time: the red hover line wins while idle over a clip,
/// exactly as before — only where the frame comes from has changed.
pub(super) fn preview_time(runtime: &MotionRuntime) -> f64 {
    hover_preview_frame(
        &runtime.motion,
        runtime.hover_time,
        runtime.hover_track,
        runtime.playing,
    )
    .unwrap_or(runtime.motion.playhead)
}

fn preview_cache_hit(
    frame: Option<&super::session::PreviewFrame>,
    width: i32,
    height: i32,
    time: f64,
    live: bool,
    gen: u64,
) -> bool {
    frame.is_some_and(|frame| {
        frame.width == width
            && frame.height == height
            && frame.time.to_bits() == time.to_bits()
            && frame.live_preview == live
            && frame.content_gen == gen
    })
}

/// Everything a background composite needs. Plain data plus raw pixel
/// copies — the only shapes allowed across the thread boundary.
struct PreviewJob {
    width: i32,
    height: i32,
    time: f64,
    live_preview: bool,
    card_scale: f64,
    content_gen: u64,
    backdrop: super::session::PreviewPixels,
    card: super::session::PreviewPixels,
    motion: MotionState,
    watermark: Option<super::session::PreviewPixels>,
}

/// Borrowed inputs to the shared compositor. The worker rebuilds surfaces
/// from the transferred pixels, then calls through the same function as the
/// inline first frame — one code path, two threads.
struct PreviewInputs<'a> {
    width: i32,
    height: i32,
    time: f64,
    live_preview: bool,
    card_scale: f64,
    backdrop: &'a ImageSurface,
    card: &'a ImageSurface,
    motion: &'a MotionState,
    watermark: Option<&'a ImageSurface>,
}

fn surface_to_pixels(surface: &mut ImageSurface) -> Option<super::session::PreviewPixels> {
    surface.flush();
    let stride = surface.stride();
    let bytes = surface.data().ok()?.to_vec();
    Some(super::session::PreviewPixels {
        width: surface.width(),
        height: surface.height(),
        stride,
        bytes,
    })
}

fn surface_from_pixels(pixels: &super::session::PreviewPixels) -> Option<ImageSurface> {
    let mut surface =
        ImageSurface::create(Format::ARgb32, pixels.width.max(1), pixels.height.max(1)).ok()?;
    if surface.stride() != pixels.stride || surface.data().ok()?.len() != pixels.bytes.len() {
        return None;
    }
    surface.data().ok()?.copy_from_slice(&pixels.bytes);
    surface.flush();
    Some(surface)
}

/// Queue one background composite unless one is already in flight (later
/// draws just mark the request dirty). Must be called with the runtime
/// already borrowed mutably by the draw.
fn schedule_preview_job_locked(
    runtime: &mut MotionRuntime,
    width: i32,
    height: i32,
    time: f64,
    live: bool,
    gen: u64,
    backdrop: Option<ImageSurface>,
) {
    if runtime.preview_busy {
        runtime.preview_dirty = true;
        return;
    }
    let (mut card, card_scale) = match runtime.card_preview.as_ref() {
        Some(preview) => (preview.clone(), runtime.card_scale),
        None => match runtime.card.as_ref() {
            Some(card) => (card.clone(), 1.0),
            None => return,
        },
    };
    let Some(mut backdrop) = backdrop else {
        return;
    };
    // Pixels are the only thing allowed across the boundary: one quick copy
    // here (~1ms) instead of a 12ms composite blocking pointer motion.
    let (Some(backdrop), Some(card)) = (
        surface_to_pixels(&mut backdrop),
        surface_to_pixels(&mut card),
    ) else {
        return;
    };
    let watermark = match runtime.watermark_surface.as_ref() {
        Some(surface) => {
            let mut owned = surface.clone();
            let Some(pixels) = surface_to_pixels(&mut owned) else {
                return;
            };
            Some(pixels)
        }
        None => None,
    };
    if runtime.preview_tx.is_none() {
        let (tx, rx) = mpsc::channel();
        runtime.preview_tx = Some(tx);
        runtime.preview_rx = Some(rx);
    }
    let Some(tx) = runtime.preview_tx.clone() else {
        return;
    };
    let job = PreviewJob {
        width,
        height,
        time,
        live_preview: live,
        card_scale,
        content_gen: gen,
        backdrop,
        card,
        motion: runtime.motion.clone(),
        watermark,
    };
    runtime.preview_busy = true;
    runtime.preview_dirty = false;
    // ponytail: one thread per composite; a small pool if scrubbing ever
    // overlaps renders faster than single frames complete.
    std::thread::spawn(move || {
        let frame = render_preview_job(&job).map(|result| super::session::PreviewResult {
            width: result.width,
            height: result.height,
            time: result.time,
            live_preview: result.live_preview,
            content_gen: result.content_gen,
            stride: result.stride,
            bytes: result.bytes,
        });
        let _ = tx.send(frame);
    });
}

/// Schedule a composite from outside a draw (the timer poll uses this after
/// a frame lands while newer input arrived mid-flight).
pub(super) fn schedule_preview_job(runtime: &Rc<RefCell<MotionRuntime>>, prefers_dark: bool) {
    let (width, height) = {
        let runtime = runtime.borrow();
        match runtime.preview_frame.as_ref() {
            Some(frame) => (frame.width, frame.height),
            None => return,
        }
    };
    let backdrop = {
        let mut guard = runtime.borrow_mut();
        cached_backdrop(&mut guard, width, height, prefers_dark)
    };
    let mut runtime = runtime.borrow_mut();
    let time = preview_time(&runtime);
    let live = runtime.live_preview || runtime.playing;
    let gen = runtime.preview_content_gen;
    if preview_cache_hit(
        runtime.preview_frame.as_ref(),
        width,
        height,
        time,
        live,
        gen,
    ) {
        return;
    }
    schedule_preview_job_locked(&mut runtime, width, height, time, live, gen, backdrop);
}

/// Drain finished composites: store the latest, then re-render if input kept
/// moving mid-flight. Runs on the UI thread from the existing tick.
pub(super) fn poll_preview_results(
    runtime: &Rc<RefCell<MotionRuntime>>,
    preview: &gtk4::DrawingArea,
    prefers_dark: bool,
) {
    let mut finished = false;
    {
        let mut runtime = runtime.borrow_mut();
        let mut last = None;
        if let Some(rx) = runtime.preview_rx.as_ref() {
            while let Ok(frame) = rx.try_recv() {
                last = Some(frame);
            }
        }
        if let Some(last) = last {
            match last {
                Some(result) => {
                    // A worker scheduled earlier can land after the cache
                    // already moved on (edits bump the gen; inline playback
                    // advances time every frame). Storing it would regress
                    // the preview to an older frame — including the frozen
                    // look when a slow worker overwrites a newer inline
                    // composite. Drop outdated results; the next draw or
                    // dirty reschedule covers the current input.
                    let outdated = result.content_gen != runtime.preview_content_gen
                        || runtime.preview_frame.as_ref().is_some_and(|cached| {
                            cached.content_gen == result.content_gen
                                && cached.width == result.width
                                && cached.height == result.height
                                && cached.time.to_bits() > result.time.to_bits()
                        });
                    if !outdated {
                        if let Some(surface) = surface_from_pixels(&super::session::PreviewPixels {
                            width: result.width,
                            height: result.height,
                            stride: result.stride,
                            bytes: result.bytes,
                        }) {
                            runtime.preview_frame = Some(super::session::PreviewFrame {
                                width: result.width,
                                height: result.height,
                                time: result.time,
                                live_preview: result.live_preview,
                                content_gen: result.content_gen,
                                surface,
                            });
                            finished = true;
                        }
                    }
                }
                // Render failure unblocks the slot; the next draw retries.
                None => runtime.preview_dirty = false,
            }
            runtime.preview_busy = false;
        }
    }
    if finished {
        preview.queue_draw();
    }
    let dirty = runtime.borrow().preview_dirty;
    if dirty {
        runtime.borrow_mut().preview_dirty = false;
        schedule_preview_job(runtime, prefers_dark);
    }
}

/// The worker body: rebuild surfaces from the transferred pixels, composite,
/// and hand pixel bytes back. Pure: same inputs always produce the same
/// pixels, which is what the test pins.
fn render_preview_job(job: &PreviewJob) -> Option<super::session::PreviewResult> {
    let backdrop = surface_from_pixels(&job.backdrop)?;
    let card = surface_from_pixels(&job.card)?;
    let watermark = match job.watermark.as_ref() {
        Some(pixels) => Some(surface_from_pixels(pixels)?),
        None => None,
    };
    let inputs = PreviewInputs {
        width: job.width,
        height: job.height,
        time: job.time,
        live_preview: job.live_preview,
        card_scale: job.card_scale,
        backdrop: &backdrop,
        card: &card,
        motion: &job.motion,
        watermark: watermark.as_ref(),
    };
    let surface = composite_preview_frame(&inputs)?;
    surface.flush();
    let mut surface = surface;
    let stride = surface.stride();
    let bytes = surface.data().ok()?.to_vec();
    Some(super::session::PreviewResult {
        width: job.width,
        height: job.height,
        time: job.time,
        live_preview: job.live_preview,
        content_gen: job.content_gen,
        stride,
        bytes,
    })
}

/// The shared compositor: backdrop blit plus the animated foreground.
/// Pure: same inputs always produce the same pixels, which is what the test
/// pins. Both the worker and the inline first frame call it.
fn composite_preview_frame(inputs: &PreviewInputs<'_>) -> Option<ImageSurface> {
    let frame =
        ImageSurface::create(Format::ARgb32, inputs.width.max(1), inputs.height.max(1)).ok()?;
    let context = Context::new(&frame).ok()?;
    context.set_source_surface(inputs.backdrop, 0.0, 0.0).ok()?;
    context.paint().ok()?;
    super::super::motion_render::draw_motion_foreground(
        &context,
        inputs.width,
        inputs.height,
        inputs.card,
        inputs.motion,
        inputs.watermark,
        inputs.time,
        true,
        inputs.live_preview,
        inputs.card_scale,
    );
    frame.flush();
    Some(frame)
}

fn blit_preview_frame(context: &Context, width: i32, height: i32, surface: &ImageSurface) {
    let _ = context.save();
    let (sw, sh) = (
        f64::from(surface.width().max(1)),
        f64::from(surface.height().max(1)),
    );
    // A resize mid-flight leaves a stale-size frame. Never stretch it: scale
    // uniformly (cover) and center so the odd frame crops slightly instead of
    // distorting the card while the fresh composite renders.
    if surface.width() != width.max(1) || surface.height() != height.max(1) {
        let scale = (f64::from(width.max(1)) / sw).max(f64::from(height.max(1)) / sh);
        let dx = (f64::from(width.max(1)) - sw * scale) * 0.5;
        let dy = (f64::from(height.max(1)) - sh * scale) * 0.5;
        context.translate(dx, dy);
        context.scale(scale, scale);
    }
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.paint().ok();
    let _ = context.restore();
}

fn cached_backdrop(
    runtime: &mut MotionRuntime,
    width: i32,
    height: i32,
    prefers_dark: bool,
) -> Option<ImageSurface> {
    let width = width.max(1);
    let height = height.max(1);
    let source = match (runtime.card_preview.as_ref(), runtime.card.as_ref()) {
        (Some(preview), _) => Some(motion_source_size(preview, runtime.card_scale)),
        (None, Some(card)) => Some(motion_source_size(card, 1.0)),
        _ => None,
    };
    let canvas = source.map(|(w, h)| {
        let composition = motion_canvas(w, h, &runtime.motion.appearance, &runtime.motion.frame);
        (composition.canvas_width, composition.canvas_height)
    });
    let (canvas_w, canvas_h) = canvas.unwrap_or((f64::from(width), f64::from(height)));
    let scene_rect =
        motion_preview_scene_rect(f64::from(width), f64::from(height), canvas_w, canvas_h);
    let stale = runtime.backdrop_cache.as_ref().is_none_or(|cache| {
        cache.width != width
            || cache.height != height
            || cache.prefers_dark != prefers_dark
            || cache.scene_rect != scene_rect
            || !same_backdrop_appearance(&cache.appearance, &runtime.motion.appearance)
    });
    if stale {
        let surface = ImageSurface::create(Format::ARgb32, width, height).ok()?;
        let context = Context::new(&surface).ok()?;
        super::super::motion_render::draw_motion_backdrop(
            &context,
            width,
            height,
            &runtime.motion,
            runtime.background_surface.as_ref(),
            true,
            prefers_dark,
            canvas,
        );
        surface.flush();
        runtime.backdrop_cache = Some(super::session::MotionBackdropCache {
            width,
            height,
            prefers_dark,
            scene_rect,
            appearance: runtime.motion.appearance.clone(),
            surface,
        });
    }
    runtime
        .backdrop_cache
        .as_ref()
        .map(|cache| cache.surface.clone())
}

/// Card styling (padding, border, and shadow) is composited above the scene.
/// Do not evict a costly blurred background just because one of those controls
/// changes while it is being dragged.
fn same_backdrop_appearance(a: &MotionAppearance, b: &MotionAppearance) -> bool {
    a.background_fill_type == b.background_fill_type
        && a.background_color == b.background_color
        && a.gradient == b.gradient
        && a.wallpaper_image_name == b.wallpaper_image_name
        && a.custom_background_image == b.custom_background_image
        && a.background_blur == b.background_blur
        && a.background_noise == b.background_noise
}

/// Hover frame for the preview, or `None` to stay on the playhead.
/// Motions play on their allocated clip span only: hovering empty lane space
/// next to a clip (or the other lane's gap) never fakes a motion preview.
pub(super) fn hover_preview_frame(
    motion: &MotionState,
    hover_time: Option<f64>,
    hover_track: Option<MotionHoverTrack>,
    playing: bool,
) -> Option<f64> {
    if playing {
        return None;
    }
    let hover = hover_time?;
    let over_clip = match hover_track? {
        MotionHoverTrack::Motion => motion.segment_index_at(hover).is_some(),
        MotionHoverTrack::Text => motion.text_index_at(hover).is_some(),
    };
    over_clip.then_some(hover)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_preview_only_plays_directly_over_a_clip() {
        let mut motion = MotionState::default();
        motion.add_segment_at(0.0).expect("motion clip");
        // Directly over the motion clip: hover frame wins, playhead untouched.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Motion), false),
            Some(0.5)
        );
        // Empty lane space next to the clip: no fake motion preview.
        assert_eq!(
            hover_preview_frame(&motion, Some(3.0), Some(MotionHoverTrack::Motion), false),
            None
        );
        // Playback always owns the preview.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Motion), true),
            None
        );
        // A motion time under a text-lane hover stays on the playhead.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Text), false),
            None
        );
        motion.add_text_at(4.0).expect("text clip");
        assert_eq!(
            hover_preview_frame(&motion, Some(4.5), Some(MotionHoverTrack::Text), false),
            Some(4.5)
        );
    }

    #[test]
    fn card_styling_keeps_the_cached_backdrop_valid() {
        let original = MotionAppearance::default();
        let mut styled = original.clone();
        styled.background_padding = 24.0;
        styled.border_radius = 18.0;
        styled.border_thickness = 2.0;
        styled.shadow_opacity = 0.75;
        styled.shadow_blur = 32.0;
        styled.shadow_position = (14.0, -8.0);

        assert!(same_backdrop_appearance(&original, &styled));

        styled.background_noise = 0.2;
        assert!(!same_backdrop_appearance(&original, &styled));
    }

    fn preview_canvas(card: &ImageSurface, motion: &MotionState) -> (f64, f64) {
        let (source_w, source_h) = motion_source_size(card, 1.0);
        let canvas = motion_canvas(source_w, source_h, &motion.appearance, &motion.frame);
        (canvas.canvas_width, canvas.canvas_height)
    }

    #[test]
    fn background_thread_composite_matches_the_direct_frame() {
        use crate::capture::editor::window::motion_render;
        use crate::recording::editor::model::MotionBackgroundFillType;

        let card = ImageSurface::create(Format::ARgb32, 64, 48).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(0.2, 0.5, 0.8);
            context.paint().unwrap();
        }
        card.flush();
        let mut motion = MotionState::default();
        motion.add_segment_at(0.0).expect("motion clip");
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;

        let backdrop = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        {
            let context = Context::new(&backdrop).unwrap();
            motion_render::draw_motion_backdrop(
                &context,
                160,
                120,
                &motion,
                None,
                true,
                true,
                Some(preview_canvas(&card, &motion)),
            );
        }
        backdrop.flush();

        // Pixel round-trip across the thread boundary must be lossless.
        let mut card = card;
        let round_tripped = surface_from_pixels(&surface_to_pixels(&mut card).unwrap()).unwrap();
        let mut round_tripped = round_tripped;
        round_tripped.flush();
        assert_eq!(
            round_tripped.data().unwrap().to_vec(),
            card.data().unwrap().to_vec()
        );

        let inputs = PreviewInputs {
            width: 160,
            height: 120,
            time: 0.4,
            live_preview: false,
            card_scale: 1.0,
            backdrop: &backdrop,
            card: &card,
            motion: &motion,
            watermark: None,
        };
        let threaded = composite_preview_frame(&inputs).expect("worker composite");

        let direct = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        {
            let context = Context::new(&direct).unwrap();
            motion_render::draw_motion_frame(
                &context, 160, 120, &card, &motion, None, None, 0.4, true, true, false, 1.0,
            );
        }
        let mut threaded = threaded;
        let mut direct = direct;
        threaded.flush();
        direct.flush();
        assert_eq!(
            threaded.data().unwrap().to_vec(),
            direct.data().unwrap().to_vec(),
            "the threaded frame must pixel-match the old synchronous paint"
        );
    }

    #[test]
    fn preview_cache_key_tracks_size_time_mode_and_content() {
        let surface = ImageSurface::create(Format::ARgb32, 4, 4).unwrap();
        let frame = super::super::session::PreviewFrame {
            width: 160,
            height: 120,
            time: 0.5,
            live_preview: false,
            content_gen: 3,
            surface,
        };
        assert!(preview_cache_hit(Some(&frame), 160, 120, 0.5, false, 3));
        assert!(!preview_cache_hit(Some(&frame), 160, 120, 0.5001, false, 3));
        assert!(!preview_cache_hit(Some(&frame), 160, 120, 0.5, true, 3));
        assert!(!preview_cache_hit(Some(&frame), 160, 120, 0.5, false, 4));
        assert!(!preview_cache_hit(Some(&frame), 161, 120, 0.5, false, 3));
        assert!(!preview_cache_hit(None, 160, 120, 0.5, false, 3));
    }
}

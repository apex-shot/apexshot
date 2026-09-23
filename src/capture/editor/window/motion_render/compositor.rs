/// Keep the interactive card tessellation identical to the export renderer.
///
/// Ease edits immediately replay a Motion segment. Previously that replay
/// selected a coarse mesh (and changed it again once the scale passed 1.75x),
/// so the perspective approximation visibly rippled even though the only
/// property being edited was timing.
const CARD_MESH_DIVISIONS: usize = 8;

/// Subframe mesh density for temporal accumulation. Subframes are averaged,
/// so their individual mesh error is divided by the sample count; the coarser
/// grid keeps the accumulation affordable without a visible difference.
const MOTION_BLUR_MESH_DIVISIONS: usize = 4;

/// Long-edge cap for the card texture the interactive preview samples from.
/// The preview never draws the card larger than its scene panel, so sampling a
/// multi-megapixel source down to that panel dominated scrub frame time.
/// Export keeps the full-resolution card.
pub const PREVIEW_CARD_MAX_EDGE: i32 = 1600;

/// Build the downscaled card texture used by the editor preview. Returns the
/// surface and the pixel scale applied to it; callers pass the scale through
/// to the renderer so card-space values (corner radius, title size) stay
/// visually identical to the full-resolution export.
pub fn scaled_card_preview(card: &ImageSurface) -> Option<(ImageSurface, f64)> {
    let (width, height) = (card.width(), card.height());
    if width < 1 || height < 1 {
        return None;
    }
    let long_edge = f64::from(width.max(height));
    if long_edge <= f64::from(PREVIEW_CARD_MAX_EDGE) {
        return None;
    }
    let factor = f64::from(PREVIEW_CARD_MAX_EDGE) / long_edge;
    if f64::from(width.min(height)) * factor < 16.0 {
        return None;
    }
    let preview_w = ((f64::from(width) * factor).round() as i32).max(1);
    let preview_h = ((f64::from(height) * factor).round() as i32).max(1);
    let surface = ImageSurface::create(Format::ARgb32, preview_w, preview_h).ok()?;
    {
        let context = Context::new(&surface).ok()?;
        context.scale(
            f64::from(preview_w) / f64::from(width),
            f64::from(preview_h) / f64::from(height),
        );
        context.set_source_surface(card, 0.0, 0.0).ok()?;
        context.source().set_filter(Filter::Good);
        context.paint().ok()?;
    }
    surface.flush();
    Some((surface, f64::from(preview_w) / f64::from(width)))
}

pub fn draw_motion_frame(
    context: &Context,
    width: i32,
    height: i32,
    surface: &ImageSurface,
    motion: &MotionState,
    background_surface: Option<&ImageSurface>,
    watermark_surface: Option<&ImageSurface>,
    time: f64,
    checkerboard: bool,
    prefers_dark: bool,
    live_preview: bool,
    card_scale: f64,
) {
    draw_motion_backdrop(
        context,
        width,
        height,
        motion,
        background_surface,
        checkerboard,
        prefers_dark,
    );
    draw_motion_foreground(
        context,
        width,
        height,
        surface,
        motion,
        watermark_surface,
        time,
        checkerboard,
        live_preview,
        card_scale,
    );
}

/// Paint the scene layer shared by the static Motion preview and playback
/// frames. The editor caches this output while its Appearance is unchanged.
pub(super) fn draw_motion_backdrop(
    context: &Context,
    width: i32,
    height: i32,
    motion: &MotionState,
    background_surface: Option<&ImageSurface>,
    checkerboard: bool,
    prefers_dark: bool,
) {
    paint_backdrop(
        context,
        width,
        height,
        motion,
        background_surface,
        checkerboard,
        prefers_dark,
    );
}

/// Paint the animated layers over an already-prepared Motion backdrop.
/// Keeping this separate lets the interactive editor avoid regenerating an
/// unchanged wallpaper, blur, checkerboard, or noise field on every frame.
pub(super) fn draw_motion_foreground(
    context: &Context,
    width: i32,
    height: i32,
    surface: &ImageSurface,
    motion: &MotionState,
    watermark_surface: Option<&ImageSurface>,
    time: f64,
    checkerboard: bool,
    live_preview: bool,
    card_scale: f64,
) {
    // Padding, zoom, and titles all lay out against the background's
    // rectangle so the card can never sit outside the scene it belongs to.
    let stage = if checkerboard {
        MotionStage::preview(
            f64::from(width),
            f64::from(height),
            motion.frame.effective_aspect(),
        )
    } else {
        MotionStage::frame(f64::from(width), f64::from(height))
    };
    // The background and animated foreground are one composition. Keep every
    // transformed layer inside the same scene bounds instead of allowing a
    // scaled or rotated card to spill over the preview checkerboard.
    let _ = context.save();
    context.rectangle(
        stage.center_x - stage.bounds_w * 0.5,
        stage.center_y - stage.bounds_h * 0.5,
        stage.bounds_w,
        stage.bounds_h,
    );
    context.clip();
    // The underlay shadow layer sits between the background scene and
    // the animated card, so the card's own drop shadow still reads on top.
    paint_motion_scene_shadow(context, stage, &motion.scene_shadow, true);
    let current_transform = motion.sample(time);
    let current_anchor = motion.zoom_anchor_at(time);
    // The editor preview draws a downscaled card texture into a panel-sized
    // canvas. Cairo's Good filter convolves the source on every downscale,
    // and that dominated scrub frame time (tens of milliseconds a frame);
    // Bilinear keeps playback and scrubbing cheap. A paused still is not
    // scrubbing, so it keeps the high-quality filter to match the Static
    // canvas — annotations must not go soft just from entering Motion.
    // Pressing play may pop sharpness slightly; that is cheaper than a
    // permanently soft still. Export (checkerboard = false) always uses Good.
    let card_filter = if checkerboard && live_preview {
        Filter::Bilinear
    } else {
        Filter::Good
    };
    // The card is drawn as a triangle mesh that approximates the perspective
    // warp. Its resolution must not depend on playhead scale: Ease edits
    // replay the segment, and a changing grid makes an otherwise smooth
    // timing curve look like a wave. Use the same stable density as export.
    let mesh_div = CARD_MESH_DIVISIONS;
    // Rounding is independent of the pose; do it once per frame instead of
    // once per accumulated subframe.
    let surface_long = f64::from(surface.width().max(surface.height()).max(1));
    let rounded = rounded_motion_surface(
        surface,
        motion.appearance.border_radius * surface_long / 400.0,
    );
    let card_surface = rounded.as_ref().unwrap_or(surface);
    // True motion blur is the average of every instant of the exposure.
    // Cairo has no CIMotionBlur/CIZoomBlur, so ApexShot reaches the same
    // result with temporal accumulation: the card is rendered at a dense set
    // of times across the exposure window and the subframes are averaged.
    // The recovered schema and bounds remain shared with the source app.
    let frame_rate = f64::from(MOTION_EXPORT_FPS);
    let blur_settings = motion.motion_blur_settings.clamped();
    let exposure = blur_settings.exposure_seconds(frame_rate);
    let sample_offsets = if exposure > f64::EPSILON {
        let exposure_start = (time - exposure).max(0.0);
        let travel = card_corner_travel(
            card_surface,
            stage,
            motion.sample(exposure_start),
            motion.sample(time),
            motion.zoom_anchor_at(exposure_start),
            motion.zoom_anchor_at(time),
            motion.appearance.effective_padding(),
        );
        blur_settings.temporal_offsets(
            frame_rate,
            travel,
            if live_preview {
                MotionBlurBudgetMode::LivePreviewPlayback
            } else {
                MotionBlurBudgetMode::FullQuality
            },
        )
    } else {
        Vec::new()
    };
    let blurred = sample_offsets.len() > 1
        && paint_motion_blurred_card(
            context,
            width,
            height,
            card_surface,
            stage,
            motion,
            &sample_offsets,
            time,
            card_filter,
        );
    if !blurred {
        draw_transformed_card(
            context,
            card_surface,
            stage,
            current_transform,
            current_anchor,
            &motion.appearance,
            1.0,
            mesh_div,
            card_filter,
        );
    }
    paint_motion_text(context, surface, stage, motion, time, card_scale);
    // The overlay shadow pass shades the card and titles; the watermark
    // stays the topmost layer.
    paint_motion_scene_shadow(context, stage, &motion.scene_shadow, false);
    // ApexShot keeps the user-selected mark above card content and Motion
    // titles. Its card-space projection makes the layer track zoom and
    // perspective identically in preview and export.
    if let Some(watermark_surface) = watermark_surface {
        paint_motion_watermark(context, surface, stage, motion, watermark_surface, time);
    }
    context.restore().ok();
}

/// Average the moving card across its exposure window. Each subframe is
/// rendered opaque into a scratch buffer so mesh-clip seams never blend at
/// partial alpha, then folded into a running average with weight 1/(k+1) —
/// the classic accumulation-buffer temporal filter. Returns false when the
/// offscreen buffers cannot be allocated so the caller can fall back to the
/// sharp pose.
fn paint_motion_blurred_card(
    context: &Context,
    width: i32,
    height: i32,
    surface: &ImageSurface,
    stage: MotionStage,
    motion: &MotionState,
    sample_offsets: &[f64],
    time: f64,
    filter: Filter,
) -> bool {
    // ponytail: two fresh buffers per blurred frame; cache them in the Motion
    // runtime if live playback ever drops frames to allocation churn.
    let (Ok(accum), Ok(scratch)) = (
        ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)),
        ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)),
    ) else {
        return false;
    };
    let (Ok(accum_context), Ok(scratch_context)) = (Context::new(&accum), Context::new(&scratch))
    else {
        return false;
    };
    // Draw oldest first and let each subframe blend the running mean by
    // 1/(k+1). A pixel the newest subframe alone covers is then attenuated by
    // every earlier draw, keeping frontier edges at their true exposure
    // fraction (1/N) instead of the first draw's full opacity.
    for (index, offset) in sample_offsets.iter().rev().enumerate() {
        let sample_t = (time + offset).max(0.0);
        scratch_context.set_operator(Operator::Clear);
        scratch_context.paint().ok();
        scratch_context.set_operator(Operator::Over);
        draw_transformed_card(
            &scratch_context,
            surface,
            stage,
            motion.sample(sample_t),
            motion.zoom_anchor_at(sample_t),
            &motion.appearance,
            1.0,
            MOTION_BLUR_MESH_DIVISIONS,
            filter,
        );
        scratch.flush();
        accum_context.set_source_surface(&scratch, 0.0, 0.0).ok();
        // Running mean: after this paint the accumulation holds the average
        // of every subframe drawn so far.
        accum_context
            .paint_with_alpha(1.0 / (index as f64 + 1.0))
            .ok();
    }
    accum.flush();
    context.set_source_surface(&accum, 0.0, 0.0).ok();
    context.paint().ok();
    true
}

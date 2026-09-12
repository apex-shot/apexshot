/// Keep the interactive card tessellation identical to the export renderer.
///
/// Ease edits immediately replay a Motion segment. Previously that replay
/// selected a coarse mesh (and changed it again once the scale passed 1.75x),
/// so the perspective approximation visibly rippled even though the only
/// property being edited was timing.
const CARD_MESH_DIVISIONS: usize = 8;

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
        MotionStage::preview(f64::from(width), f64::from(height), motion.frame.preset.aspect())
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
    // Shotbase's underlay shadow layer sits between the background scene and
    // the animated card, so the card's own drop shadow still reads on top.
    paint_motion_scene_shadow(context, stage, motion, true);
    let current_transform = motion.sample(time);
    let current_anchor = motion.zoom_anchor_at(time);
    // The card is drawn as a triangle mesh that approximates the perspective
    // warp. Its resolution must not depend on playhead scale: Ease edits
    // replay the segment, and a changing grid makes an otherwise smooth
    // timing curve look like a wave. Use the same stable density as export.
    let mesh_div = CARD_MESH_DIVISIONS;
    // Cairo has no equivalent of Shotbase's full-quality CIMotionBlur and
    // CIZoomBlur filters, so ApexShot uses this bounded temporal fallback.
    // The recovered schema and bounds remain shared with the source app.
    for sample in motion.motion_blur_settings.transform_trail(
        f64::from(MOTION_EXPORT_FPS),
        if live_preview {
            MotionBlurBudgetMode::LivePreviewPlayback
        } else {
            MotionBlurBudgetMode::FullQuality
        },
    ) {
        let sample_t = (time + sample.offset_seconds).max(0.0);
        if sample.opacity > 0.001 {
            let transform = motion.sample(sample_t);
            let anchor = motion.zoom_anchor_at(sample_t);
            if !motion_pose_differs(transform, current_transform, anchor, current_anchor) {
                continue;
            }
            draw_transformed_card(
                context,
                surface,
                stage,
                transform,
                anchor,
                &motion.appearance,
                sample.opacity,
                mesh_div,
                card_scale,
            );
        }
    }
    draw_transformed_card(
        context,
        surface,
        stage,
        current_transform,
        current_anchor,
        &motion.appearance,
        1.0,
        mesh_div,
        card_scale,
    );
    paint_motion_text(context, surface, stage, motion, time, card_scale);
    // The overlay shadow pass shades the card and titles; the watermark
    // stays the topmost layer.
    paint_motion_scene_shadow(context, stage, motion, false);
    // ApexShot keeps the user-selected mark above card content and Motion
    // titles. Its card-space projection makes the layer track zoom and
    // perspective identically in preview and export.
    if let Some(watermark_surface) = watermark_surface {
        paint_motion_watermark(context, surface, stage, motion, watermark_surface, time);
    }
    context.restore().ok();
}

/// A held camera pose is already identical to the sharp overlay. Skipping it
/// preserves Shotbase's budgeted-preview behavior and avoids mesh work without
/// changing the exported pixels.
fn motion_pose_differs(
    a: MotionTransform,
    b: MotionTransform,
    anchor_a: (f64, f64),
    anchor_b: (f64, f64),
) -> bool {
    const EPSILON: f64 = 0.0001;
    (a.scale - b.scale).abs() > EPSILON
        || (a.rotation_x - b.rotation_x).abs() > EPSILON
        || (a.rotation_y - b.rotation_y).abs() > EPSILON
        || (a.rotation_z - b.rotation_z).abs() > EPSILON
        || (a.perspective - b.perspective).abs() > EPSILON
        || (a.pos_x - b.pos_x).abs() > EPSILON
        || (a.pos_y - b.pos_y).abs() > EPSILON
        || (anchor_a.0 - anchor_b.0).abs() > EPSILON
        || (anchor_a.1 - anchor_b.1).abs() > EPSILON
}

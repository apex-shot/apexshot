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
    // Padding, zoom, and titles all lay out against the background's
    // rectangle so the card can never sit outside the scene it belongs to.
    let stage = if checkerboard {
        MotionStage::preview(f64::from(width), f64::from(height))
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
    let current_transform = motion.sample(time);
    let current_anchor = motion.zoom_anchor_at(time);
    // The card is drawn as a triangle mesh that approximates the perspective
    // warp. Zooming in magnifies each affine cell until the tessellation
    // reads as a wavy warp, so live previews subdivide more when zoomed.
    let mesh_div = if live_preview {
        if current_transform.scale > 1.75 {
            7
        } else {
            3
        }
    } else {
        8
    };
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
    );
    paint_motion_text(context, surface, stage, motion, time);
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

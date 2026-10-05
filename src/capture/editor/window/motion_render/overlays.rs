/// Paint a selected watermark in source-card coordinates.  The source image
/// is never sampled from disk here: callers retain it for live preview and
/// resolve it once for export, keeping the two compositor paths equivalent.
fn paint_motion_watermark(
    context: &Context,
    card: &ImageSurface,
    stage: MotionStage,
    motion: &MotionState,
    watermark: &ImageSurface,
    time: f64,
) {
    if motion.watermark.image_file_name.is_none() {
        return;
    }
    let transform = motion.sample(time);
    let layout = CardLayout::with_padding(
        card,
        stage,
        transform,
        motion.zoom_anchor_at(time),
        motion.appearance.effective_padding(),
    );
    let source_w = f64::from(watermark.width().max(1));
    let source_h = f64::from(watermark.height().max(1));
    let width = (layout.img_w * motion.watermark.size.clamp(0.02, 0.8)).max(1.0);
    let height = (width * source_h / source_w)
        .min(layout.img_h * 0.8)
        .max(1.0);
    let inset = (layout.img_w * motion.watermark.inset.clamp(0.0, 0.45))
        .min((layout.img_w - width).max(0.0) * 0.5);
    let inset_y = inset.min((layout.img_h - height).max(0.0) * 0.5);
    let x = (motion.watermark.position.0.clamp(0.0, 1.0) * layout.img_w - width * 0.5)
        .clamp(inset, (layout.img_w - inset - width).max(inset));
    let y = (motion.watermark.position.1.clamp(0.0, 1.0) * layout.img_h - height * 0.5)
        .clamp(inset_y, (layout.img_h - inset_y - height).max(inset_y));
    let Some(matrix) = layout.local_matrix(x, y) else {
        return;
    };
    let _ = context.save();
    context.transform(matrix);
    context.translate(x, y);
    context.scale(width / source_w, height / source_h);
    context.set_source_surface(watermark, 0.0, 0.0).ok();
    context.source().set_filter(Filter::Bilinear);
    context.paint().ok();
    context.restore().ok();
}

/// Paint the selected Scene Shadows preset across the scene rectangle.
/// Distinct overlay and underlay shadow render layers; the
/// `underlay` pass draws beneath the card, the overlay pass above card and
/// titles but below the watermark. Presets are procedural shading rather
/// than image assets, so Motion preview/export and the static
/// preview/export all share this one painter.
pub(crate) fn paint_motion_scene_shadow(
    context: &Context,
    stage: MotionStage,
    shadow: &MotionSceneShadow,
    underlay: bool,
) {
    if shadow.preset == MotionSceneShadowPreset::None {
        return;
    }
    if (shadow.placement == crate::recording::editor::model::MotionSceneShadowPlacement::Underlay)
        != underlay
    {
        return;
    }
    let opacity = shadow.opacity.clamp(0.0, 1.0);
    if opacity <= 0.001 {
        return;
    }
    let left = stage.center_x - stage.bounds_w * 0.5;
    let top = stage.center_y - stage.bounds_h * 0.5;
    let right = left + stage.bounds_w;
    let bottom = top + stage.bounds_h;
    let _ = context.save();
    context.rectangle(left, top, stage.bounds_w, stage.bounds_h);
    context.clip();
    match shadow.preset {
        MotionSceneShadowPreset::None => {}
        MotionSceneShadowPreset::Diagonal => {
            let gradient = LinearGradient::new(left, top, right, bottom);
            gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, opacity);
            gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            context.set_source(&gradient).ok();
        }
        MotionSceneShadowPreset::Top => {
            let gradient = LinearGradient::new(0.0, top, 0.0, bottom);
            gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, opacity);
            gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            context.set_source(&gradient).ok();
        }
        MotionSceneShadowPreset::Bottom => {
            let gradient = LinearGradient::new(0.0, top, 0.0, bottom);
            gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, opacity);
            context.set_source(&gradient).ok();
        }
        MotionSceneShadowPreset::Side => {
            let gradient = LinearGradient::new(left, 0.0, right, 0.0);
            gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, opacity);
            gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            context.set_source(&gradient).ok();
        }
        MotionSceneShadowPreset::Vignette => {
            let radius = stage.bounds_w.max(stage.bounds_h) * 0.5;
            let gradient = gtk4::cairo::RadialGradient::new(
                stage.center_x,
                stage.center_y,
                0.0,
                stage.center_x,
                stage.center_y,
                radius,
            );
            gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, opacity);
            context.set_source(&gradient).ok();
        }
        MotionSceneShadowPreset::Window => {
            // Two soft diagonal light gaps across the scene, expressed as one
            // striped gradient along the diagonal.
            let gradient = LinearGradient::new(left, top, right, bottom);
            for (offset, strength) in [
                (0.08, 0.0),
                (0.16, 0.0),
                (0.24, opacity),
                (0.32, 0.0),
                (0.5, 0.0),
                (0.58, 0.0),
                (0.66, opacity * 0.8),
                (0.74, 0.0),
                (1.0, 0.0),
            ] {
                gradient.add_color_stop_rgba(offset, 0.0, 0.0, 0.0, strength);
            }
            context.set_source(&gradient).ok();
        }
    }
    context.paint().ok();
    context.restore().ok();
}

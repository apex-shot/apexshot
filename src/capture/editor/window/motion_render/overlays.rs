/// Draw titles in the captured image's coordinate space, then project that
/// space through the current camera. This is deliberately shared by preview
/// and export: a title placed on the card follows yaw, pitch, roll, scale, and
/// position instead of floating over the editor viewport.
fn paint_motion_text(
    context: &Context,
    surface: &ImageSurface,
    stage: MotionStage,
    motion: &MotionState,
    time: f64,
    card_scale: f64,
) {
    let transform = motion.sample(time);
    let layout = CardLayout::with_padding(
        surface,
        stage,
        transform,
        motion.zoom_anchor_at(time),
        motion.appearance.background_padding,
    );
    for segment in &motion.text_segments {
        let Some(style) = segment.sample(time) else {
            continue;
        };
        if style.alpha < 0.02 {
            continue;
        }
        let raw = segment.text.trim();
        let line = if raw.is_empty() { "Title" } else { raw };
        let line = line.lines().next().unwrap_or("Title");
        let line = visible_motion_text(line, segment.scope, style.reveal);
        if line.is_empty() {
            continue;
        }
        // Size clamps are authored against the source card, so evaluate them
        // in source pixels and scale the result into the (possibly
        // downscaled) preview texture.
        let size = (layout.img_h / card_scale.max(1e-6) * 0.060 * segment.size.clamp(0.5, 2.2))
            .clamp(14.0, 160.0)
            * card_scale;
        let anchor_x = segment.pos_x.clamp(0.05, 0.95) * layout.img_w;
        let anchor_y = segment.pos_y.clamp(0.05, 0.95) * layout.img_h;
        let Some(matrix) = layout.local_matrix(anchor_x, anchor_y) else {
            continue;
        };
        let _ = context.save();
        context.transform(matrix);
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(size);
        let Ok(ext) = context.text_extents(&line) else {
            let _ = context.restore();
            continue;
        };
        let animated_offset_x = style.offset_x * layout.img_h / 1080.0;
        let x = anchor_x + animated_offset_x - ext.width() / 2.0 - ext.x_bearing();
        // `MotionTextStyle` uses a 1080pt artboard baseline. Scale it into
        // the source image so the entrance distance remains consistent after
        // the card is fitted into either preview or export.
        let animated_offset = style.offset_y * layout.img_h / 1080.0;
        let y = anchor_y + animated_offset - ext.height() / 2.0 - ext.y_bearing();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.42 * style.alpha);
        context.move_to(x, y + 4.0 * card_scale);
        let _ = context.show_text(&line);
        context.set_source_rgba(1.0, 1.0, 1.0, style.alpha);
        context.move_to(x, y);
        let _ = context.show_text(&line);
        let _ = context.restore();
    }
}

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
        motion.appearance.background_padding,
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
/// Shotbase names distinct overlay and underlay shadow render layers; the
/// `underlay` pass draws beneath the card, the overlay pass above card and
/// titles but below the watermark. Presets are procedural shading rather
/// than image assets, so preview and export share this one painter.
fn paint_motion_scene_shadow(
    context: &Context,
    stage: MotionStage,
    motion: &MotionState,
    underlay: bool,
) {
    let shadow = &motion.scene_shadow;
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

fn visible_motion_text(
    text: &str,
    scope: crate::recording::editor::model::MotionTextScope,
    reveal: f64,
) -> String {
    if reveal >= 0.999 {
        return text.to_string();
    }
    match scope {
        crate::recording::editor::model::MotionTextScope::Character => {
            let count = (text.chars().count() as f64 * reveal).ceil() as usize;
            text.chars().take(count).collect()
        }
        crate::recording::editor::model::MotionTextScope::Word => {
            let words: Vec<_> = text.split_whitespace().collect();
            let count = (words.len() as f64 * reveal).ceil() as usize;
            words.into_iter().take(count).collect::<Vec<_>>().join(" ")
        }
        crate::recording::editor::model::MotionTextScope::Line => {
            if reveal > 0.0 {
                text.to_string()
            } else {
                String::new()
            }
        }
    }
}

/// Whether a preview pointer is on the rendered text itself. Text placement
/// must begin on this hit region; a selected title alone is not permission to
/// rewrite its coordinates by dragging empty canvas space.
pub fn motion_text_contains_view_point(
    surface: &ImageSurface,
    stage: MotionStage,
    padding: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    segment: &MotionTextSegment,
    time: f64,
    view_x: f64,
    view_y: f64,
) -> bool {
    let Some(style) = segment.sample(time) else {
        return false;
    };
    let raw = segment.text.trim();
    let line = if raw.is_empty() { "Title" } else { raw };
    let line = visible_motion_text(
        line.lines().next().unwrap_or("Title"),
        segment.scope,
        style.reveal,
    );
    if line.is_empty() || style.alpha < 0.02 {
        return false;
    }

    let layout = CardLayout::with_padding(surface, stage, transform, zoom_anchor, padding);
    let size = (layout.img_h * 0.060 * segment.size.clamp(0.5, 2.2)).clamp(14.0, 160.0);
    let Ok(measure) = Context::new(surface) else {
        return false;
    };
    measure.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    measure.set_font_size(size);
    let Ok(ext) = measure.text_extents(&line) else {
        return false;
    };
    let anchor_x =
        segment.pos_x.clamp(0.05, 0.95) * layout.img_w + style.offset_x * layout.img_h / 1080.0;
    let anchor_y =
        segment.pos_y.clamp(0.05, 0.95) * layout.img_h + style.offset_y * layout.img_h / 1080.0;
    let pointer = view_point_to_motion_text_position(
        surface,
        stage,
        padding,
        transform,
        zoom_anchor,
        view_x,
        view_y,
    );
    let pointer_x = pointer.0 * layout.img_w;
    let pointer_y = pointer.1 * layout.img_h;
    // Keep a small source-space hit slop so a title is still easy to grab at
    // normal preview zoom, while never turning empty card space into a drag.
    let slop = (8.0 / layout.fit.max(0.05)).min(24.0);
    let left = anchor_x - ext.width() / 2.0;
    let top = anchor_y - ext.height() / 2.0;
    pointer_x >= left - slop
        && pointer_x <= left + ext.width() + slop
        && pointer_y >= top - slop
        && pointer_y <= top + ext.height() + slop
}

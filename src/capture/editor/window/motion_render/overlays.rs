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
        let size = (layout.img_h * 0.060 * segment.size.clamp(0.5, 2.2)).clamp(14.0, 160.0);
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
        context.move_to(x, y + 4.0);
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

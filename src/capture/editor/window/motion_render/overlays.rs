/// Where a watermark lands inside a card rectangle, in that rectangle's own
/// units. The compositor places it on the card and the Watermark pad on its
/// thumbnail, so both show the same placement at their own scale.
pub(in crate::capture::editor::window) fn motion_watermark_rect(
    (left, top, card_w, card_h): (f64, f64, f64, f64),
    position: (f64, f64),
    size: f64,
    inset: f64,
    (source_w, source_h): (f64, f64),
) -> (f64, f64, f64, f64) {
    let width = (card_w * size.clamp(0.02, 0.8)).max(1.0);
    let height = (width * source_h / source_w.max(1.0))
        .min(card_h * 0.8)
        .max(1.0);
    let inset_x = (card_w * inset.clamp(0.0, 0.45)).min((card_w - width).max(0.0) * 0.5);
    let inset_y = inset_x.min((card_h - height).max(0.0) * 0.5);
    let x = (position.0.clamp(0.0, 1.0) * card_w - width * 0.5)
        .clamp(inset_x, (card_w - inset_x - width).max(inset_x));
    let y = (position.1.clamp(0.0, 1.0) * card_h - height * 0.5)
        .clamp(inset_y, (card_h - inset_y - height).max(inset_y));
    (left + x, top + y, width, height)
}

/// Paint a selected watermark in source-card coordinates.  The source image
/// is never sampled from disk here: callers retain it for live preview and
/// resolve it once for export, keeping the two compositor paths equivalent.
fn paint_motion_watermark(
    context: &Context,
    layout: CardLayout,
    motion: &MotionState,
    watermark: &ImageSurface,
) {
    if motion.watermark.image_file_name.is_none() {
        return;
    }
    let source_w = f64::from(watermark.width().max(1));
    let source_h = f64::from(watermark.height().max(1));
    let (x, y, width, height) = motion_watermark_rect(
        (0.0, 0.0, layout.img_w(), layout.img_h()),
        motion.watermark.position,
        motion.watermark.size,
        motion.watermark.inset,
        (source_w, source_h),
    );
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

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
    let width = (layout.img_w() * motion.watermark.size.clamp(0.02, 0.8)).max(1.0);
    let height = (width * source_h / source_w)
        .min(layout.img_h() * 0.8)
        .max(1.0);
    let inset = (layout.img_w() * motion.watermark.inset.clamp(0.0, 0.45))
        .min((layout.img_w() - width).max(0.0) * 0.5);
    let inset_y = inset.min((layout.img_h() - height).max(0.0) * 0.5);
    let x = (motion.watermark.position.0.clamp(0.0, 1.0) * layout.img_w() - width * 0.5)
        .clamp(inset, (layout.img_w() - inset - width).max(inset));
    let y = (motion.watermark.position.1.clamp(0.0, 1.0) * layout.img_h() - height * 0.5)
        .clamp(inset_y, (layout.img_h() - inset_y - height).max(inset_y));
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

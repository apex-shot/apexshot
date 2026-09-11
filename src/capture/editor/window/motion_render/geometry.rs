/// The rectangle the Motion card is laid out inside: the background fill's
/// area. Exports lay out against the full frame; the editor lays out against
/// the bounded scene panel so padding can never push the card outside the
/// background. Both rectangles share the viewport's center.
#[derive(Clone, Copy)]
pub(super) struct MotionStage {
    pub bounds_w: f64,
    pub bounds_h: f64,
    pub center_x: f64,
    pub center_y: f64,
}

impl MotionStage {
    pub(super) fn frame(width: f64, height: f64) -> Self {
        Self {
            bounds_w: width,
            bounds_h: height,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }

    pub(super) fn preview(width: f64, height: f64, frame_aspect: Option<f64>) -> Self {
        let (_, _, bounds_w, bounds_h) = motion_scene_bounds(width, height);
        let (bounds_w, bounds_h) = fit_stage_aspect(bounds_w, bounds_h, frame_aspect);
        Self {
            bounds_w,
            bounds_h,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }
}

/// Largest centered rectangle with the requested aspect inside the given
/// bounds; `None` (Standard) keeps the bounds unchanged.
pub(super) fn fit_stage_aspect(bounds_w: f64, bounds_h: f64, aspect: Option<f64>) -> (f64, f64) {
    let Some(aspect) = aspect else {
        return (bounds_w, bounds_h);
    };
    let width = bounds_w.min(bounds_h * aspect);
    (width, width / aspect)
}

/// The preview scene panel rectangle for the current Frame preset: the inset
/// scene bounds re-fitted to the preset's aspect. The backdrop clip and the
/// foreground stage must agree on this rectangle.
pub(super) fn motion_preview_scene_rect(
    width: f64,
    height: f64,
    frame_aspect: Option<f64>,
) -> (f64, f64, f64, f64) {
    let (x, y, bounds_w, bounds_h) = motion_scene_bounds(width, height);
    let (scene_w, scene_h) = fit_stage_aspect(bounds_w, bounds_h, frame_aspect);
    (
        x + (bounds_w - scene_w) / 2.0,
        y + (bounds_h - scene_h) / 2.0,
        scene_w,
        scene_h,
    )
}

#[derive(Clone, Copy)]
struct CardLayout {
    img_w: f64,
    img_h: f64,
    fit: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
}

impl CardLayout {
    fn with_padding(
        surface: &ImageSurface,
        stage: MotionStage,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
        padding: f64,
    ) -> Self {
        let img_w = surface.width().max(1) as f64;
        let img_h = surface.height().max(1) as f64;
        let pad = padding.clamp(0.0, (stage.bounds_w.min(stage.bounds_h) - 2.0).max(0.0));
        let fit = ((stage.bounds_w - pad) / img_w)
            .min((stage.bounds_h - pad) / img_h)
            .clamp(0.05, 1.0);
        let (cx, cy) = motion_card_center(img_w, img_h, fit, stage, transform, zoom_anchor);
        Self {
            img_w,
            img_h,
            fit,
            transform,
            cx,
            cy,
        }
    }

    fn project(&self, image_x: f64, image_y: f64) -> (f64, f64) {
        let hw = self.img_w * self.fit * self.transform.scale / 2.0;
        let hh = self.img_h * self.fit * self.transform.scale / 2.0;
        let depth = card_depth(hw, hh, self.transform.perspective);
        let (x, y) = project_point(
            (image_x / self.img_w * 2.0 - 1.0) * hw,
            (image_y / self.img_h * 2.0 - 1.0) * hh,
            self.transform,
            depth,
        );
        (self.cx + x, self.cy + y)
    }

    fn local_matrix(&self, image_x: f64, image_y: f64) -> Option<Matrix> {
        let origin = self.project(image_x, image_y);
        let x = self.project((image_x + 1.0).min(self.img_w), image_y);
        let y = self.project(image_x, (image_y + 1.0).min(self.img_h));
        let xx = x.0 - origin.0;
        let yx = x.1 - origin.1;
        let xy = y.0 - origin.0;
        let yy = y.1 - origin.1;
        if (xx * yy - xy * yx).abs() < 1e-8 {
            return None;
        }
        Some(Matrix::new(
            xx,
            yx,
            xy,
            yy,
            origin.0 - xx * image_x - xy * image_y,
            origin.1 - yx * image_x - yy * image_y,
        ))
    }
}

/// Convert a pointer in the Motion preview back into the source artboard.
/// A short Newton refinement keeps placement accurate for the non-linear
/// perspective projection used by the card mesh.
pub fn view_point_to_motion_text_position(
    surface: &ImageSurface,
    stage: MotionStage,
    padding: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    view_x: f64,
    view_y: f64,
) -> (f64, f64) {
    let layout = CardLayout::with_padding(surface, stage, transform, zoom_anchor, padding);
    let mut best = (0.5, 0.5);
    let mut best_distance = f64::INFINITY;
    for row in 0..=12 {
        for column in 0..=12 {
            let u = column as f64 / 12.0;
            let v = row as f64 / 12.0;
            let point = layout.project(u * layout.img_w, v * layout.img_h);
            let distance = (point.0 - view_x).powi(2) + (point.1 - view_y).powi(2);
            if distance < best_distance {
                best_distance = distance;
                best = (u, v);
            }
        }
    }
    for _ in 0..6 {
        let point = layout.project(best.0 * layout.img_w, best.1 * layout.img_h);
        let du = layout.project(
            ((best.0 + 0.002).min(1.0)) * layout.img_w,
            best.1 * layout.img_h,
        );
        let dv = layout.project(
            best.0 * layout.img_w,
            ((best.1 + 0.002).min(1.0)) * layout.img_h,
        );
        let j00 = (du.0 - point.0) / 0.002;
        let j10 = (du.1 - point.1) / 0.002;
        let j01 = (dv.0 - point.0) / 0.002;
        let j11 = (dv.1 - point.1) / 0.002;
        let det = j00 * j11 - j01 * j10;
        if det.abs() < 1e-7 {
            break;
        }
        let dx = point.0 - view_x;
        let dy = point.1 - view_y;
        best.0 = (best.0 - (j11 * dx - j01 * dy) / det).clamp(0.0, 1.0);
        best.1 = (best.1 - (-j10 * dx + j00 * dy) / det).clamp(0.0, 1.0);
    }
    (best.0.clamp(0.05, 0.95), best.1.clamp(0.05, 0.95))
}

/// Preview-only scene panel: an inset rectangle that keeps the editor's
/// checkerboard visible around the Motion fill as its boundary.
fn motion_scene_bounds(width: f64, height: f64) -> (f64, f64, f64, f64) {
    const MARGIN: f64 = 24.0;
    let w = width.max(0.0);
    let h = height.max(0.0);
    let inset = MARGIN.min(w.min(h) * 0.25);
    (
        inset,
        inset,
        (w - inset * 2.0).max(1.0),
        (h - inset * 2.0).max(1.0),
    )
}

fn rounded_rectangle(context: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width.min(height) * 0.5).max(0.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2 * 3.0,
    );
    context.close_path();
}

/// Keep the selected source point stationary while a camera scales in. The
/// transform's ordinary X/Y position is applied first; the anchor offset then
/// compensates only for zoom. Preview, export, title painting, and inverse
/// title placement all use this same center calculation.
fn motion_card_center(
    img_w: f64,
    img_h: f64,
    fit: f64,
    stage: MotionStage,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
) -> (f64, f64) {
    // Position is the card center in the background's coordinate space. The
    // pad edges therefore map directly to the background edges at every scale
    // instead of behaving like a small translation or a zoom-dependent pan.
    let cx = stage.center_x + transform.pos_x * stage.bounds_w * 0.5;
    let cy = stage.center_y + transform.pos_y * stage.bounds_h * 0.5;
    let (anchor_x, anchor_y) = (zoom_anchor.0.clamp(0.0, 1.0), zoom_anchor.1.clamp(0.0, 1.0));
    if (transform.scale - 1.0).abs() < f64::EPSILON
        || ((anchor_x - 0.5).abs() < f64::EPSILON && (anchor_y - 0.5).abs() < f64::EPSILON)
    {
        return (cx, cy);
    }
    let half_w = img_w * fit / 2.0;
    let half_h = img_h * fit / 2.0;
    let local_x = (anchor_x * 2.0 - 1.0) * half_w;
    let local_y = (anchor_y * 2.0 - 1.0) * half_h;
    let mut unzoomed = transform;
    unzoomed.scale = 1.0;
    let before = project_point(
        local_x,
        local_y,
        unzoomed,
        card_depth(half_w, half_h, transform.perspective),
    );
    let scaled_half_w = half_w * transform.scale;
    let scaled_half_h = half_h * transform.scale;
    let after = project_point(
        local_x * transform.scale,
        local_y * transform.scale,
        transform,
        card_depth(scaled_half_w, scaled_half_h, transform.perspective),
    );
    (cx + before.0 - after.0, cy + before.1 - after.1)
}

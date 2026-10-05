/// The rectangle the Motion card is laid out inside: the background fill's
/// area. Exports lay out against the full frame; the editor lays out against
/// the bounded scene panel so padding can never push the card outside the
/// background. Both rectangles share the viewport's center.
#[derive(Clone, Copy)]
pub(crate) struct MotionStage {
    pub bounds_w: f64,
    pub bounds_h: f64,
    pub center_x: f64,
    pub center_y: f64,
}

impl MotionStage {
    pub(crate) fn frame(width: f64, height: f64) -> Self {
        Self {
            bounds_w: width,
            bounds_h: height,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }

    /// A scene rectangle at an arbitrary origin: the static canvas is laid out
    /// inside a padded viewport, so its fill is not anchored at (0, 0).
    pub(crate) fn rect_at(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            bounds_w: width,
            bounds_h: height,
            center_x: x + width / 2.0,
            center_y: y + height / 2.0,
        }
    }

    /// The editor's scene panel for a composition canvas, capped at the
    /// canvas's native size like Static's own canvas scale.
    pub(super) fn preview(width: f64, height: f64, canvas_w: f64, canvas_h: f64) -> Self {
        let (_, _, bounds_w, bounds_h) =
            motion_preview_scene_rect(width, height, canvas_w, canvas_h);
        Self {
            bounds_w,
            bounds_h,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }

    fn composition_fit(&self, canvas_w: f64, canvas_h: f64) -> f64 {
        (self.bounds_w / canvas_w.max(1.0))
            .min(self.bounds_h / canvas_h.max(1.0))
            .min(1.0)
    }
}

/// Largest centered rectangle with the requested aspect inside the given
/// bounds; `None` keeps the bounds unchanged.
pub(super) fn fit_stage_aspect(bounds_w: f64, bounds_h: f64, aspect: Option<f64>) -> (f64, f64) {
    let Some(aspect) = aspect else {
        return (bounds_w, bounds_h);
    };
    let width = bounds_w.min(bounds_h * aspect);
    (width, width / aspect)
}

/// The preview scene panel rectangle for a composition canvas, capped at the
/// canvas's native size. Backdrop, foreground, controls and cache all use it.
pub(super) fn motion_preview_scene_rect(
    width: f64,
    height: f64,
    canvas_w: f64,
    canvas_h: f64,
) -> (f64, f64, f64, f64) {
    let (x, y, bounds_w, bounds_h) = motion_scene_bounds(width, height);
    let canvas_w = canvas_w.max(1.0);
    let canvas_h = canvas_h.max(1.0);
    let (fitted_w, _) = fit_stage_aspect(bounds_w, bounds_h, Some(canvas_w / canvas_h));
    let scale = (fitted_w / canvas_w).min(1.0);
    let scene_w = canvas_w * scale;
    let scene_h = canvas_h * scale;
    (
        x + (bounds_w - scene_w) / 2.0,
        y + (bounds_h - scene_h) / 2.0,
        scene_w,
        scene_h,
    )
}

/// Original source dimensions behind a Motion card texture. The preview draws
/// a downscaled texture (`card_scale`) while exports draw it full size; every
/// card-space value has to be computed against the source, or the fixed-pixel
/// frame overhangs would shrink with the preview texture.
pub(super) fn motion_source_size(surface: &ImageSurface, card_scale: f64) -> (f64, f64) {
    let scale = if card_scale.is_finite() && card_scale > 0.0 {
        card_scale
    } else {
        1.0
    };
    (
        (f64::from(surface.width().max(1)) / scale).round().max(1.0),
        (f64::from(surface.height().max(1)) / scale)
            .round()
            .max(1.0),
    )
}

/// The composition canvas for a still: source dimensions, the shared
/// Appearance and the Frame ratio in one place, for the backdrop clip, the
/// card, the controls and the export size.
pub(super) fn motion_canvas(
    source_w: f64,
    source_h: f64,
    appearance: &MotionAppearance,
    frame: &MotionFrame,
) -> CompositionLayout {
    motion_background_composition(source_w, source_h, appearance, frame.effective_aspect())
        .compute()
}

#[derive(Clone, Copy)]
pub(crate) struct CardLayout {
    /// Static's shared card composition for the original source dimensions:
    /// the one description every Motion layer reads, so the normalized image
    /// rectangle cannot drift between the two editors.
    composition: CompositionLayout,
    stage: MotionStage,
    /// Stage pixels per composition-canvas pixel.
    canvas_fit: f64,
    /// Composition image center in stage pixels, before the camera transform.
    base_x: f64,
    base_y: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
}

/// Reference scale shared with the Static canvas: slider units are defined
/// against a 400px long edge, so a padding of 40 means 10% surround per side
/// no matter how large the screenshot is. Deliberately unclamped, exactly like
/// `BackgroundComposition`'s `scale_factor`: a clamp here would make Motion
/// (preview and export) render a different surround than Static for the same
/// stored value on very large or very small images.
pub(super) fn motion_reference_scale(img_w: f64, img_h: f64) -> f64 {
    img_w.max(img_h) / 400.0
}

/// Stage position of the composition's image center: the canvas is fitted and
/// centered in the stage exactly like Static centers its canvas in the
/// viewport, then the image's own inset and alignment place the card inside
/// it.
fn motion_composition_center(
    composition: &CompositionLayout,
    canvas_fit: f64,
    stage: MotionStage,
) -> (f64, f64) {
    let origin_x = stage.center_x - composition.canvas_width * canvas_fit / 2.0;
    let origin_y = stage.center_y - composition.canvas_height * canvas_fit / 2.0;
    (
        origin_x + (composition.image_rect.x + composition.image_rect.width / 2.0) * canvas_fit,
        origin_y + (composition.image_rect.y + composition.image_rect.height / 2.0) * canvas_fit,
    )
}

impl CardLayout {
    pub(crate) fn new(
        composition: CompositionLayout,
        stage: MotionStage,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> Self {
        let canvas_fit = stage.composition_fit(composition.canvas_width, composition.canvas_height);
        let (base_x, base_y) = motion_composition_center(&composition, canvas_fit, stage);
        let mut layout = Self {
            composition,
            stage,
            canvas_fit,
            base_x,
            base_y,
            transform,
            cx: base_x,
            cy: base_y,
        };
        let (cx, cy) = layout.pose_center(transform, zoom_anchor);
        layout.cx = cx;
        layout.cy = cy;
        layout
    }

    /// The same composition under a different camera pose. Motion blur
    /// accumulates several poses of one card, and the zoom anchor depends on
    /// the pose, so each subframe re-centers the unchanged base layout.
    fn with_pose(&self, transform: MotionTransform, zoom_anchor: (f64, f64)) -> Self {
        let (cx, cy) = self.pose_center(transform, zoom_anchor);
        Self {
            transform,
            cx,
            cy,
            ..*self
        }
    }

    /// Camera framing in the background's coordinate space: pad top-right
    /// shows top-right, so the card moves opposite the camera and the pad
    /// edges map to the background edges at every scale.
    fn pose_center(&self, transform: MotionTransform, zoom_anchor: (f64, f64)) -> (f64, f64) {
        let cx = self.base_x - transform.pos_x * self.stage.bounds_w * 0.5;
        let cy = self.base_y - transform.pos_y * self.stage.bounds_h * 0.5;
        let (anchor_x, anchor_y) = (zoom_anchor.0.clamp(0.0, 1.0), zoom_anchor.1.clamp(0.0, 1.0));
        if (transform.scale - 1.0).abs() < f64::EPSILON
            || ((anchor_x - 0.5).abs() < f64::EPSILON && (anchor_y - 0.5).abs() < f64::EPSILON)
        {
            return (cx, cy);
        }
        let half_w = self.img_w() * self.source_fit() / 2.0;
        let half_h = self.img_h() * self.source_fit() / 2.0;
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

    /// Original source width of the card.
    fn img_w(&self) -> f64 {
        self.composition.image_rect.width / self.composition.draw_scale
    }

    /// Original source height of the card.
    fn img_h(&self) -> f64 {
        self.composition.image_rect.height / self.composition.draw_scale
    }

    /// Stage pixels per source pixel, including the composition's draw scale.
    fn source_fit(&self) -> f64 {
        self.composition.draw_scale * self.canvas_fit
    }

    fn project(&self, image_x: f64, image_y: f64) -> (f64, f64) {
        let hw = self.composition.image_rect.width * self.canvas_fit * self.transform.scale / 2.0;
        let hh = self.composition.image_rect.height * self.canvas_fit * self.transform.scale / 2.0;
        let depth = card_depth(hw, hh, self.transform.perspective);
        let (x, y) = project_point(
            (image_x / self.img_w() * 2.0 - 1.0) * hw,
            (image_y / self.img_h() * 2.0 - 1.0) * hh,
            self.transform,
            depth,
        );
        (self.cx + x, self.cy + y)
    }

    fn local_matrix(&self, image_x: f64, image_y: f64) -> Option<Matrix> {
        let origin = self.project(image_x, image_y);
        let x = self.project(image_x + 1.0, image_y);
        let y = self.project(image_x, image_y + 1.0);
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

    fn unproject(&self, view_x: f64, view_y: f64) -> (f64, f64) {
        let mut best = (0.5, 0.5);
        let mut best_distance = f64::INFINITY;
        for row in 0..=12 {
            for column in 0..=12 {
                let u = column as f64 / 12.0;
                let v = row as f64 / 12.0;
                let point = self.project(u * self.img_w(), v * self.img_h());
                let distance = (point.0 - view_x).powi(2) + (point.1 - view_y).powi(2);
                if distance < best_distance {
                    best_distance = distance;
                    best = (u, v);
                }
            }
        }
        for _ in 0..6 {
            let point = self.project(best.0 * self.img_w(), best.1 * self.img_h());
            let du = self.project((best.0 + 0.002) * self.img_w(), best.1 * self.img_h());
            let dv = self.project(best.0 * self.img_w(), (best.1 + 0.002) * self.img_h());
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
        best
    }
}

/// Maximum travel of any card corner between two poses, in stage pixels.
/// The motion blur renderer sizes its temporal sample count from this so the
/// smear gradient stays continuous even during fast camera moves.
pub(super) fn card_corner_travel(
    base: CardLayout,
    from: MotionTransform,
    from_anchor: (f64, f64),
    to: MotionTransform,
    to_anchor: (f64, f64),
) -> f64 {
    let corners = |transform: MotionTransform, anchor: (f64, f64)| {
        let layout = base.with_pose(transform, anchor);
        [
            layout.project(0.0, 0.0),
            layout.project(layout.img_w(), 0.0),
            layout.project(layout.img_w(), layout.img_h()),
            layout.project(0.0, layout.img_h()),
        ]
    };
    let from = corners(from, from_anchor);
    let to = corners(to, to_anchor);
    from.iter()
        .zip(to.iter())
        .map(|(a, b)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
        .fold(0.0, f64::max)
}

/// Card layout for the interactive Motion preview: the widget's allocated
/// size and the still's full-resolution surface, laid out in the same
/// composition the renderer draws. Pointer placement and title hit testing
/// share it, so a click lands where the title is drawn.
pub(crate) fn motion_preview_card_layout(
    card: &ImageSurface,
    width: f64,
    height: f64,
    motion: &MotionState,
    time: f64,
) -> CardLayout {
    let (source_w, source_h) = motion_source_size(card, 1.0);
    let composition = motion_canvas(source_w, source_h, &motion.appearance, &motion.frame);
    let stage = MotionStage::preview(
        width,
        height,
        composition.canvas_width,
        composition.canvas_height,
    );
    CardLayout::new(
        composition,
        stage,
        motion.sample(time),
        motion.zoom_anchor_at(time),
    )
}

/// Convert a pointer in the Motion preview back into the source artboard.
/// A short Newton refinement keeps placement accurate for the non-linear
/// perspective projection used by the card mesh.
#[cfg(test)]
pub fn view_point_to_motion_text_position(
    layout: CardLayout,
    view_x: f64,
    view_y: f64,
) -> (f64, f64) {
    let best = layout.unproject(view_x, view_y);
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
    // Motion card shares the static smooth-corner outline so preview, export
    // and thumbnails agree; see render::rounded_rect_path.
    crate::capture::editor::render::rounded_rect_path(context, x, y, width, height, radius);
}

/// Stage-space outline of a card-space rounded rect under the live camera.
///
/// Borders, the Liquid rim and inset bands must hug the card edge even
/// mid-clip. Tracing the projected quad instead draws a sharp frame around
/// a rounded image while the clip plays — the image keeps its radius via
/// texture alpha, so the frame visibly loses it and snaps back when
/// playback stops. Sampling the same superellipse corners as
/// `render::rounded_rect_path` and pushing them through the perspective
/// projection keeps every pose consistent. Straight edges need no samples
/// (projective transforms preserve lines); `radius` uses the same
/// card-space units as the mesh grid (`hw`/`hh` include fit and scale).
fn projected_rounded_rect_points(
    half_w: f64,
    half_h: f64,
    radius: f64,
    transform: MotionTransform,
    depth: f64,
    cx: f64,
    cy: f64,
) -> Vec<(f64, f64)> {
    const SEGMENTS_PER_CORNER: usize = 10;
    let half_w = half_w.max(0.0);
    let half_h = half_h.max(0.0);
    let radius = radius.clamp(0.0, half_w.min(half_h));
    let project = |x: f64, y: f64| {
        let (px, py) = project_point(x, y, transform, depth);
        (cx + px, cy + py)
    };
    if radius <= 0.001 {
        // Sharp-mitered frame, matching the zero-radius flat path.
        return [
            (half_w, -half_h),
            (half_w, half_h),
            (-half_w, half_h),
            (-half_w, -half_h),
        ]
        .into_iter()
        .map(|(x, y)| project(x, y))
        .collect();
    }
    // (center_x, center_y, start_angle) in path order: TR, BR, BL, TL.
    let corners = [
        (
            half_w - radius,
            -half_h + radius,
            -std::f64::consts::FRAC_PI_2,
        ),
        (half_w - radius, half_h - radius, 0.0),
        (
            -half_w + radius,
            half_h - radius,
            std::f64::consts::FRAC_PI_2,
        ),
        (-half_w + radius, -half_h + radius, std::f64::consts::PI),
    ];
    let mut out = Vec::with_capacity(4 * (SEGMENTS_PER_CORNER + 1));
    for (center_x, center_y, start) in corners {
        for step in 0..=SEGMENTS_PER_CORNER {
            let angle =
                start + (step as f64) / (SEGMENTS_PER_CORNER as f64) * std::f64::consts::FRAC_PI_2;
            let (sine, cosine) = angle.sin_cos();
            out.push(project(
                center_x + radius * cosine.signum() * cosine.abs().sqrt(),
                center_y + radius * sine.signum() * sine.abs().sqrt(),
            ));
        }
    }
    out
}

/// Cairo path through projected outline points.
fn path_through_points(context: &Context, points: &[(f64, f64)]) {
    let mut first = true;
    for (x, y) in points {
        if first {
            context.move_to(*x, *y);
            first = false;
        } else {
            context.line_to(*x, *y);
        }
    }
    context.close_path();
}

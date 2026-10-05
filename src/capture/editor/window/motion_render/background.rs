const PREVIEW_BACKGROUND_BLUR_MAX_EDGE: f64 = 360.0;
const EXPORT_BACKGROUND_BLUR_MAX_EDGE: f64 = 720.0;

fn paint_backdrop(
    context: &Context,
    width: i32,
    height: i32,
    motion: &MotionState,
    background_surface: Option<&ImageSurface>,
    checkerboard: bool,
    prefers_dark: bool,
    canvas: Option<(f64, f64)>,
) {
    let appearance = &motion.appearance;
    // The editor canvas keeps its checkerboard; a chosen fill paints inside a
    // bounded scene panel so the fill reads as a layer with visible
    // boundaries. Exports have no canvas: fills cover the full frame.
    let scene = if checkerboard {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
        let (canvas_w, canvas_h) = canvas.unwrap_or((f64::from(width), f64::from(height)));
        Some(motion_preview_scene_rect(
            f64::from(width),
            f64::from(height),
            canvas_w,
            canvas_h,
        ))
    } else {
        None
    };
    // The preview shows the plain checkerboard until a fill is chosen;
    // the black scene for an unset fill only applies to exports.
    if checkerboard
        && matches!(
            appearance.background_fill_type,
            MotionBackgroundFillType::None
        )
    {
        return;
    }
    let _ = context.save();
    if let Some((x, y, scene_w, scene_h)) = scene {
        context.rectangle(x, y, scene_w, scene_h);
        context.clip();
    }
    match appearance.background_fill_type {
        // Explicit Motion-mode rule: None is a black scene, not
        // the editor's transparent checkerboard.
        MotionBackgroundFillType::None => {
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().ok();
        }
        MotionBackgroundFillType::Color => {
            let [r, g, b, a] = appearance.background_color;
            context.set_source_rgba(r, g, b, a);
            context.paint().ok();
        }
        MotionBackgroundFillType::Gradient => {
            let (x, y, w, h) = scene.unwrap_or((0.0, 0.0, f64::from(width), f64::from(height)));
            // Rasterize through the video editor's shared renderer: a Cairo
            // two-stop line would be a second description of the same fill and
            // could not honor the stop positions, kind, angle, or reversal this
            // spec carries. The raster is built once per draw, which is fine
            // because the editor caches the whole backdrop layer.
            let bitmap = render_gradient(
                &appearance.gradient,
                w.ceil().max(1.0) as u32,
                h.ceil().max(1.0) as u32,
            );
            let surface = crate::recording::editor::window::custom_wallpaper_popover::bitmap_to_surface(&bitmap);
            let _ = context.save();
            context.translate(x, y);
            context.set_source_surface(&surface, 0.0, 0.0).ok();
            context.paint().ok();
            let _ = context.restore();
        }
        MotionBackgroundFillType::Wallpaper | MotionBackgroundFillType::Image => {
            let (x, y, scene_w, scene_h) =
                scene.unwrap_or((0.0, 0.0, f64::from(width), f64::from(height)));
            if let Some(surface) = background_surface {
                paint_image_background_with_max_edge(
                    context,
                    surface,
                    x,
                    y,
                    scene_w,
                    scene_h,
                    appearance.background_blur,
                    if checkerboard {
                        PREVIEW_BACKGROUND_BLUR_MAX_EDGE
                    } else {
                        EXPORT_BACKGROUND_BLUR_MAX_EDGE
                    },
                );
            } else {
                // A wallpaper may still be decoding in a worker. Drawing must
                // never synchronously read it from disk and freeze the UI.
                context.set_source_rgb(0.0, 0.0, 0.0);
                context.paint().ok();
            }
        }
    }
    // Same grain field as the static canvas and the static export, so the
    // Background tool reads identically in both editors.
    crate::capture::editor::render::paint_background_noise(
        context,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        appearance.background_noise,
    );
    context.restore().ok();
}

pub(super) fn load_motion_background_surface(path: &str) -> Option<ImageSurface> {
    let image = image::open(path).ok()?.into_rgba8();
    crate::capture::editor::render::rgba_image_to_surface(&image)
}

/// Preview-sized sibling of [`load_motion_background_surface`]. The Motion
/// runtime and the inspector only ever draw backgrounds at screen resolution,
/// so they decode bounded (JPEG via DCT scale) and leave full resolution to
/// the export path.
pub(super) fn load_motion_background_preview_surface(
    path: &str,
    max_edge: u32,
) -> Option<ImageSurface> {
    let image = crate::capture::editor::window::background_panel::load_background_preview_image(
        std::path::Path::new(path),
        max_edge,
    )?;
    crate::capture::editor::render::rgba_image_to_surface(&image)
}

/// Export-default wrapper kept for tests; release code always passes an
/// explicit `max_render_edge`.
#[cfg(test)]
fn paint_image_background(
    context: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    blur: f64,
) {
    paint_image_background_with_max_edge(
        context,
        surface,
        x,
        y,
        width,
        height,
        blur,
        EXPORT_BACKGROUND_BLUR_MAX_EDGE,
    );
}

fn paint_image_background_with_max_edge(
    context: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    blur: f64,
    max_render_edge: f64,
) {
    let blur = blur.clamp(0.0, 1.0);
    if blur <= 0.001 {
        paint_cover_fit_at(context, surface, x, y, width, height, Filter::Good);
        return;
    }

    // Render the cover-fitted image once at a bounded resolution, then blur
    // its pixels. Downsampling alone only softened resampling artifacts and
    // did not produce a reliable background blur.
    let render_scale = (max_render_edge / width.max(height)).min(1.0);
    let render_w = (width * render_scale).ceil().max(1.0) as i32;
    let render_h = (height * render_scale).ceil().max(1.0) as i32;
    let Ok(mut rendered) = ImageSurface::create(Format::ARgb32, render_w, render_h) else {
        paint_cover_fit_at(context, surface, x, y, width, height, Filter::Good);
        return;
    };
    if let Ok(rendered_context) = Context::new(&rendered) {
        paint_cover_fit(&rendered_context, surface, render_w, render_h, Filter::Good);
    }
    rendered.flush();
    let stride = rendered.stride() as usize;
    let mut rgba = {
        let Ok(data) = rendered.data() else {
            return;
        };
        crate::capture::editor::render::cairo_argb_to_rgba_image(
            render_w as u32,
            render_h as u32,
            stride,
            data.as_ref(),
        )
    };
    let blur_rect = crate::capture::editor::types::Rect {
        x: 0,
        y: 0,
        width: render_w,
        height: render_h,
    };
    let radius = (blur * 32.0 * render_scale).max(1.0);
    for _ in 0..3 {
        crate::capture::editor::render::apply_blur_rect(&mut rgba, blur_rect, radius, true);
    }
    let Some(blurred) = crate::capture::editor::render::rgba_image_to_surface(&rgba) else {
        return;
    };
    let _ = context.save();
    context.translate(x, y);
    context.scale(width / f64::from(render_w), height / f64::from(render_h));
    context.set_source_surface(&blurred, 0.0, 0.0).ok();
    context.source().set_filter(Filter::Bilinear);
    context.paint().ok();
    context.restore().ok();
}

fn paint_cover_fit_at(
    context: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    filter: Filter,
) {
    let _ = context.save();
    context.translate(x, y);
    paint_cover_fit(
        context,
        surface,
        width.ceil() as i32,
        height.ceil() as i32,
        filter,
    );
    context.restore().ok();
}

/// Draw a surface cover-fitted (scaled to fill, center-cropped) into the
/// given rectangle.
fn paint_cover_fit(
    context: &Context,
    surface: &ImageSurface,
    width: i32,
    height: i32,
    filter: Filter,
) {
    let source_w = surface.width().max(1) as f64;
    let source_h = surface.height().max(1) as f64;
    let scale = (f64::from(width) / source_w).max(f64::from(height) / source_h);
    let drawn_w = source_w * scale;
    let drawn_h = source_h * scale;
    let _ = context.save();
    context.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
    context.clip();
    context.translate(
        (f64::from(width) - drawn_w) * 0.5,
        (f64::from(height) - drawn_h) * 0.5,
    );
    context.scale(scale, scale);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(filter);
    context.paint().ok();
    context.restore().ok();
}

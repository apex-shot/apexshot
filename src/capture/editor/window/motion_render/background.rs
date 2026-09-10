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
        Some(motion_scene_bounds(f64::from(width), f64::from(height)))
    } else {
        None
    };
    // The preview shows the plain checkerboard until a fill is chosen;
    // Shotbase's black scene for an unset fill only applies to exports.
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
        // Shotbase's explicit Motion-mode rule: None is a black scene, not
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
            let [r1, g1, b1, a1] = appearance.gradient_color_1;
            let [r2, g2, b2, a2] = appearance.gradient_color_2;
            let (x, y, w, h) = scene.unwrap_or((0.0, 0.0, f64::from(width), f64::from(height)));
            let gradient = LinearGradient::new(x, y, x + w, y + h);
            gradient.add_color_stop_rgba(0.0, r1, g1, b1, a1);
            gradient.add_color_stop_rgba(1.0, r2, g2, b2, a2);
            context.set_source(&gradient).ok();
            context.paint().ok();
        }
        MotionBackgroundFillType::Wallpaper | MotionBackgroundFillType::Image => {
            let path = match appearance.background_fill_type {
                MotionBackgroundFillType::Wallpaper => appearance.wallpaper_image_name.as_deref(),
                MotionBackgroundFillType::Image => appearance.custom_background_image.as_deref(),
                _ => None,
            };
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
            } else if let Some(surface) = path.and_then(load_motion_background_surface) {
                paint_image_background_with_max_edge(
                    context,
                    &surface,
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
                context.set_source_rgb(0.0, 0.0, 0.0);
                context.paint().ok();
            }
        }
    }
    paint_background_noise(context, width, height, appearance.background_noise);
    context.restore().ok();
}

pub(super) fn load_motion_background_surface(path: &str) -> Option<ImageSurface> {
    let image = image::open(path).ok()?.into_rgba8();
    crate::capture::editor::render::rgba_image_to_surface(&image)
}

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

fn paint_background_noise(context: &Context, width: i32, height: i32, amount: f64) {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.001 {
        return;
    }
    // Fixed pseudo-noise keeps every frame stable (and therefore exportable)
    // rather than shimmering as the Motion playhead advances.
    let step = 4;
    for y in (0..height.max(0)).step_by(step) {
        for x in (0..width.max(0)).step_by(step) {
            let hash =
                ((x as u32).wrapping_mul(73_856_093)) ^ ((y as u32).wrapping_mul(19_349_663));
            let light = if hash & 1 == 0 { 1.0 } else { 0.0 };
            context.set_source_rgba(light, light, light, amount * 0.045);
            context.rectangle(f64::from(x), f64::from(y), step as f64, step as f64);
            context.fill().ok();
        }
    }
}

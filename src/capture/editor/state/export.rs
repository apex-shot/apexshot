use super::super::composition::{BackgroundComposition, CompositionLayout, FloatRect};
use super::super::pen_weight::{HighlighterMode, PenWeight};
use super::super::render::{
    apply_background_blur, apply_background_noise, apply_blur_rect, cairo_argb_to_rgba_image,
    glass_layer, rgba_image_to_surface, GlassLook, GlassRing, BACKGROUND_BLUR_MAX_RADIUS,
};
use super::super::types::{
    AnnotationAction, BackgroundStyle, DrawColor, EditorError, FrameStyle, Rect,
};
use super::super::window::motion_render::{paint_motion_scene_shadow, MotionStage};
use super::EditorState;
use crate::recording::editor::model::MotionSceneShadow;
use image::RgbaImage;
use std::path::Path;

/// Backing sheets (Stack looks, Retro window) painted behind the card.
/// Shared by the wallpaper export and the frameless export so both agree.
fn paint_frame_backings(
    mut canvas: RgbaImage,
    image_rect: &FloatRect,
    radius: f64,
    style: FrameStyle,
) -> Result<RgbaImage, EditorError> {
    let spec = style.spec();
    let backings = [spec.backing1, spec.backing2];
    if backings.iter().any(|b| b.is_some()) {
        if let Some(mut surface) = rgba_image_to_surface(&canvas) {
            {
                let context = gtk4::cairo::Context::new(&surface)
                    .map_err(|e| EditorError::ImageSave(e.to_string()))?;
                for backing in backings.into_iter().flatten() {
                    let _ = context.save();
                    if backing.center_pivot {
                        context.translate(
                            image_rect.x + image_rect.width / 2.0 + backing.offset_x,
                            image_rect.y + image_rect.height / 2.0 + backing.offset_y,
                        );
                        context.rotate(backing.rotation_deg.to_radians());
                        context.translate(-image_rect.width / 2.0, -image_rect.height / 2.0);
                    } else {
                        // Pivot at the hidden bottom-right corner so
                        // the slant fans top-left (Stack2).
                        context.translate(
                            image_rect.x + backing.offset_x + image_rect.width,
                            image_rect.y + backing.offset_y + image_rect.height,
                        );
                        context.rotate(backing.rotation_deg.to_radians());
                        context.translate(-image_rect.width, -image_rect.height);
                    }
                    draw_rounded_rect_path(
                        &context,
                        0.0,
                        0.0,
                        image_rect.width,
                        image_rect.height,
                        radius.max(0.0),
                    );
                    context.set_source_rgba(
                        backing.color.r,
                        backing.color.g,
                        backing.color.b,
                        backing.color.a,
                    );
                    let _ = context.fill();
                    let _ = context.restore();
                }
            }
            surface.flush();
            let stride = gtk4::cairo::Format::ARgb32
                .stride_for_width(canvas.width())
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            let surface_data = surface
                .data()
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            canvas = cairo_argb_to_rgba_image(
                canvas.width(),
                canvas.height(),
                stride as usize,
                surface_data.as_ref(),
            );
        }
    }
    Ok(canvas)
}

/// The Scene Shadows layer over the composed canvas. It calls the same
/// painter as the Motion compositor and the canvas preview, so the still
/// export cannot drift from what the editor shows. The `underlay` pass lands
/// on the fill (below the backings, drop shadow and card); the overlay pass
/// is composited above the annotations.
fn paint_scene_shadow_layer(
    mut canvas: RgbaImage,
    shadow: &MotionSceneShadow,
    underlay: bool,
) -> Result<RgbaImage, EditorError> {
    let (width, height) = (canvas.width(), canvas.height());
    if let Some(mut surface) = rgba_image_to_surface(&canvas) {
        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            paint_motion_scene_shadow(
                &context,
                MotionStage::frame(f64::from(width), f64::from(height)),
                shadow,
                underlay,
            );
        }
        surface.flush();
        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        canvas = cairo_argb_to_rgba_image(width, height, stride as usize, surface_data.as_ref());
    }
    Ok(canvas)
}

/// Outside border around the image plus accent strokes. The stroke is
/// centered on an expanded path so the inner edge aligns with the image
/// edge and the outer edge carries the radius outward.
fn stroke_frame_border(
    context: &gtk4::cairo::Context,
    image_rect: &FloatRect,
    unit: f64,
    base_radius: f64,
    border_thickness: f64,
    border_color: DrawColor,
    style: FrameStyle,
) {
    let spec = style.spec();
    if spec.liquid {
        // Liquid Glass is rendered as a refracted layer over the composited
        // canvas instead of a stroke (see `paint_liquid_glass`).
        return;
    }
    // Inset borders paint fully inside the image edge; the surround (and any
    // accent rings) still starts at the image edge itself.
    if spec.inset_border && border_thickness > 0.0 {
        let lw = (border_thickness * unit).max(0.0);
        if lw > 0.01 {
            let e = lw / 2.0;
            context.set_source_rgba(
                border_color.r,
                border_color.g,
                border_color.b,
                border_color.a,
            );
            context.set_line_width(lw);
            draw_rounded_rect_path(
                context,
                image_rect.x + e,
                image_rect.y + e,
                (image_rect.width - e * 2.0).max(1.0),
                (image_rect.height - e * 2.0).max(1.0),
                (base_radius - e).max(0.0),
            );
            let _ = context.stroke();
        }
    }
    let mut expand = 0.0;
    let stroke_outside = |context: &gtk4::cairo::Context,
                          thickness: f64,
                          r: f64,
                          g: f64,
                          b: f64,
                          a: f64,
                          extra: f64| {
        let lw = (thickness * unit).max(0.0);
        if lw <= 0.01 {
            return extra;
        }
        let e = extra + lw / 2.0;
        context.set_source_rgba(r, g, b, a);
        context.set_line_width(lw);
        // A zero radius stays a sharp mitered frame: the expanded path must
        // not inherit half the line width as corner rounding.
        let path_radius = if base_radius <= 0.0 {
            0.0
        } else {
            base_radius + e
        };
        draw_rounded_rect_path(
            context,
            image_rect.x - e,
            image_rect.y - e,
            image_rect.width + e * 2.0,
            image_rect.height + e * 2.0,
            path_radius,
        );
        let _ = context.stroke();
        extra + lw
    };
    if border_thickness > 0.0 && !spec.inset_border {
        expand = stroke_outside(
            context,
            border_thickness,
            border_color.r,
            border_color.g,
            border_color.b,
            border_color.a,
            expand,
        );
    }
    for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
        expand += outer.gap * unit;
        expand = stroke_outside(
            context,
            outer.thickness,
            outer.color.r,
            outer.color.g,
            outer.color.b,
            outer.color.a,
            expand,
        );
    }
}

impl EditorState {
    pub fn to_rendered_image(&self) -> Result<RgbaImage, EditorError> {
        let (width, height) = self.working_image.dimensions();
        if width == 0 || height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }
        if crate::capture::editor::types::frame_needs_canvas(
            self.frame_style,
            self.border_thickness,
        ) {
            return self.to_framed_image_without_background();
        }

        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        let data = super::super::render::rgba_to_cairo_argb_bytes(&self.working_image);
        let mut surface = gtk4::cairo::ImageSurface::create_for_data(
            data,
            gtk4::cairo::Format::ARgb32,
            width as i32,
            height as i32,
            stride,
        )
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;

            if self.border_thickness > 0.0 {
                let thickness = self.border_thickness.max(0.0);
                if thickness > 0.01 {
                    let inset = thickness / 2.0;
                    context.set_source_rgba(
                        self.border_color.r,
                        self.border_color.g,
                        self.border_color.b,
                        self.border_color.a,
                    );
                    context.set_line_width(thickness);
                    context.rectangle(
                        inset,
                        inset,
                        (width as f64 - thickness).max(1.0),
                        (height as f64 - thickness).max(1.0),
                    );
                    let _ = context.stroke();
                }
            }

            for action in &self.actions {
                if matches!(
                    action,
                    AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                ) {
                    continue;
                }
                super::super::render::draw_annotation_action(&context, action);
            }
        }

        surface.flush();
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        Ok(super::super::render::cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    /// Frame styles without a wallpaper: the card grows a transparent canvas
    /// holding the backing window plus the outside border, all following the
    /// Border Radius. Plain screenshots (Default, no border) keep the legacy
    /// direct path above.
    fn to_framed_image_without_background(&self) -> Result<RgbaImage, EditorError> {
        let layout = self.background_layout_for(&self.working_image);
        let canvas_w = layout.canvas_width.round().max(1.0) as u32;
        let canvas_h = layout.canvas_height.round().max(1.0) as u32;
        let mut canvas = RgbaImage::from_pixel(canvas_w, canvas_h, image::Rgba([0, 0, 0, 0]));
        let radius = self.background_corner_radius * layout.scale_factor * layout.draw_scale;
        canvas = paint_frame_backings(canvas, &layout.image_rect, radius, self.frame_style)?;

        let working: &RgbaImage = self.working_image.as_ref();
        let mut final_shot = if (layout.draw_scale - 1.0).abs() > 0.001 {
            image::imageops::resize(
                working,
                layout.image_rect.width.round().max(1.0) as u32,
                layout.image_rect.height.round().max(1.0) as u32,
                image::imageops::FilterType::CatmullRom,
            )
        } else {
            working.clone()
        };
        if self.background_corner_radius > 0.0 {
            apply_corner_radius(&mut final_shot, radius);
        }
        image::imageops::overlay(
            &mut canvas,
            &final_shot,
            layout.image_rect.x.round() as i64,
            layout.image_rect.y.round() as i64,
        );

        let (width, height) = (canvas.width(), canvas.height());
        if width == 0 || height == 0 {
            return Ok(canvas);
        }
        let canvas = self.paint_liquid_glass(canvas, &layout);
        let Some(mut surface) = rgba_image_to_surface(&canvas) else {
            return Ok(canvas);
        };
        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            let unit = layout.scale_factor * layout.draw_scale;
            stroke_frame_border(
                &context,
                &layout.image_rect,
                unit,
                radius,
                self.border_thickness,
                self.border_color,
                self.frame_style,
            );
            context.translate(layout.image_rect.x, layout.image_rect.y);
            context.scale(layout.draw_scale, layout.draw_scale);
            for action in self.vector_annotation_actions() {
                super::super::render::draw_annotation_action(&context, action);
            }
        }
        surface.flush();
        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        Ok(cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    pub fn to_final_image(&self) -> Result<RgbaImage, EditorError> {
        if self.background_style != BackgroundStyle::None {
            // Annotations paint above the background/wallpaper in canvas space,
            // so text/shapes placed on the padding stay visible instead of being
            // baked into the screenshot underneath the wallpaper. Use the clean
            // working image (effects only) for the card, then overlay vectors.
            return self.render_with_background_and_annotations(&self.working_image);
        }

        self.to_rendered_image()
    }

    /// Background-free card for Motion: the screenshot (effects baked) plus
    /// vector annotations, with no wallpaper/padding, no corner radius, no
    /// frame backings/borders, and no shadow. Motion composites all of that
    /// itself from the shared `MotionAppearance`, so baking any of it here
    /// would stack a second background behind the one already there and warp
    /// it with the camera.
    pub fn to_motion_card_image(&self) -> Result<RgbaImage, EditorError> {
        let (width, height) = self.working_image.dimensions();
        if width == 0 || height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }
        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let data = super::super::render::rgba_to_cairo_argb_bytes(&self.working_image);
        let mut surface = gtk4::cairo::ImageSurface::create_for_data(
            data,
            gtk4::cairo::Format::ARgb32,
            width as i32,
            height as i32,
            stride,
        )
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            for action in self.vector_annotation_actions() {
                super::super::render::draw_annotation_action(&context, action);
            }
        }
        surface.flush();
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        Ok(super::super::render::cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    fn vector_annotation_actions(&self) -> impl Iterator<Item = &AnnotationAction> {
        self.actions.iter().filter(|action| {
            !matches!(
                action,
                AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
            )
        })
    }

    fn paint_vector_annotations_on_canvas(
        &self,
        canvas: RgbaImage,
        layout: &CompositionLayout,
    ) -> Result<RgbaImage, EditorError> {
        let (width, height) = (canvas.width(), canvas.height());
        if width == 0 || height == 0 {
            return Ok(canvas);
        }
        let canvas = self.paint_liquid_glass(canvas, layout);
        let Some(mut surface) = rgba_image_to_surface(&canvas) else {
            return Ok(canvas);
        };
        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;
            // Frame preset: outside border around the image plus accent strokes.
            {
                let unit = layout.scale_factor * layout.draw_scale;
                let base_radius =
                    self.background_corner_radius * layout.scale_factor * layout.draw_scale;
                stroke_frame_border(
                    &context,
                    &layout.image_rect,
                    unit,
                    base_radius,
                    self.border_thickness,
                    self.border_color,
                    self.frame_style,
                );
            }
            context.translate(layout.image_rect.x, layout.image_rect.y);
            context.scale(layout.draw_scale, layout.draw_scale);
            for action in self.vector_annotation_actions() {
                super::super::render::draw_annotation_action(&context, action);
            }
        }
        surface.flush();
        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        Ok(cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    fn render_with_background_and_annotations(
        &self,
        clean_screenshot: &RgbaImage,
    ) -> Result<RgbaImage, EditorError> {
        let canvas = self.render_with_background(clean_screenshot)?;
        let layout = self.background_layout_for(clean_screenshot);
        let canvas = self.paint_vector_annotations_on_canvas(canvas, &layout)?;
        // Scene Shadows overlay: above the card and the annotations. The still
        // has no watermark layer, so nothing sits on top of it here.
        paint_scene_shadow_layer(canvas, &self.scene_shadow, false)
    }

    fn background_layout_for(&self, screenshot: &RgbaImage) -> CompositionLayout {
        BackgroundComposition::new(screenshot.width() as f64, screenshot.height() as f64)
            .with_style(self.background_style.clone())
            .with_padding(self.background_padding)
            .with_shadow(self.background_shadow)
            .with_insert(self.background_insert)
            .with_alignment(self.background_alignment)
            .with_corner_radius(self.background_corner_radius)
            .with_aspect_ratio(self.background_aspect_ratio)
            .with_frame_style(self.frame_style)
            .with_frame_border_thickness(self.border_thickness)
            .compute()
    }

    /// Liquid Glass frame: bend the composited canvas through the glass band
    /// and light it. Runs before the annotation pass so strokes stay on top.
    fn paint_liquid_glass(&self, canvas: RgbaImage, layout: &CompositionLayout) -> RgbaImage {
        let spec = self.frame_style.spec();
        if !spec.liquid {
            return canvas;
        }
        let unit = layout.scale_factor * layout.draw_scale;
        let gap = (spec.border_thickness + spec.outer1.map(|outer| outer.thickness).unwrap_or(0.0))
            * unit;
        let ring = GlassRing {
            x: layout.image_rect.x,
            y: layout.image_rect.y,
            width: layout.image_rect.width,
            height: layout.image_rect.height,
            radius: self.background_corner_radius * unit,
            gap,
        };
        // The glass family shares one shader; geometry plus per-style body
        // shade carry the look. The wide frosted siblings smear instead of
        // lensing, so they get dedicated frost presets.
        let look = match self.frame_style {
            FrameStyle::GlassLight => GlassLook::frost_light(),
            FrameStyle::GlassDark => GlassLook::frost_dark(),
            _ => GlassLook::default(),
        };
        let Some(layer) = glass_layer(&canvas, &ring, &look) else {
            return canvas;
        };
        let mut canvas = canvas;
        image::imageops::overlay(&mut canvas, &layer.image, layer.x, layer.y);
        canvas
    }

    fn render_with_background(&self, screenshot: &RgbaImage) -> Result<RgbaImage, EditorError> {
        let layout = self.background_layout_for(screenshot);

        let mut canvas = match &self.background_style {
            BackgroundStyle::PlainColor(color) => {
                let pixel = image::Rgba([
                    (color.r.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.g.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.b.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.a.clamp(0.0, 1.0) * 255.0) as u8,
                ]);
                RgbaImage::from_pixel(
                    layout.canvas_width as u32,
                    layout.canvas_height as u32,
                    pixel,
                )
            }
            BackgroundStyle::Gradient(idx) => {
                let file_name = crate::capture::editor::window::background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES[*idx];
                let path = crate::capture::editor::window::background_panel::background_gradient_asset_path(file_name);
                self.load_and_resize_background(
                    &path,
                    layout.canvas_width as u32,
                    layout.canvas_height as u32,
                )?
            }
            BackgroundStyle::Wallpaper(path) => self.load_and_resize_background(
                path,
                layout.canvas_width as u32,
                layout.canvas_height as u32,
            )?,
            BackgroundStyle::Blurred(blur_idx) => {
                let base_radius = match blur_idx {
                    0 => 10.0,
                    1 => 35.0,
                    2 => 80.0,
                    _ => 20.0,
                };
                // Match the editor's on-screen preview: downsample the screenshot
                // to <=800px on its longest edge BEFORE blurring, then upscale to
                // canvas size. The on-screen draw path does this for the cached
                // preview surface, but the original save path re-blurred at full
                // resolution which dominated "Done" latency on large captures
                // (4K screenshots could spend 1-2 s just blurring before encode).
                // Blur is intrinsically smooth, so the visible result of
                // downsample -> blur -> upscale is indistinguishable from
                // full-resolution blur.
                const MAX_BLUR_DIM: u32 = 800;
                let (sw, sh) = screenshot.dimensions();
                let mut blurred = if sw > MAX_BLUR_DIM || sh > MAX_BLUR_DIM {
                    let scale = MAX_BLUR_DIM as f64 / (sw.max(sh) as f64);
                    image::imageops::resize(
                        screenshot,
                        ((sw as f64) * scale).round().max(1.0) as u32,
                        ((sh as f64) * scale).round().max(1.0) as u32,
                        image::imageops::FilterType::Triangle,
                    )
                } else {
                    screenshot.clone()
                };
                let (bw, bh) = blurred.dimensions();
                // The Appearance slider adds to the style's own blur level. Its
                // radius is measured in canvas pixels, so convert it to this
                // working image before blurring.
                let slider_radius = self.background_blur.clamp(0.0, 1.0)
                    * BACKGROUND_BLUR_MAX_RADIUS
                    * (f64::from(bw) / layout.canvas_width.max(1.0));
                apply_blur_rect(
                    &mut blurred,
                    Rect {
                        x: 0,
                        y: 0,
                        width: bw as i32,
                        height: bh as i32,
                    },
                    base_radius + slider_radius,
                    false,
                );
                image::imageops::resize(
                    &blurred,
                    layout.canvas_width as u32,
                    layout.canvas_height as u32,
                    image::imageops::FilterType::Triangle,
                )
            }
            BackgroundStyle::None => return Ok(screenshot.clone()),
        };

        // The Appearance blur softens the fill only, and it lands before the
        // grain so noise stays crisp on top of a blurred background.
        if matches!(
            self.background_style,
            BackgroundStyle::Gradient(_) | BackgroundStyle::Wallpaper(_)
        ) {
            apply_background_blur(&mut canvas, self.background_blur, 1.0);
        }

        // Background grain belongs to the fill, so it lands before the card,
        // its backings, and its shadow: the same layer order the canvas preview
        // and the Motion renderer use.
        canvas = apply_background_noise(canvas, self.background_noise);

        // Scene Shadows underlay: on the fill, under the card backings, the
        // drop shadow, and the card itself.
        canvas = paint_scene_shadow_layer(canvas, &self.scene_shadow, true)?;

        // Backing sheets (Stack looks, Retro window) behind the card.
        {
            let radius = self.background_corner_radius * layout.scale_factor * layout.draw_scale;
            canvas = paint_frame_backings(canvas, &layout.image_rect, radius, self.frame_style)?;
        }

        let mut final_screenshot = if (layout.draw_scale - 1.0).abs() > 0.001 {
            image::imageops::resize(
                screenshot,
                layout.image_rect.width.round().max(1.0) as u32,
                layout.image_rect.height.round().max(1.0) as u32,
                image::imageops::FilterType::CatmullRom,
            )
        } else {
            screenshot.clone()
        };

        if self.background_corner_radius > 0.0 {
            let radius = self.background_corner_radius * layout.scale_factor * layout.draw_scale;
            apply_corner_radius(&mut final_screenshot, radius);
        }

        if let Some(shadow) = layout.shadow {
            let mut shadow_layer = render_shadow_layer(
                final_screenshot.width(),
                final_screenshot.height(),
                shadow.blur,
                shadow.opacity,
                self.background_corner_radius * layout.scale_factor * layout.draw_scale,
            )?;
            if shadow.blur > 0.0 {
                let shadow_width = shadow_layer.width() as i32;
                let shadow_height = shadow_layer.height() as i32;
                let blur_rect = Rect {
                    x: 0,
                    y: 0,
                    width: shadow_width,
                    height: shadow_height,
                };
                // Apply 3 passes of box blur to approximate Gaussian blur.
                // A single pass produces harsh edges; multiple passes create
                // the smooth falloff expected of a realistic shadow.
                let pass_radius = (shadow.blur / 2.0).max(1.0);
                for _ in 0..3 {
                    apply_blur_rect(&mut shadow_layer, blur_rect, pass_radius, true);
                }
            }
            image::imageops::overlay(
                &mut canvas,
                &shadow_layer,
                shadow.rect.x.round() as i64,
                shadow.rect.y.round() as i64,
            );
        }

        image::imageops::overlay(
            &mut canvas,
            &final_screenshot,
            layout.image_rect.x.round() as i64,
            layout.image_rect.y.round() as i64,
        );

        Ok(canvas)
    }

    fn load_and_resize_background(
        &self,
        path: &Path,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, EditorError> {
        let img = image::open(path).map_err(|e| EditorError::ImageLoad(e.to_string()))?;
        let rgba = img.into_rgba8();
        Ok(image::imageops::resize(
            &rgba,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ))
    }

    pub fn set_highlighter_mode(&mut self, mode: HighlighterMode) {
        self.highlighter_mode = mode;
    }

    pub fn set_pen_weight(&mut self, weight: PenWeight) {
        self.pen_weight = weight;
    }
}

pub(super) fn render_shadow_layer(
    width: u32,
    height: u32,
    blur: f64,
    opacity: f64,
    corner_radius: f64,
) -> Result<RgbaImage, EditorError> {
    let spread_px = (blur * 1.35).ceil().max(0.0) as i32;
    let shadow_width = width as i32 + spread_px * 2;
    let shadow_height = height as i32 + spread_px * 2;
    let stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(shadow_width as u32)
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;
    let mut surface =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, shadow_width, shadow_height)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
    {
        let context = gtk4::cairo::Context::new(&surface)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        context.set_source_rgba(0.0, 0.0, 0.0, opacity.clamp(0.0, 1.0));
        draw_rounded_rect_path(
            &context,
            spread_px as f64,
            spread_px as f64,
            width as f64,
            height as f64,
            corner_radius,
        );
        let _ = context.fill();
    }
    surface.flush();
    let data = surface
        .data()
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;
    Ok(cairo_argb_to_rgba_image(
        shadow_width as u32,
        shadow_height as u32,
        stride as usize,
        data.as_ref(),
    ))
}

fn draw_rounded_rect_path(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    // One smooth-corner implementation for preview + export; the clip, the
    // shadow, the borders and the backings must all trace the same outline.
    super::super::render::rounded_rect_path(context, x, y, width, height, radius);
}

fn apply_corner_radius(image: &mut RgbaImage, radius: f64) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || radius <= 0.0 {
        return;
    }
    let radius = radius.min(width as f64 / 2.0).min(height as f64 / 2.0);
    if radius <= 0.0 {
        return;
    }
    let Some(source_surface) = rgba_image_to_surface(image) else {
        return;
    };
    let stride = match gtk4::cairo::Format::ARgb32.stride_for_width(width) {
        Ok(stride) => stride,
        Err(_) => return,
    };
    let mut clipped_surface = match gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
    ) {
        Ok(surface) => surface,
        Err(_) => return,
    };
    {
        let context = match gtk4::cairo::Context::new(&clipped_surface) {
            Ok(context) => context,
            Err(_) => return,
        };
        context.set_antialias(gtk4::cairo::Antialias::Best);
        draw_rounded_rect_path(&context, 0.0, 0.0, width as f64, height as f64, radius);
        context.clip();
        if context
            .set_source_surface(&source_surface, 0.0, 0.0)
            .is_err()
        {
            return;
        }
        let _ = context.paint();
    }
    clipped_surface.flush();
    let surface_data = match clipped_surface.data() {
        Ok(data) => data,
        Err(_) => return,
    };
    *image = cairo_argb_to_rgba_image(width, height, stride as usize, surface_data.as_ref());
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::apply_corner_radius;

    #[test]
    fn corner_radius_antialiases_top_right_edge() {
        let mut image = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));

        apply_corner_radius(&mut image, 12.0);

        let top_right_band_has_partial_alpha = (28..40).any(|x| {
            (0..12).any(|y| {
                let alpha = image.get_pixel(x, y)[3];
                alpha > 0 && alpha < 255
            })
        });

        assert!(
            top_right_band_has_partial_alpha,
            "expected antialiased pixels along the top-right rounded edge"
        );
        assert_eq!(image.get_pixel(39, 0)[3], 0);
        assert_eq!(image.get_pixel(20, 20)[3], 255);
    }
}

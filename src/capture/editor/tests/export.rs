use super::*;
use crate::capture::editor::render::{
    cairo_argb_to_rgba_image, clear_lens, frame_has_attached_border, glass_layer,
    liquid_preview_backdrop, liquid_preview_layer, rounded_rect_path,
    squircle_rounded_rect_contains, GlassLook, GlassRing, LiquidPreview,
};

#[test]
fn final_image_applies_focus_overlay() {
    let mut image = RgbaImage::new(12, 12);
    for y in 0..12 {
        for x in 0..12 {
            image.put_pixel(x, y, image::Rgba([180, 170, 160, 255]));
        }
    }

    let mut state = EditorState::new(image);
    state.actions.push(AnnotationAction::Focus {
        rect: Rect {
            x: 3,
            y: 3,
            width: 6,
            height: 6,
        },
        intensity: 58.0,
    });
    state.rebuild_effect_layer();

    let final_image = state.to_final_image().unwrap();
    let inside = *final_image.get_pixel(4, 4);
    let outside = *final_image.get_pixel(1, 1);

    assert_eq!(inside, image::Rgba([180, 170, 160, 255]));
    assert!(outside[0] < 180);
    assert!(outside[1] < 170);
    assert!(outside[2] < 160);
}

#[test]
fn final_image_shadow_does_not_replace_background_with_black_mask() {
    let image = RgbaImage::from_pixel(400, 240, image::Rgba([220, 220, 220, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_shadow = 45.0;

    let final_image = state.to_final_image().expect("final image");
    let corner = *final_image.get_pixel(0, 0);

    assert_ne!(corner, image::Rgba([0, 0, 0, 255]));
    assert_eq!(corner, image::Rgba([255, 255, 255, 255]));
}

#[test]
fn final_image_background_keeps_screenshot_at_native_scale_by_default() {
    let mut image = RgbaImage::new(400, 300);
    image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    image.put_pixel(399, 299, image::Rgba([0, 0, 255, 255]));

    let mut state = EditorState::new(image.clone());
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    let final_image = state.to_final_image().expect("final image");

    // A fill floors the stored 0px padding to the shared breathing room, so
    // the canvas grows by that gap on each side while the screenshot itself
    // stays 1:1 inside it.
    let gap = crate::capture::editor::types::BACKGROUND_MIN_GAP.round() as u32;
    assert_eq!(final_image.dimensions(), (400 + gap * 2, 300 + gap * 2));
    assert_eq!(*final_image.get_pixel(gap, gap), *image.get_pixel(0, 0));
    assert_eq!(
        *final_image.get_pixel(gap + 399, gap + 299),
        *image.get_pixel(399, 299)
    );
}

#[test]
fn final_image_background_shadow_visibly_darkens_pixels_below_card() {
    let image = RgbaImage::from_pixel(400, 240, image::Rgba([220, 220, 220, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_insert = 0.0;
    state.background_corner_radius = 18.0;
    // The shared Appearance panel drives the still's shadow now; a raised
    // opacity with a downward offset is what the panel produces.
    state.shadow_opacity = 0.45;
    state.shadow_blur = 40.0;
    state.shadow_offset_x = 0.0;
    state.shadow_offset_y = 24.0;

    let final_image = state.to_final_image().expect("final image");
    let shadow_pixel = *final_image.get_pixel(final_image.width() / 2, 290);

    assert!(
        shadow_pixel[0] < 235,
        "expected visible shadow below the card, got pixel {:?}",
        shadow_pixel
    );
}

#[test]
fn final_image_draws_annotations_on_top_of_background_padding() {
    // Screenshot 100x100 mid-gray; white background padding; red pen stroke
    // placed on the top-left wallpaper (negative screenshot coords).
    // Before the fix the stroke was baked into the screenshot and clipped away,
    // so the wallpaper pixel stayed white (annotation "underneath" the wallpaper).
    let image = RgbaImage::from_pixel(100, 100, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    state.actions.push(AnnotationAction::Pen {
        points: vec![Point { x: -8.0, y: -8.0 }, Point { x: -2.0, y: -8.0 }],
        color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
        stroke_size: 4.0,
    });

    let final_image = state.to_final_image().expect("final image");
    // Canvas is 120x120; screenshot at (10, 10); stroke maps to canvas y=2, x=2..8.
    let pixel = *final_image.get_pixel(5, 2);
    assert!(
        pixel[0] > 200 && pixel[1] < 100 && pixel[2] < 100,
        "expected red annotation on wallpaper padding, got pixel {:?}",
        pixel
    );
}

#[test]
fn annotation_canvas_bounds_include_wallpaper_padding() {
    let image = RgbaImage::from_pixel(100, 100, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    let (min_x, min_y, max_x, max_y) = state.annotation_canvas_bounds();
    assert!(
        min_x < 0.0 && min_y < 0.0,
        "padding should map to negative coords, got ({}, {})",
        min_x,
        min_y
    );
    assert!(
        max_x > 100.0 && max_y > 100.0,
        "padding should extend beyond screenshot, got ({}, {})",
        max_x,
        max_y
    );
}

#[test]
fn number_marker_can_be_placed_on_background_padding() {
    let image = RgbaImage::from_pixel(400, 300, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    let (min_x, _, _, _) = state.annotation_canvas_bounds();
    assert!(
        min_x < 0.0,
        "expected negative canvas origin, got {}",
        min_x
    );
    // Request a point inside the left wallpaper padding; the marker must not be
    // snapped back onto the screenshot (old behavior clamped to 0..image).
    state.add_number_marker(Point {
        x: min_x + 20.0,
        y: 150.0,
    });
    match state.actions.last().expect("number action") {
        AnnotationAction::Number { position, .. } => {
            assert!(
                position.x < 0.0,
                "number should stay on wallpaper padding, got x={}",
                position.x
            );
        }
        other => panic!("expected number action, got {:?}", other),
    }
}

#[test]
fn final_image_draws_border_on_top_of_wallpaper_background() {
    let image = RgbaImage::from_pixel(100, 100, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    state.border_thickness = 6.0;
    state.border_color = DrawColor::new(1.0, 0.0, 0.0, 1.0);

    let final_image = state.to_final_image().expect("final image");
    // Canvas is 120x120 with the screenshot at (10, 10); the outside border
    // sits around the card, so sample just left of the image edge.
    let edge = *final_image.get_pixel(9, 60);
    assert!(
        edge[0] as i32 - edge[1] as i32 > 30 && edge[0] as i32 - edge[2] as i32 > 30,
        "expected red border on card edge, got pixel {:?}",
        edge
    );
}

fn retro_backing_is_solid_down_right(
    backing: &crate::capture::editor::types::FrameBacking,
) -> bool {
    backing.offset_x > 0.0
        && backing.offset_y > 0.0
        && backing.rotation_deg.abs() < f64::EPSILON
        && backing.color == DrawColor::new(0.0, 0.0, 0.0, 1.0)
}

#[test]
fn retro_draws_thin_border_with_solid_window_behind_card() {
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 0.0, 0.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Retro.spec();
    state.frame_style = FrameStyle::Retro;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    let (w, h) = (final_image.width(), final_image.height());
    let x = w / 2;
    let is_gray = |px: &image::Rgba<u8>| px[0] == 100 && px[1] == 100 && px[2] == 100;
    let is_black = |px: &image::Rgba<u8>| px[0] == 0 && px[1] == 0 && px[2] == 0;
    let is_dark = |px: &image::Rgba<u8>| px[0].max(px[1]).max(px[2]) < 160;
    let is_red = |px: &image::Rgba<u8>| px[0] == 255 && px[1] == 0 && px[2] == 0;
    let is_reddish = |px: &image::Rgba<u8>| px[0] > 150 && px[1] < 150 && px[2] < 150;
    let first_gray = (0..h)
        .find(|y| is_gray(final_image.get_pixel(x, *y)))
        .expect("card top");
    let last_gray = (0..h)
        .rev()
        .find(|y| is_gray(final_image.get_pixel(x, *y)))
        .expect("card bottom");
    // Thin border directly around the card (fractional layout anti-aliases
    // the edge, so accept any dark pixel here).
    assert!(
        is_dark(final_image.get_pixel(x, first_gray - 1)),
        "expected thin top border above card, got {:?}",
        final_image.get_pixel(x, first_gray - 1)
    );
    assert!(
        is_dark(final_image.get_pixel(x, last_gray + 1)),
        "expected thin bottom border below card, got {:?}",
        final_image.get_pixel(x, last_gray + 1)
    );
    // Offset window peeks well below the card but not above it: the hard
    // shadow only extends down-right.
    assert!(
        is_black(final_image.get_pixel(x, last_gray + 10)),
        "expected retro window peeking below card"
    );
    assert!(
        is_reddish(final_image.get_pixel(x, first_gray - 6)),
        "retro window must not peek above card, got {:?}",
        final_image.get_pixel(x, first_gray - 6)
    );
    // Spot-check the backdrop itself.
    assert!(is_red(final_image.get_pixel(2, 2)));
}

#[test]
fn retro_window_renders_without_background_and_follows_radius() {
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::None;
    state.background_corner_radius = 24.0;
    let spec = FrameStyle::Retro.spec();
    state.frame_style = FrameStyle::Retro;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Canvas grows to hold the thin border plus the offset window.
    assert!(
        final_image.width() > 400 && final_image.height() > 400,
        "expected grown canvas, got {}x{}",
        final_image.width(),
        final_image.height()
    );
    let (w, h) = (final_image.width(), final_image.height());
    let is_black = |px: &image::Rgba<u8>| px[0] == 0 && px[1] == 0 && px[2] == 0;
    // The window peeks along the right/bottom edges but not top-left.
    assert!(
        is_black(final_image.get_pixel(w - 2, h / 2)),
        "expected retro window along right edge, got {:?}",
        final_image.get_pixel(w - 2, h / 2)
    );
    assert!(
        final_image.get_pixel(2, 2)[3] < 128,
        "retro window must not peek top-left, got {:?}",
        final_image.get_pixel(2, 2)
    );
    // Border radius rounds the card with a smooth continuous corner: the
    // extreme corner is still cut away, but the fuller diagonal — which a
    // circular arc would already cut — stays covered by the Retro frame
    // tracing the same smooth outline.
    let extreme = *final_image.get_pixel(1, 1);
    assert!(
        extreme[3] < 128,
        "expected smooth card corner to cut the extreme corner, got {:?}",
        extreme
    );
    let diagonal = *final_image.get_pixel(5, 5);
    assert!(
        diagonal[3] > 200,
        "expected smooth corner diagonal to stay covered, got {:?}",
        diagonal
    );
    let inside = *final_image.get_pixel(30, 30);
    assert_eq!(
        inside,
        image::Rgba([100, 100, 100, 255]),
        "expected image inside the rounded corner, got {:?}",
        inside
    );
}

#[test]
fn retro_border_stays_sharp_when_radius_is_zero() {
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::None;
    state.background_corner_radius = 0.0;
    let spec = FrameStyle::Retro.spec();
    state.frame_style = FrameStyle::Retro;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Mitered frame: the extreme outer corner is border, not a rounded cutout.
    let corner = *final_image.get_pixel(1, 1);
    assert_eq!(
        corner,
        image::Rgba([0, 0, 0, 255]),
        "expected sharp black frame corner at radius 0, got {:?}",
        corner
    );
    // Card itself stays square too.
    assert_eq!(
        *final_image.get_pixel(30, 30),
        image::Rgba([100, 100, 100, 255])
    );
}

#[test]
fn inset_border_paints_inside_the_card_edge() {
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 0.0, 0.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    for style in [FrameStyle::InsetLight, FrameStyle::InsetDark] {
        let spec = style.spec();
        assert!((spec.border_thickness - 3.0).abs() < f64::EPSILON);
        assert!(spec.inset_border);
        state.frame_style = style;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;

        let final_image = state.to_final_image().expect("final image");
        // No outside frame: canvas stays image + padding, edge pixel is backdrop.
        assert_eq!(final_image.width(), 560);
        assert_eq!(
            *final_image.get_pixel(280, 79),
            image::Rgba([255, 0, 0, 255]),
            "inset must not paint outside the card for {:?}",
            style
        );
        // Just inside the edge the inset line shows.
        let edge = *final_image.get_pixel(280, 81);
        // Deep inside stays image.
        let inner = *final_image.get_pixel(280, 90);
        assert_eq!(inner, image::Rgba([100, 100, 100, 255]));
        match style {
            FrameStyle::InsetLight => assert!(
                edge[0] > 200 && edge[1] > 200 && edge[2] > 200,
                "expected light inset line, got {:?}",
                edge
            ),
            _ => assert!(
                edge[0] < 60 && edge[1] < 60 && edge[2] < 60,
                "expected dark inset line, got {:?}",
                edge
            ),
        }
    }
}

#[test]
fn attached_outside_borders_have_no_background_seam_at_rounded_corners() {
    for style in [FrameStyle::Border, FrameStyle::Retro] {
        let spec = style.spec();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            800,
            800,
            image::Rgba([220, 0, 0, 255]),
        ));
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 1.0, 0.0, 1.0));
        state.background_padding = 50.0;
        state.background_shadow = 0.0;
        state.background_corner_radius = 20.0;
        state.frame_style = style;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;
        let output = state.to_final_image().expect("rounded border image");
        let safe_expand = state.border_thickness * 2.0 - 1.0;
        let mut checked = 0;
        for y in 90..145 {
            for x in 90..145 {
                if !squircle_rounded_rect_contains(
                    f64::from(x) + 0.5 - 100.0 + safe_expand,
                    f64::from(y) + 0.5 - 100.0 + safe_expand,
                    800.0 + safe_expand * 2.0,
                    800.0 + safe_expand * 2.0,
                    40.0 + safe_expand,
                ) {
                    continue;
                }
                checked += 1;
                let pixel = output.get_pixel(x, y);
                assert!(
                    pixel[1] <= 12,
                    "{style:?} exposes the green background at ({x},{y}): {pixel:?}"
                );
            }
        }
        assert!(checked > 500);
    }
}

#[test]
fn attached_inset_borders_cover_the_images_rounded_corner_edge() {
    let mut mask =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 800, 800).expect("mask");
    {
        let context = gtk4::cairo::Context::new(&mask).expect("mask context");
        rounded_rect_path(&context, 0.0, 0.0, 800.0, 800.0, 40.0);
        context.set_source_rgb(1.0, 1.0, 1.0);
        context.fill().expect("mask fill");
    }
    mask.flush();
    let stride = mask.stride() as usize;
    let mask = cairo_argb_to_rgba_image(800, 800, stride, &mask.data().expect("mask bytes"));
    for style in [FrameStyle::InsetLight, FrameStyle::InsetDark] {
        let spec = style.spec();
        let mut state = EditorState::new(RgbaImage::from_pixel(
            800,
            800,
            image::Rgba([220, 0, 0, 255]),
        ));
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 1.0, 0.0, 1.0));
        state.background_padding = 50.0;
        state.background_shadow = 0.0;
        state.background_corner_radius = 20.0;
        state.frame_style = style;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;
        let output = state.to_final_image().expect("rounded inset image");
        let inset = state.border_thickness * 2.0 - 1.0;
        let color = spec.border_color;
        let expected = [
            220.0 * (1.0 - color.a) + color.r * 255.0 * color.a,
            color.g * 255.0 * color.a,
            color.b * 255.0 * color.a,
        ];
        let mut checked = 0;
        for y in 0..45 {
            for x in 0..45 {
                if mask.get_pixel(x, y)[3] < 250
                    || squircle_rounded_rect_contains(
                        f64::from(x) + 0.5 - inset,
                        f64::from(y) + 0.5 - inset,
                        800.0 - inset * 2.0,
                        800.0 - inset * 2.0,
                        40.0 - inset,
                    )
                {
                    continue;
                }
                checked += 1;
                let pixel = output.get_pixel(100 + x, 100 + y);
                for channel in 0..3 {
                    assert!((f64::from(pixel[channel]) - expected[channel]).abs() <= 12.0,
                        "{style:?} leaves image colour uncovered at ({x},{y}): {pixel:?}, expected {expected:?}");
                }
            }
        }
        assert!(checked > 100);
    }
}

#[test]
fn attached_border_fill_preserves_transparency_and_unaffected_presets() {
    let mut image = RgbaImage::from_pixel(400, 400, image::Rgba([220, 0, 0, 255]));
    image.put_pixel(200, 200, image::Rgba([0, 0, 0, 0]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::None;
    state.background_corner_radius = 20.0;
    state.frame_style = FrameStyle::Border;
    state.border_thickness = FrameStyle::Border.spec().border_thickness;
    state.border_color = FrameStyle::Border.spec().border_color;
    let output = state.to_final_image().expect("transparent bordered image");
    assert_eq!(
        output.get_pixel(output.width() / 2, output.height() / 2)[3],
        0
    );
    for style in [
        FrameStyle::Liquid,
        FrameStyle::GlassLight,
        FrameStyle::GlassDark,
        FrameStyle::Outline,
        FrameStyle::Card,
        FrameStyle::Stack,
        FrameStyle::Stack2,
    ] {
        assert!(
            !frame_has_attached_border(style),
            "{style:?} must retain its existing treatment"
        );
    }
}

#[test]
fn frame_style_presets_resolve_to_distinct_outside_borders() {
    assert_eq!(FrameStyle::ALL.len(), 12);
    assert_eq!(FrameStyle::Default.spec().border_thickness, 0.0);
    for style in [FrameStyle::Border, FrameStyle::Retro] {
        assert!(
            (style.spec().border_thickness - 3.0).abs() < f64::EPSILON,
            "{:?} should share the thin 3px frame",
            style
        );
    }
    // Outline is a floating hairline: no color frame on the card, a gray
    // ring held off the image by a background gap.
    let outline = FrameStyle::Outline.spec();
    assert!((outline.border_thickness - 0.0).abs() < f64::EPSILON);
    let ring = outline.outer1.expect("outline floating ring");
    assert!((ring.thickness - 1.0).abs() < f64::EPSILON);
    assert!((ring.gap - 2.0).abs() < f64::EPSILON);
    assert_eq!(ring.color, DrawColor::new(0.7, 0.7, 0.7, 1.0));
    assert!(outline.outer2.is_none());
    assert!(outline.backing1.is_none() && outline.backing2.is_none());
    assert!(FrameStyle::Card.spec().backing1.is_some());
    assert!(FrameStyle::Card.spec().backing2.is_none());
    assert!(FrameStyle::Stack.spec().backing1.is_some());
    assert!(FrameStyle::Stack.spec().backing2.is_none());
    assert!(FrameStyle::Stack2.spec().backing1.is_some());
    assert!(FrameStyle::Stack2.spec().backing2.is_some());
    let retro = FrameStyle::Retro.spec();
    assert!(retro.outer1.is_none());
    assert!(retro.outer2.is_none());
    assert!((retro.border_thickness - 3.0).abs() < f64::EPSILON);
    let retro_back = retro.backing1.expect("retro hard shadow behind card");
    assert!(retro_backing_is_solid_down_right(&retro_back));
    assert!(retro.backing2.is_none());
    assert_eq!(FrameStyle::Default.label(), "Default");
    assert_eq!(FrameStyle::Stack2.label(), "Stack 2");
}

#[test]
fn outline_floats_a_hairline_off_the_card_with_a_gap() {
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 0.0, 0.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Outline.spec();
    state.frame_style = FrameStyle::Outline;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Card lands at (80, 80): 1px ring covering y=77, background gap at y=79.
    let ring = *final_image.get_pixel(280, 77);
    assert!(
        (ring[0] as i32 - 178).abs() < 4
            && (ring[0] as i32 - ring[1] as i32).abs() < 4
            && (ring[0] as i32 - ring[2] as i32).abs() < 4,
        "expected gray hairline ring, got {:?}",
        ring
    );
    assert_eq!(
        *final_image.get_pixel(280, 79),
        image::Rgba([255, 0, 0, 255]),
        "expected background gap between ring and card"
    );
    assert_eq!(
        *final_image.get_pixel(280, 81),
        image::Rgba([100, 100, 100, 255])
    );
}

#[test]
fn card_preset_draws_a_single_backing_sheet_behind_the_card() {
    let image = RgbaImage::from_pixel(100, 100, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Card.spec();
    state.frame_style = FrameStyle::Card;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Padding 80 at scale 0.25 => 20px base; the framed stack is centered,
    // so the card sits at (23, 28) with its backing sheet at (17, 12).
    let sheet = *final_image.get_pixel(20, 60);
    assert!(
        sheet[0] > 170
            && sheet[0] < 230
            && (sheet[0] as i32 - sheet[1] as i32).abs() < 12
            && (sheet[0] as i32 - sheet[2] as i32).abs() < 12,
        "expected gray backing sheet behind card, got pixel {:?}",
        sheet
    );
    // Main image pixels stay intact.
    assert_eq!(
        *final_image.get_pixel(70, 70),
        image::Rgba([100, 100, 100, 255])
    );
}

#[test]
fn stack_preset_fans_diagonally_top_right_and_bottom_left() {
    let image = RgbaImage::from_pixel(100, 100, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Stack.spec();
    state.frame_style = FrameStyle::Stack;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Card stays centered at (20, 20); the center-pivot sheet peeks above
    // the top edge on the right half and below the bottom edge on the left.
    let top = *final_image.get_pixel(100, 19);
    assert!(
        top[0] > 150
            && top[0] < 235
            && (top[0] as i32 - top[1] as i32).abs() < 14
            && (top[0] as i32 - top[2] as i32).abs() < 14,
        "expected diagonal sheet peeking above top-right, got pixel {:?}",
        top
    );
    let bottom = *final_image.get_pixel(40, 120);
    assert!(
        bottom[0] > 150
            && bottom[0] < 235
            && (bottom[0] as i32 - bottom[1] as i32).abs() < 14
            && (bottom[0] as i32 - bottom[2] as i32).abs() < 14,
        "expected diagonal sheet peeking below bottom-left, got pixel {:?}",
        bottom
    );
    // Top-left and bottom-right stay clean background.
    assert_eq!(
        *final_image.get_pixel(10, 10),
        image::Rgba([255, 255, 255, 255])
    );
    assert_eq!(
        *final_image.get_pixel(70, 70),
        image::Rgba([100, 100, 100, 255])
    );
}

#[test]
fn stack2_preset_draws_two_backing_sheets_like_stacked_prints() {
    // Mirrors the reference: red backdrop, gray sheets peeking top-left.
    let image = RgbaImage::from_pixel(200, 120, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 0.0, 0.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Stack2.spec();
    state.frame_style = FrameStyle::Stack2;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let final_image = state.to_final_image().expect("final image");
    // Padding 80 at scale 0.5 => 40px base; the framed stack is centered,
    // so the card sits at (48, 54) with fanned sheets behind it. Sample the
    // far sheet left of the near sheet's left edge.
    let far = *final_image.get_pixel(37, 60);
    assert!(
        far[0] > 115
            && far[0] < 175
            && (far[0] as i32 - far[1] as i32).abs() < 12
            && (far[0] as i32 - far[2] as i32).abs() < 12,
        "expected far gray backing sheet, got pixel {:?}",
        far
    );
    // Near sheet peeks between far sheet and card.
    let near = *final_image.get_pixel(44, 50);
    assert!(
        near[0] > 175 && near[0] < 225 && (near[0] as i32 - near[1] as i32).abs() < 12,
        "expected near gray backing sheet, got pixel {:?}",
        near
    );
    // Card itself stays intact and borderless.
    assert_eq!(
        *final_image.get_pixel(140, 100),
        image::Rgba([100, 100, 100, 255])
    );
}

#[test]
fn liquid_glass_lights_the_band_from_the_top_and_stays_translucent() {
    // A red backdrop proves both halves of the look at once: the glass stays
    // clear (red reads through it) while the light pools along the top edge
    // (band and rim brighter at the top than at the bottom).
    let image = RgbaImage::from_pixel(400, 400, image::Rgba([100, 100, 100, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 0.0, 0.0, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;
    state.background_insert = 0.0;
    let spec = FrameStyle::Liquid.spec();
    assert!(spec.liquid, "Liquid must opt into the glass renderer");
    assert!(!spec.inset_border);
    let rim = spec.outer1.expect("liquid specular rim");
    state.frame_style = FrameStyle::Liquid;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;

    let frame = state.to_final_image().expect("final image");
    let luma = |px: &image::Rgba<u8>| {
        0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2])
    };
    let gap = (spec.border_thickness + rim.thickness).round() as u32;
    let card_top = 80u32;
    let card_bottom = 480u32;
    let mid = gap.div_ceil(2);
    let top_band = *frame.get_pixel(280, card_top - mid);
    let bottom_band = *frame.get_pixel(280, card_bottom + mid - 1);
    let brightest = |range: std::ops::Range<u32>| {
        range
            .map(|y| frame.get_pixel(280, y)[1])
            .max()
            .expect("non-empty range")
    };
    // Red saturates the red channel, so the highlight is measured on green:
    // full-strength white over red leaves ~229, the faded bottom ~127.
    let top_rim = brightest(card_top - gap - 1..card_top - gap + 2);
    let bottom_rim = brightest(card_bottom + gap - 2..card_bottom + gap + 1);

    assert!(
        luma(&top_band) > luma(&bottom_band) + 6.0,
        "top of the band should be lit brighter than the bottom: {top_band:?} vs {bottom_band:?}"
    );
    assert!(
        i32::from(top_rim) > i32::from(bottom_rim) + 40,
        "specular rim should fade from the top of the card to the bottom: \
         {top_rim:?} vs {bottom_rim:?}"
    );
    // Clear glass: the red backdrop still dominates the band, so it must not
    // read as an opaque white frame (the flat look this preset replaced).
    assert!(
        bottom_band[0] > 200 && bottom_band[1] < 90 && bottom_band[2] < 90,
        "band should stay translucent over the backdrop, got {bottom_band:?}"
    );
    assert!(
        (rim.thickness - 1.0).abs() < f64::EPSILON
            && (spec.border_thickness - 2.5).abs() < f64::EPSILON,
        "liquid glass should have a 3.5px edge: 2.5px body plus 1px rim"
    );
}

#[test]
fn liquid_glass_refraction_bends_a_patterned_backdrop() {
    let mut backdrop = RgbaImage::from_pixel(240, 240, image::Rgba([0, 0, 0, 255]));
    for (x, y, pixel) in backdrop.enumerate_pixels_mut() {
        if (x + y) % 2 == 0 {
            *pixel = image::Rgba([255, 255, 255, 255]);
        }
    }
    let ring = GlassRing {
        x: 80.0,
        y: 80.0,
        width: 80.0,
        height: 80.0,
        radius: 0.0,
        gap: 7.0,
    };
    let look = GlassLook::default();
    let bent = glass_layer(&backdrop, &ring, &look).expect("refracted layer");
    let flat = glass_layer(
        &backdrop,
        &ring,
        &GlassLook {
            refraction: 0.0,
            ..look
        },
    )
    .expect("unrefracted layer");
    assert_eq!(
        (bent.x, bent.y),
        (64, 64),
        "the band should start at the card edge minus the glass reach"
    );
    let mut lit = 0usize;
    let mut displaced = 0usize;
    for (x, y, pixel) in bent.image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        let other_x = x as i64 + bent.x - flat.x;
        let other_y = y as i64 + bent.y - flat.y;
        if other_x < 0
            || other_y < 0
            || other_x as u32 >= flat.image.width()
            || other_y as u32 >= flat.image.height()
        {
            continue;
        }
        lit += 1;
        if *pixel != *flat.image.get_pixel(other_x as u32, other_y as u32) {
            displaced += 1;
        }
    }
    assert!(lit > 100, "the band should cover real pixels, got {lit}");
    assert!(
        displaced > 20,
        "the refracted band should pull the patterned backdrop through the glass, \
         only {displaced} of {lit} band pixels moved"
    );
}

#[test]
fn liquid_glass_keeps_the_backdrop_colour_dominant() {
    let backdrop = RgbaImage::from_pixel(240, 240, image::Rgba([255, 0, 0, 255]));
    let ring = GlassRing {
        x: 80.0,
        y: 80.0,
        width: 80.0,
        height: 80.0,
        radius: 0.0,
        gap: 7.0,
    };
    let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("layer");
    let mid_x = (120.0 - layer.x as f64) as u32;
    let mid_y = (76.0 - layer.y as f64) as u32;
    let mid = *layer.image.get_pixel(mid_x, mid_y);
    assert!(
        mid[0] > 200 && mid[1] < 60 && mid[2] < 60,
        "the flat body should stay the backdrop's colour, got {mid:?}"
    );
    let mut lit = 0usize;
    let mut coloured = 0usize;
    for (_, _, pixel) in layer.image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        lit += 1;
        if pixel[0] > 200 && pixel[1] < 90 && pixel[2] < 90 {
            coloured += 1;
        }
    }
    assert!(lit > 100, "the band should cover real pixels, got {lit}");
    assert!(
        coloured * 2 > lit,
        "the backdrop colour should dominate more than half the band: {coloured}/{lit}"
    );
}

#[test]
fn liquid_glass_layer_only_covers_the_band() {
    let backdrop = RgbaImage::from_pixel(240, 240, image::Rgba([30, 60, 120, 255]));
    let ring = GlassRing {
        x: 80.0,
        y: 80.0,
        width: 80.0,
        height: 80.0,
        radius: 0.0,
        gap: 7.0,
    };
    let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("layer");
    let center = *layer.image.get_pixel(
        (120.0 - layer.x as f64) as u32,
        (120.0 - layer.y as f64) as u32,
    );
    assert_eq!(
        center[3], 0,
        "the card centre must stay untouched by the layer, got {center:?}"
    );
    for (x, y, pixel) in layer.image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        let ax = layer.x as f64 + f64::from(x) + 0.5;
        let ay = layer.y as f64 + f64::from(y) + 0.5;
        let dx = (80.0 - ax).max(ax - 160.0).max(0.0);
        let dy = (80.0 - ay).max(ay - 160.0).max(0.0);
        let distance = (dx * dx + dy * dy).sqrt();
        assert!(
            distance <= 9.0,
            "layer paints {distance}px outside the 7px band at ({ax}, {ay})"
        );
        assert!(
            !(ax > 82.0 && ax < 158.0 && ay > 82.0 && ay < 158.0),
            "layer reached the card interior at ({ax}, {ay})"
        );
    }
}

#[test]
fn liquid_glass_transparent_backdrop_antialiasing_stays_dim() {
    let backdrop = RgbaImage::from_pixel(240, 240, image::Rgba([24, 24, 72, 90]));
    let ring = GlassRing {
        x: 80.0,
        y: 80.0,
        width: 80.0,
        height: 80.0,
        radius: 0.0,
        gap: 7.0,
    };
    let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("layer");
    let mut feathered = 0usize;
    for (_, _, pixel) in layer.image.enumerate_pixels() {
        if pixel[3] == 0 || pixel[3] >= 250 {
            continue;
        }
        feathered += 1;
        assert!(
            pixel[0] <= 240 && pixel[1] <= 240 && pixel[2] <= 240,
            "the feathered lip must not blow out to white, got {pixel:?}"
        );
    }
    assert!(
        feathered > 50,
        "the band should carry a feathered edge to check, got {feathered} pixels"
    );
}

#[test]
fn liquid_preview_layer_replays_the_recorded_scene_for_the_shader() {
    let width = 200i32;
    let height = 180i32;
    let paint_scene = |context: &gtk4::cairo::Context| {
        context.set_operator(gtk4::cairo::Operator::Source);
        context.set_source_rgb(0.25, 0.55, 0.85);
        context.paint().expect("background");
        context.set_operator(gtk4::cairo::Operator::Over);
        context.set_source_rgb(0.9, 0.35, 0.2);
        context.rectangle(60.0, 50.0, 80.0, 70.0);
        context.fill().expect("card");
    };
    let scene = gtk4::cairo::RecordingSurface::create(
        gtk4::cairo::Content::ColorAlpha,
        Some(gtk4::cairo::Rectangle::new(
            0.0,
            0.0,
            width as f64,
            height as f64,
        )),
    )
    .expect("scene surface");
    let group = {
        let context = gtk4::cairo::Context::new(&scene).expect("scene context");
        context.push_group();
        paint_scene(&context);
        context.pop_group().expect("recorded scene")
    };
    let mut direct = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, width, height)
        .expect("direct surface");
    {
        let context = gtk4::cairo::Context::new(&direct).expect("direct context");
        paint_scene(&context);
    }
    direct.flush();
    let direct_stride = direct.stride() as usize;
    let scene_pixels = {
        let data = direct.data().expect("direct data");
        cairo_argb_to_rgba_image(width as u32, height as u32, direct_stride, data.as_ref())
    };

    let crop = Rect {
        x: 17,
        y: 11,
        width: 160,
        height: 150,
    };
    let backdrop = liquid_preview_backdrop(&group, crop).expect("rasterized backdrop");
    for (x, y, pixel) in backdrop.enumerate_pixels() {
        assert_eq!(
            *pixel,
            *scene_pixels.get_pixel(crop.x as u32 + x, crop.y as u32 + y),
            "the preview must feed the shader the recorded scene pixels, ({x}, {y}) differs"
        );
    }

    let look = GlassLook::default();
    let ring = GlassRing {
        x: 60.0 - f64::from(crop.x),
        y: 50.0 - f64::from(crop.y),
        width: 80.0,
        height: 70.0,
        radius: 0.0,
        gap: 7.0,
    };
    let mut preview = LiquidPreview::new();
    let first =
        liquid_preview_layer(&mut preview, &group, crop, &ring, &look).expect("preview layer");
    let direct_layer = glass_layer(&backdrop, &ring, &look).expect("direct layer");
    assert_eq!(
        (first.x, first.y),
        (
            i64::from(crop.x) + direct_layer.x,
            i64::from(crop.y) + direct_layer.y
        ),
        "the preview layer should land at the crop origin plus the layer offset"
    );
    assert_eq!(
        first.image, direct_layer.image,
        "the preview layer must agree with the shader on the same pixels"
    );
    let again =
        liquid_preview_layer(&mut preview, &group, crop, &ring, &look).expect("cached layer");
    assert_eq!(
        again.image, first.image,
        "the cached layer must be reused unchanged"
    );
    let changed_look = GlassLook {
        body_shade: 0.4,
        ..look
    };
    let changed = liquid_preview_layer(&mut preview, &group, crop, &ring, &changed_look)
        .expect("updated layer");
    assert_ne!(
        changed.image, first.image,
        "changing the look must invalidate the cache"
    );
    assert_eq!(
        changed.image,
        glass_layer(&backdrop, &ring, &changed_look)
            .expect("updated direct layer")
            .image
    );
}

#[test]
fn liquid_glass_follows_the_card_squircle_corners() {
    let backdrop = RgbaImage::from_pixel(160, 160, image::Rgba([30, 60, 120, 255]));
    let ring = GlassRing {
        x: 40.0,
        y: 40.0,
        width: 80.0,
        height: 80.0,
        radius: 20.0,
        gap: 7.0,
    };
    let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("layer");
    let alpha = |x: i64, y: i64| {
        layer
            .image
            .get_pixel((x - layer.x) as u32, (y - layer.y) as u32)[3]
    };
    for (x, y) in [(44, 44), (115, 44), (44, 115), (115, 115)] {
        assert_eq!(
            alpha(x, y),
            0,
            "glass must not cover the card corner at {x},{y}"
        );
    }
    for (x, y) in [(42, 42), (117, 42), (42, 117), (117, 117)] {
        assert!(
            alpha(x, y) > 0,
            "glass must meet the curved corner at {x},{y}"
        );
    }
}

#[test]
fn liquid_glass_has_a_lit_body_without_an_inner_outline() {
    let backdrop = RgbaImage::from_pixel(200, 200, image::Rgba([70, 80, 110, 255]));
    let ring = GlassRing {
        x: 50.0,
        y: 50.0,
        width: 100.0,
        height: 100.0,
        radius: 20.0,
        gap: 7.0,
    };
    let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("layer");
    let pixel = |x: i64, y: i64| {
        *layer
            .image
            .get_pixel((x - layer.x) as u32, (y - layer.y) as u32)
    };
    let mid = pixel(100, 46);
    let inner = pixel(100, 49);
    assert!(mid[1] > 90, "the body must carry light, got {mid:?}");
    assert!(
        inner[1] <= mid[1] + 5,
        "the inner edge must not form a second highlight: {inner:?} vs {mid:?}"
    );
    let left = pixel(46, 65);
    let right = pixel(153, 135);
    assert!(
        left[1] > right[1] + 10,
        "the broad reflection must taper from upper-left to lower-right"
    );
}

#[test]
fn liquid_glass_body_samples_the_background_before_the_card_shadow() {
    let mut state = EditorState::new(RgbaImage::from_pixel(
        400,
        400,
        image::Rgba([25, 25, 25, 255]),
    ));
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.6, 0.3, 0.8, 1.0));
    state.background_padding = 80.0;
    state.background_shadow = 20.0;
    state.background_corner_radius = 20.0;
    state.frame_style = FrameStyle::Liquid;
    state.border_thickness = FrameStyle::Liquid.spec().border_thickness;
    state.shadow_opacity = 0.0;
    let without = state.to_final_image().expect("without shadow");
    state.shadow_opacity = 0.9;
    let with = state.to_final_image().expect("with shadow");
    let spec = state.frame_style.spec();
    let gap = state.border_thickness + spec.outer1.expect("glass rim").thickness;
    let body_y = 480 + (gap / 2.0).floor() as u32;
    assert_eq!(with.dimensions(), without.dimensions());
    assert_eq!(
        with.get_pixel(280, body_y),
        without.get_pixel(280, body_y),
        "the glass body must not inherit the shadow underneath it"
    );
    assert!(
        with.get_pixel(280, 496)[0] < without.get_pixel(280, 496)[0],
        "the shadow must still shade the surround outside the glass"
    );
}

#[test]
fn liquid_glass_body_stays_visible_on_every_side_of_light_and_dark_backdrops() {
    let ring = GlassRing {
        x: 50.0,
        y: 50.0,
        width: 100.0,
        height: 100.0,
        radius: 20.0,
        gap: 7.0,
    };
    for fill in [[10, 15, 30, 255], [0, 0, 0, 255], [245, 245, 245, 255]] {
        let backdrop = RgbaImage::from_pixel(200, 200, image::Rgba(fill));
        let layer = glass_layer(&backdrop, &ring, &GlassLook::default()).expect("glass layer");
        for (x, y) in [(46, 100), (153, 100), (100, 46), (100, 153)] {
            let pixel = layer
                .image
                .get_pixel((x - layer.x) as u32, (y - layer.y) as u32);
            let contrast: i32 = (0..3)
                .map(|channel| (i32::from(pixel[channel]) - i32::from(fill[channel])).abs())
                .sum();
            assert!(
                contrast >= 30,
                "the glass body must remain distinct at ({x},{y}) over {fill:?}: {pixel:?}"
            );
            assert_eq!(pixel[3], 255, "the body must not fade away on any side");
        }
    }
}

#[test]
fn glass_frost_siblings_share_the_3px_edge_with_opposite_tints() {
    let luma = |px: &image::Rgba<u8>| {
        0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2])
    };
    let mut band_luma = [0.0; 2];
    for (index, style) in [FrameStyle::GlassLight, FrameStyle::GlassDark]
        .into_iter()
        .enumerate()
    {
        let spec = style.spec();
        assert!(spec.liquid, "{style:?} must use the glass renderer");
        assert!(
            spec.frost,
            "{style:?} must retain its tinted Cairo approximation"
        );
        let rim = spec.outer1.expect("glass specular rim");
        assert!(
            (rim.thickness - 1.0).abs() < f64::EPSILON
                && (spec.border_thickness - 2.5).abs() < f64::EPSILON,
            "{style:?} must share the 3.5px glass geometry"
        );
        let image = RgbaImage::from_pixel(400, 400, image::Rgba([60, 60, 60, 255]));
        let mut state = EditorState::new(image);
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.5, 0.5, 0.5, 1.0));
        state.background_padding = 80.0;
        state.background_shadow = 0.0;
        state.background_corner_radius = 0.0;
        state.background_insert = 0.0;
        state.frame_style = style;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;
        let frame = state.to_final_image().expect("final image");
        band_luma[index] = luma(frame.get_pixel(280, 481));
    }
    assert!(
        band_luma[0] > band_luma[1] + 25.0,
        "Glass Light frost should read milky over smoked Glass Dark: {:?}",
        band_luma
    );
}

#[test]
fn glass_presets_share_the_reduced_geometry_and_liquid_optics() {
    let base = GlassLook::for_style(FrameStyle::Liquid);
    for (style, shade) in [
        (FrameStyle::Liquid, 0.0),
        (FrameStyle::GlassLight, -0.55),
        (FrameStyle::GlassDark, 0.55),
    ] {
        let spec = style.spec();
        let rim = spec.outer1.expect("glass rim");
        assert_eq!(spec.border_thickness + rim.thickness, 3.5);
        let look = GlassLook::for_style(style);
        assert_eq!(look.body_shade, shade);
        assert_eq!(
            GlassLook {
                body_shade: 0.0,
                ..look
            },
            base
        );
    }
    let source = include_str!("../window/canvas_render.rs");
    assert!(source.contains("let liquid_group = frame_style.spec().liquid;"));
    assert!(source.contains("&GlassLook::for_style(frame_style)"));
}

#[test]
fn clear_lens_follows_snells_law_across_the_bevel() {
    let gap = 7.0;
    let strength = GlassLook::default().refraction;
    assert_eq!(clear_lens(0.0, gap, strength), (0.0, 0.0));
    let (lip_slope, lip_bend) = clear_lens(1.0, gap, strength);
    assert!(
        lip_slope.is_finite() && lip_slope >= 1.0e4,
        "the lip slope must stay finite and steep, got {lip_slope}"
    );
    assert_eq!(lip_bend, 0.0, "the lip must not displace the sample");
    assert_eq!(clear_lens(-0.5, gap, strength), (0.0, 0.0));
    assert_eq!(clear_lens(2.0, gap, strength), (1.0e4, 0.0));

    let mut max_bend = 0.0_f64;
    for step in 1..100 {
        let u = f64::from(step) / 100.0;
        let (slope, bend) = clear_lens(u, gap, strength);
        assert!(slope.is_finite() && slope > 0.0, "slope at u={u}: {slope}");
        assert!(bend.is_finite(), "displacement at u={u}: {bend}");
        let theta_t = ((slope.atan()).sin() / 1.5).asin();
        assert!(theta_t.is_finite() && theta_t >= 0.0, "index law at u={u}");
        assert_eq!(
            clear_lens(u, gap, 0.0),
            (slope, 0.0),
            "zero strength must keep the slope and drop the displacement at u={u}"
        );
        max_bend = max_bend.max(bend);
    }
    assert!(
        max_bend > gap * 0.5,
        "the dome must pull samples more than half the band inward, got {max_bend}"
    );
}

#[test]
fn dump_liquid_glass_preview() {
    let sample_input = std::env::var_os("APEXSHOT_GLASS_SAMPLE_INPUT");
    let padding = if sample_input.is_some() { 30.0 } else { 60.0 };
    let image = sample_input
        .map(|path| image::open(path).expect("sample image").into_rgba8())
        .unwrap_or_else(|| RgbaImage::from_pixel(1373, 882, image::Rgba([150, 160, 175, 255])));
    let output = std::env::var_os("APEXSHOT_GLASS_SAMPLE_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    std::fs::create_dir_all(&output).expect("sample output directory");
    let spec = FrameStyle::Liquid.spec();
    let mut cases: Vec<(&str, BackgroundStyle)> = vec![
        (
            "purple",
            BackgroundStyle::Gradient(crate::recording::editor::model::VideoGradient {
                stops: vec![
                    crate::recording::editor::model::GradientStop::new(0.0, 0x4a, 0x1d, 0x8c),
                    crate::recording::editor::model::GradientStop::new(1.0, 0xd8, 0xb4, 0xff),
                ],
                ..Default::default()
            }),
        ),
        (
            "wallpaper",
            BackgroundStyle::Wallpaper(
                crate::capture::editor::window::background_panel::background_gradient_asset_path(
                    "wallpaper-001.jpg",
                ),
            ),
        ),
        (
            "black",
            BackgroundStyle::PlainColor(DrawColor::new(0.05, 0.05, 0.06, 1.0)),
        ),
        (
            "pink",
            BackgroundStyle::Gradient(crate::recording::editor::model::VideoGradient {
                stops: vec![
                    crate::recording::editor::model::GradientStop::new(0.0, 245, 139, 211),
                    crate::recording::editor::model::GradientStop::new(0.45, 205, 40, 194),
                    crate::recording::editor::model::GradientStop::new(1.0, 31, 12, 70),
                ],
                angle_degrees: 20.0,
                ..Default::default()
            }),
        ),
        (
            "navy",
            BackgroundStyle::PlainColor(DrawColor::new(0.04, 0.06, 0.12, 1.0)),
        ),
        (
            "white",
            BackgroundStyle::PlainColor(DrawColor::new(0.98, 0.98, 0.98, 1.0)),
        ),
    ];
    let checker_path = output.join("glass-sample-checker.png");
    let mut checker = RgbaImage::new(240, 240);
    for (x, y, pixel) in checker.enumerate_pixels_mut() {
        let light = ((x / 12) + (y / 12)) % 2 == 0;
        let value = if light { 245 } else { 20 };
        *pixel = image::Rgba([value, value, value, 255]);
    }
    checker.save(&checker_path).expect("checker fill");
    cases.push(("checker", BackgroundStyle::Wallpaper(checker_path)));
    if let Some(background) = std::env::var_os("APEXSHOT_GLASS_SAMPLE_BACKGROUND") {
        cases.push((
            "reference",
            BackgroundStyle::Wallpaper(std::path::PathBuf::from(background)),
        ));
    }
    for (name, style) in cases {
        let mut state = EditorState::new(image.clone());
        state.background_style = style;
        state.background_padding = padding;
        state.background_shadow = 20.0;
        state.background_corner_radius = 20.0;
        state.frame_style = FrameStyle::Liquid;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;
        let out = state.to_final_image().expect("final image");
        image::save_buffer(
            output.join(format!("glass-{name}.png")),
            &out,
            out.width(),
            out.height(),
            image::ColorType::Rgba8,
        )
        .expect("save");
    }

    if let Some(background) = std::env::var_os("APEXSHOT_GLASS_SAMPLE_BACKGROUND") {
        for (name, style) in [
            ("light-reference", FrameStyle::GlassLight),
            ("dark-reference", FrameStyle::GlassDark),
        ] {
            let spec = style.spec();
            let mut state = EditorState::new(image.clone());
            state.background_style = BackgroundStyle::Wallpaper(background.clone().into());
            state.background_padding = padding;
            state.background_shadow = 20.0;
            state.background_corner_radius = 20.0;
            state.frame_style = style;
            state.border_thickness = spec.border_thickness;
            state.border_color = spec.border_color;
            let out = state.to_final_image().expect("tinted final image");
            out.save(output.join(format!("glass-{name}.png")))
                .expect("tinted sample");
        }
        for (name, style) in [
            ("inset-light", FrameStyle::InsetLight),
            ("inset-dark", FrameStyle::InsetDark),
            ("border", FrameStyle::Border),
            ("retro", FrameStyle::Retro),
        ] {
            let spec = style.spec();
            let mut state = EditorState::new(image.clone());
            state.background_style = BackgroundStyle::Wallpaper(background.clone().into());
            state.background_padding = padding;
            state.background_corner_radius = 20.0;
            state.frame_style = style;
            state.border_thickness = spec.border_thickness;
            state.border_color = spec.border_color;
            let out = state.to_final_image().expect("frame final image");
            out.save(output.join(format!("frame-{name}.png")))
                .expect("frame sample");
        }
    }

    // Tinted siblings on black: Glass Light should glow whitish, Glass Dark
    // should deepen while keeping its edge light.
    for (name, style) in [
        ("light-black", FrameStyle::GlassLight),
        ("dark-black", FrameStyle::GlassDark),
    ] {
        let spec = style.spec();
        let mut state = EditorState::new(image.clone());
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.05, 0.05, 0.06, 1.0));
        state.background_padding = padding;
        state.background_shadow = 20.0;
        state.background_corner_radius = 20.0;
        state.frame_style = style;
        state.border_thickness = spec.border_thickness;
        state.border_color = spec.border_color;
        let out = state.to_final_image().expect("final image");
        image::save_buffer(
            output.join(format!("glass-{name}.png")),
            &out,
            out.width(),
            out.height(),
            image::ColorType::Rgba8,
        )
        .expect("save");
    }

    // Transparent canvas: only the glass highlights should survive.
    let mut state = EditorState::new(image.clone());
    state.background_style = BackgroundStyle::None;
    state.background_corner_radius = 20.0;
    state.frame_style = FrameStyle::Liquid;
    state.border_thickness = spec.border_thickness;
    state.border_color = spec.border_color;
    let out = state.to_final_image().expect("final image");
    let mut flat =
        image::RgbaImage::from_pixel(out.width(), out.height(), image::Rgba([0, 0, 0, 255]));
    image::imageops::overlay(&mut flat, &out, 0, 0);
    image::save_buffer(
        output.join("glass-transparent.png"),
        &flat,
        flat.width(),
        flat.height(),
        image::ColorType::Rgba8,
    )
    .expect("save");
}

#[test]
fn final_image_background_noise_grains_the_fill_but_not_the_card() {
    let image = RgbaImage::from_pixel(200, 150, image::Rgba([90, 90, 90, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.08, 0.08, 0.08, 1.0));
    state.background_padding = 40.0;
    state.background_insert = 0.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    let flat = state.to_final_image().expect("final image");
    state.background_noise = 0.8;
    let grainy = state.to_final_image().expect("final image");

    let fill = image::Rgba([20, 20, 20, 255]);
    assert_eq!(
        *flat.get_pixel(2, 2),
        fill,
        "without noise the plain fill stays flat"
    );
    // A single pixel can land on a faint speckle, so scan the fill band: once
    // the slider is up, the background must not be uniform any more.
    let grained = (0..60).any(|x| *grainy.get_pixel(x, 2) != *flat.get_pixel(x, 2));
    assert!(grained, "the exported fill must carry visible grain");

    // The screenshot composites above the grain: the card stays clean, and the
    // grain must not shift the canvas size.
    assert_eq!(flat.dimensions(), grainy.dimensions());
    let card = (grainy.width() / 2, grainy.height() / 2);
    assert_eq!(
        *grainy.get_pixel(card.0, card.1),
        *flat.get_pixel(card.0, card.1)
    );
    assert_eq!(
        *grainy.get_pixel(card.0, card.1),
        image::Rgba([90, 90, 90, 255])
    );
}

#[test]
fn final_image_without_a_fill_never_grains_the_transparent_surround() {
    let image = RgbaImage::from_pixel(40, 30, image::Rgba([90, 90, 90, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::None;
    state.background_noise = 1.0;

    let out = state.to_final_image().expect("final image");

    assert_eq!(out.dimensions(), (40, 30));
    assert_eq!(*out.get_pixel(0, 0), image::Rgba([90, 90, 90, 255]));
}

#[test]
fn final_image_background_blur_softens_the_fill_and_keeps_grain_crisp() {
    // A hard-edged wallpaper makes the blur measurable.
    let path = std::env::temp_dir().join("apexshot-background-blur-fixture.png");
    let mut wallpaper = RgbaImage::new(140, 120);
    for y in 0..120 {
        for x in 0..140 {
            let value = if x < 70 { 0 } else { 255 };
            wallpaper.put_pixel(x, y, image::Rgba([value, value, value, 255]));
        }
    }
    wallpaper.save(&path).expect("fixture");

    let screenshot = RgbaImage::from_pixel(80, 60, image::Rgba([90, 90, 90, 255]));
    let mut state = EditorState::new(screenshot);
    state.background_style = BackgroundStyle::Wallpaper(path.clone());
    state.background_padding = 30.0;
    state.background_insert = 0.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    // Row 2 sits in the top padding band, so it is fill only.
    let row = |image: &RgbaImage| -> Vec<u8> {
        (0..image.width())
            .map(|x| image.get_pixel(x, 2)[0])
            .collect()
    };
    // Pixels between the two fill values: the width of the softened edge.
    let ramp = |values: &[u8]| {
        values
            .iter()
            .filter(|value| **value > 20 && **value < 235)
            .count()
    };
    let flat_run = |image: &RgbaImage| -> f64 {
        let values: Vec<f64> = (0..30)
            .map(|x| f64::from(image.get_pixel(x, 2)[0]))
            .collect();
        values
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .sum::<f64>()
            / (values.len() - 1) as f64
    };

    let sharp = state.to_final_image().expect("final image");
    state.background_blur = 1.0;
    let blurred = state.to_final_image().expect("final image");

    assert!(
        ramp(&row(&sharp)) <= 4,
        "the wallpaper edge stays hard without blur",
    );
    assert!(
        ramp(&row(&blurred)) > 12,
        "the Appearance blur must soften the fill's edge",
    );

    // The card composites above the blurred fill and is never softened.
    let card = (blurred.width() / 2, blurred.height() / 2);
    assert_eq!(
        *blurred.get_pixel(card.0, card.1),
        image::Rgba([90, 90, 90, 255])
    );

    // Grain is painted after the blur, so the fill keeps per-pixel speckle
    // instead of being smoothed into the blurred background.
    let blurred_flat = flat_run(&blurred);
    state.background_noise = 1.0;
    let grainy = state.to_final_image().expect("final image");
    let grainy_flat = flat_run(&grainy);

    assert!(
        blurred_flat < 2.0,
        "a blurred fill is smooth on its own, got {blurred_flat}",
    );
    assert!(
        grainy_flat > 4.0,
        "grain must stay crisp on top of the blur, got {grainy_flat}",
    );

    let _ = std::fs::remove_file(&path);
}

/// The static style carries the shared `VideoGradient` spec, so a custom
/// gradient exports as itself instead of falling back to a bundled preset
/// image (the bug this replaced).
#[test]
fn final_image_renders_a_gradient_background_from_its_spec() {
    use crate::recording::editor::model::{GradientStop, VideoGradient};

    let screenshot = RgbaImage::from_pixel(80, 60, image::Rgba([90, 90, 90, 255]));
    let mut state = EditorState::new(screenshot);
    state.background_style = BackgroundStyle::Gradient(VideoGradient {
        stops: vec![
            GradientStop::new(0.0, 255, 0, 0),
            GradientStop::new(1.0, 0, 0, 255),
        ],
        ..Default::default()
    });
    state.background_padding = 30.0;
    state.background_insert = 0.0;
    state.background_shadow = 0.0;
    state.background_corner_radius = 0.0;

    let out = state.to_final_image().expect("final image");
    // Row 2 is fill only (the top padding band). Angle 0 runs left-to-right,
    // so the band's left edge is the first stop and its right edge the last.
    let left = *out.get_pixel(1, 2);
    let right = *out.get_pixel(out.width() - 2, 2);
    assert!(
        left[0] > 200 && left[2] < 60,
        "the first stop should paint the left edge, got {left:?}",
    );
    assert!(
        right[2] > 200 && right[0] < 60,
        "the last stop should paint the right edge, got {right:?}",
    );
}

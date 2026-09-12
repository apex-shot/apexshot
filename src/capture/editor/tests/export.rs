use super::*;

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
fn final_image_applies_crop_selection() {
    let mut image = RgbaImage::new(12, 10);
    for y in 0..10 {
        for x in 0..12 {
            image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 80, 255]));
        }
    }

    let mut state = EditorState::new(image.clone());
    state.crop_selection = Some(Rect {
        x: 3,
        y: 2,
        width: 5,
        height: 4,
    });

    let final_image = state.to_final_image().unwrap();
    assert_eq!(final_image.dimensions(), (5, 4));
    assert_eq!(*final_image.get_pixel(0, 0), *image.get_pixel(3, 2));
    assert_eq!(*final_image.get_pixel(4, 3), *image.get_pixel(7, 5));
}

#[test]
fn final_image_keeps_background_for_tall_capture_crop() {
    let mut image = RgbaImage::from_pixel(120, 1200, image::Rgba([240, 240, 240, 255]));
    image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));

    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.05, 0.05, 0.05, 1.0));
    state.background_padding = 36.0;
    state.background_shadow = 30.0;
    state.crop_selection = Some(Rect {
        x: -20,
        y: -10,
        width: 180,
        height: 1240,
    });

    let final_image = state.to_final_image().expect("final image");

    assert!(final_image.width() > 180);
    assert!(final_image.height() > 1240);
    assert_eq!(*final_image.get_pixel(0, 0), image::Rgba([12, 12, 12, 255]));
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

    assert_eq!(final_image.dimensions(), (448, 348));
    assert_eq!(*final_image.get_pixel(24, 24), *image.get_pixel(0, 0));
    assert_eq!(*final_image.get_pixel(423, 323), *image.get_pixel(399, 299));
}

#[test]
fn final_image_background_shadow_visibly_darkens_pixels_below_card() {
    let image = RgbaImage::from_pixel(400, 240, image::Rgba([220, 220, 220, 255]));
    let mut state = EditorState::new(image);
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
    state.background_padding = 40.0;
    state.background_insert = 0.0;
    state.background_shadow = 45.0;
    state.background_corner_radius = 18.0;

    let final_image = state.to_final_image().expect("final image");
    let shadow_pixel = *final_image.get_pixel(final_image.width() / 2, 290);

    assert!(
        shadow_pixel[0] < 235,
        "expected visible shadow below the card, got pixel {:?}",
        shadow_pixel
    );
}

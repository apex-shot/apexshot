use super::*;

#[test]
fn blur_changes_pixels_inside_rect_only() {
    let mut image = RgbaImage::new(10, 10);
    for y in 0..10 {
        for x in 0..10 {
            let value = if x < 5 { 0 } else { 255 };
            image.put_pixel(x, y, image::Rgba([value, value, value, 255]));
        }
    }

    let rect = Rect {
        x: 2,
        y: 2,
        width: 6,
        height: 6,
    };
    let before_outside = *image.get_pixel(0, 0);

    render::apply_blur_rect(&mut image, rect, 2.0, false);

    let inside = *image.get_pixel(4, 4);
    let outside = *image.get_pixel(0, 0);

    assert_ne!(inside[0], 0);
    assert_eq!(outside, before_outside);
}

#[test]
fn censor_pixelates_pixels_inside_rect_only() {
    let mut image = RgbaImage::new(12, 12);
    for y in 0..12 {
        for x in 0..12 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 7) as u8, (y * 11) as u8, ((x + y) * 5) as u8, 255]),
            );
        }
    }

    let rect = Rect {
        x: 2,
        y: 2,
        width: 7,
        height: 7,
    };

    let before_outside = *image.get_pixel(0, 0);
    let before_inside = *image.get_pixel(2, 2);

    render::apply_censor_rect(&mut image, rect, 3.0);

    let outside = *image.get_pixel(0, 0);
    let inside = *image.get_pixel(2, 2);

    assert_eq!(outside, before_outside);
    assert_ne!(inside, before_inside);

    let block_a = *image.get_pixel(2, 2);
    for y in 2..5 {
        for x in 2..5 {
            assert_eq!(*image.get_pixel(x, y), block_a);
        }
    }

    let block_b = *image.get_pixel(5, 2);
    for y in 2..5 {
        for x in 5..8 {
            assert_eq!(*image.get_pixel(x, y), block_b);
        }
    }
}

#[test]
fn focus_darkens_pixels_outside_rect_only() {
    let mut image = RgbaImage::new(12, 12);
    for y in 0..12 {
        for x in 0..12 {
            image.put_pixel(x, y, image::Rgba([180, 170, 160, 255]));
        }
    }

    let rect = Rect {
        x: 3,
        y: 3,
        width: 6,
        height: 6,
    };

    let before_inside = *image.get_pixel(4, 4);
    let before_outside = *image.get_pixel(1, 1);

    render::apply_focus_rect(&mut image, rect, 58.0);

    let inside = *image.get_pixel(4, 4);
    let outside = *image.get_pixel(1, 1);

    assert_eq!(inside, before_inside);
    assert!(outside[0] < before_outside[0]);
    assert!(outside[1] < before_outside[1]);
    assert!(outside[2] < before_outside[2]);
}

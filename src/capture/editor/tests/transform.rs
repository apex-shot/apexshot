use super::*;

#[test]
fn rect_from_points_normalizes_values() {
    let start = Point { x: 20.0, y: 10.0 };
    let end = Point { x: 2.0, y: 3.0 };
    let rect = Rect::from_points(start, end).unwrap();

    assert_eq!(rect.x, 2);
    assert_eq!(rect.y, 3);
    assert_eq!(rect.width, 18);
    assert_eq!(rect.height, 7);
}

#[test]
fn view_transform_scales_to_fit() {
    let t = ViewTransform::fit(4000.0, 2000.0, 1000.0, 500.0);

    assert!((t.scale - 0.25).abs() < f64::EPSILON);
    assert!((t.offset_x - 0.0).abs() < f64::EPSILON);
    assert!((t.offset_y - 0.0).abs() < f64::EPSILON);

    let mapped = t.view_to_image_clamped(Point { x: 500.0, y: 250.0 });
    assert!((mapped.x - 2000.0).abs() < f64::EPSILON);
    assert!((mapped.y - 1000.0).abs() < f64::EPSILON);
}

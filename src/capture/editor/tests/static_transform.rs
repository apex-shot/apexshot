use super::*;
use crate::capture::editor::image_transform::StaticImageTransform;

#[test]
fn identity_transform_keeps_the_existing_still_render_path() {
    let state = EditorState::new(RgbaImage::from_pixel(
        24,
        12,
        image::Rgba([12, 34, 56, 255]),
    ));
    assert_eq!(
        state.to_final_image().expect("identity still renders"),
        state.to_rendered_image().expect("legacy path renders"),
    );
}

#[test]
fn frozen_motion_pose_keeps_the_background_fixed() {
    let mut state = EditorState::new(RgbaImage::from_pixel(
        32,
        20,
        image::Rgba([220, 20, 10, 255]),
    ));
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 1.0, 1.0));
    state.background_padding = 48.0;
    let before = state.to_final_image().expect("background still renders");
    state.static_image_transform = StaticImageTransform {
        yaw: 0.0,
        pitch: -24.0,
        roll: 0.0,
        perspective: 0.18,
        pos_x: 0.35,
        pos_y: -0.2,
    };
    let after = state.to_final_image().expect("transformed still renders");
    assert_eq!(before.dimensions(), after.dimensions());
    assert_eq!(before.get_pixel(0, 0), after.get_pixel(0, 0));
    assert_ne!(before, after);
}

#[test]
fn annotations_move_with_the_frozen_motion_card() {
    let mut state = EditorState::new(RgbaImage::from_pixel(
        24,
        16,
        image::Rgba([20, 60, 180, 255]),
    ));
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 0.0, 1.0));
    state.background_padding = 200.0;
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 2,
            y: 3,
            width: 8,
            height: 6,
        },
        color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
        stroke_size: 2.0,
        shadow: false,
    });
    let before = state
        .to_final_image()
        .expect("untransformed annotations render");
    state.static_image_transform = StaticImageTransform {
        pos_x: 0.25,
        ..Default::default()
    };
    let transformed = state
        .to_final_image()
        .expect("transformed annotations render");
    let red_bounds = |image: &RgbaImage| {
        image
            .enumerate_pixels()
            .filter(|(_, _, pixel)| pixel[0] > 150 && pixel[1] < 100 && pixel[2] < 100)
            .map(|(x, y, _)| (x, y))
            .fold(None::<(u32, u32, u32, u32)>, |bounds, point| {
                Some(match bounds {
                    None => (point.0, point.1, point.0, point.1),
                    Some((min_x, min_y, max_x, max_y)) => (
                        min_x.min(point.0),
                        min_y.min(point.1),
                        max_x.max(point.0),
                        max_y.max(point.1),
                    ),
                })
            })
            .expect("red annotation remains visible")
    };
    assert_ne!(red_bounds(&transformed), red_bounds(&before));
}

#[test]
fn motion_card_source_is_independent_of_the_static_pose() {
    let image = RgbaImage::from_pixel(24, 12, image::Rgba([12, 34, 56, 255]));
    let mut state = EditorState::new(image.clone());
    state.static_image_transform = StaticImageTransform {
        yaw: 12.0,
        pitch: -18.0,
        roll: 7.0,
        perspective: 0.7,
        pos_x: 0.25,
        pos_y: -0.3,
    };
    assert_eq!(state.to_motion_card_image().unwrap(), image);
}

#[test]
fn crop_tool_keeps_the_frozen_pose_in_export() {
    let mut state = EditorState::new(RgbaImage::from_fn(32, 20, |x, y| {
        image::Rgba([(x * 7) as u8, (y * 11) as u8, (x + y) as u8, 255])
    }));
    state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 1.0, 1.0));
    state.background_padding = 96.0;
    state.static_image_transform = StaticImageTransform {
        yaw: 12.0,
        pitch: -13.0,
        perspective: 0.18,
        pos_x: 0.4,
        ..Default::default()
    };
    state.set_tool_without_rebuild(Tool::Crop);
    let stored = state.static_image_transform;
    let crop_export = state.to_final_image().expect("crop-mode export");
    state.set_tool_without_rebuild(Tool::Background);
    assert_eq!(state.static_image_transform, stored);
    assert_eq!(state.to_final_image().unwrap(), crop_export);
}

use super::*;

#[test]
fn set_tool_clears_crop_selection_when_leaving_crop_mode() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Crop);
    state.crop_selection = Some(Rect {
        x: 2,
        y: 2,
        width: 10,
        height: 10,
    });

    state.set_tool(Tool::Arrow);
    assert!(state.crop_selection.is_none());
}

#[test]
fn ensure_crop_selection_initialized_creates_default_crop_frame() {
    let mut state = EditorState::new(RgbaImage::new(200, 120));

    assert!(state.ensure_crop_selection_initialized());
    let crop = state.crop_selection.unwrap();
    assert_eq!(crop.x, 0);
    assert_eq!(crop.y, 0);
    assert_eq!(crop.width, 200);
    assert_eq!(crop.height, 120);
    assert!(!state.ensure_crop_selection_initialized());
}

#[test]
fn crop_drag_moves_existing_selection_without_redrawing_first() {
    let mut state = EditorState::new(RgbaImage::new(160, 100));
    state.set_tool(Tool::Crop);
    state.crop_selection = Some(Rect {
        x: 20,
        y: 18,
        width: 80,
        height: 48,
    });

    assert!(state.begin_crop_drag_with_scale(Point { x: 40.0, y: 30.0 }, 1.0));
    assert!(state.update_crop_drag(Point { x: 52.0, y: 41.0 }));
    state.end_crop_drag();

    let crop = state.crop_selection.unwrap();
    assert_eq!(crop.x, 32);
    assert_eq!(crop.y, 29);
    assert_eq!(crop.width, 80);
    assert_eq!(crop.height, 48);
}

#[test]
fn crop_edge_drag_resizes_selection_by_moving_single_edge() {
    let mut state = EditorState::new(RgbaImage::new(160, 100));
    state.set_tool(Tool::Crop);
    state.crop_selection = Some(Rect {
        x: 20,
        y: 18,
        width: 80,
        height: 48,
    });

    // Click on right edge (at x=100 which is 20+80)
    assert!(state.begin_crop_drag_with_scale(Point { x: 100.0, y: 42.0 }, 1.0));
    // Drag left by 8 pixels
    assert!(state.update_crop_drag(Point { x: 92.0, y: 42.0 }));
    state.end_crop_drag();

    let crop = state.crop_selection.unwrap();
    // Left edge should stay at 20, right edge should move to 92
    // Width = 92 - 20 = 72
    assert_eq!(crop.x, 20);
    assert_eq!(crop.y, 18);
    assert_eq!(crop.width, 72);
    assert_eq!(crop.height, 48);
}

#[test]
fn apply_crop_selection_flattens_editor_state() {
    let mut image = RgbaImage::new(16, 12);
    for y in 0..12 {
        for x in 0..16 {
            image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 120, 255]));
        }
    }

    let mut state = EditorState::new(image);
    state.crop_selection = Some(Rect {
        x: 4,
        y: 3,
        width: 8,
        height: 5,
    });

    assert!(state.apply_crop_selection().unwrap());
    assert_eq!(state.base_image.dimensions(), (8, 5));
    assert_eq!(state.working_image.dimensions(), (8, 5));
    assert!(state.actions.is_empty());
    assert!(state.redo_actions.is_empty());
    assert!(state.crop_selection.is_none());
}

use super::*;

#[test]
fn select_handle_cursor_mapping_matches_expected_names() {
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::TopLeft),
        "nw-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::Top),
        "ns-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::TopRight),
        "ne-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::Left),
        "ew-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::Right),
        "ew-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::BottomLeft),
        "sw-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::Bottom),
        "ns-resize"
    );
    assert_eq!(
        cursor_name_for_select_handle(SelectHandle::BottomRight),
        "se-resize"
    );
    assert_eq!(cursor_name_for_select_handle(SelectHandle::Start), "move");
    assert_eq!(cursor_name_for_select_handle(SelectHandle::End), "move");
}

#[test]
fn cursor_name_for_view_point_uses_select_handle_and_grab_states() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
    state.set_tool(Tool::Select);
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 20,
            y: 16,
            width: 24,
            height: 18,
        },
        color: DRAW_COLORS[1],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });
    state.selected_action_index = Some(0);

    let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 20.0, y: 16.0 }),
        "nw-resize"
    );
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 28.0, y: 22.0 }),
        "grab"
    );

    state.select_drag_anchor = Some(Point { x: 20.0, y: 16.0 });
    state.select_resize_handle = Some(SelectHandle::Bottom);
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 30.0 }),
        "ns-resize"
    );

    state.select_resize_handle = None;
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 30.0 }),
        "grabbing"
    );
}

#[test]
fn cursor_name_for_view_point_matches_text_and_crosshair_modes() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
    let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);

    state.set_tool(Tool::Text);
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 12.0, y: 12.0 }),
        "text"
    );

    state.set_tool(Tool::Arrow);
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 12.0, y: 12.0 }),
        "crosshair"
    );
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: -4.0, y: -4.0 }),
        "default"
    );
}

#[test]
fn cursor_name_for_view_point_uses_crop_drag_and_resize_states() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
    let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);
    state.set_tool(Tool::Crop);
    state.crop_selection = Some(Rect {
        x: 20,
        y: 16,
        width: 24,
        height: 18,
    });

    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 32.0, y: 16.0 }),
        "ns-resize"
    );
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 28.0, y: 22.0 }),
        "grab"
    );

    state.select_drag_anchor = Some(Point { x: 32.0, y: 16.0 });
    state.select_resize_handle = Some(SelectHandle::Right);
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 36.0, y: 22.0 }),
        "ew-resize"
    );

    state.select_resize_handle = None;
    assert_eq!(
        cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 24.0 }),
        "grabbing"
    );
}

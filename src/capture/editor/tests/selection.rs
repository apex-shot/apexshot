use super::*;

#[test]
fn select_action_prefers_topmost_match() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 8,
            y: 8,
            width: 22,
            height: 22,
        },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 14,
            y: 14,
            width: 22,
            height: 22,
        },
        color: DRAW_COLORS[1],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert!(state.select_action_at_point_with_scale(Point { x: 20.0, y: 20.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(1));
}

#[test]
fn select_drag_moves_annotation() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Line {
        start: Point { x: 4.0, y: 6.0 },
        end: Point { x: 18.0, y: 20.0 },
        color: DRAW_COLORS[2],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    // Click near the midpoint of the line (away from endpoints) to trigger drag, not resize
    assert!(state.begin_select_drag_with_scale(Point { x: 11.0, y: 13.0 }, 1.0));
    assert!(state.update_select_drag(Point { x: 18.0, y: 22.0 }));
    state.end_select_drag();

    match &state.actions[0] {
        AnnotationAction::Line { start, end, .. } => {
            // Delta: (18.0-11.0, 22.0-13.0) = (7.0, 9.0)
            assert_eq!(*start, Point { x: 11.0, y: 15.0 });
            assert_eq!(*end, Point { x: 25.0, y: 29.0 });
        }
        other => panic!("unexpected action after selection drag: {:?}", other),
    }
}

#[test]
fn select_action_scales_hit_padding_for_zoomed_out_view() {
    let mut state = EditorState::new(RgbaImage::new(120, 120));
    state.push_action(AnnotationAction::Line {
        start: Point { x: 12.0, y: 20.0 },
        end: Point { x: 92.0, y: 20.0 },
        color: DRAW_COLORS[3],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert!(!state.select_action_at_point_with_scale(Point { x: 40.0, y: 45.0 }, 1.0));
    assert!(state.select_action_at_point_with_scale(Point { x: 40.0, y: 45.0 }, 0.2));
}

#[test]
fn select_handle_hit_radius_scales_for_zoomed_out_view() {
    let mut state = EditorState::new(RgbaImage::new(120, 120));
    state.push_action(AnnotationAction::Line {
        start: Point { x: 10.0, y: 10.0 },
        end: Point { x: 70.0, y: 42.0 },
        color: DRAW_COLORS[2],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert!(state.select_action_at_point_with_scale(Point { x: 34.0, y: 23.0 }, 1.0));
    assert!(state.begin_select_drag_with_scale(Point { x: 24.0, y: 20.0 }, 0.2));
    assert_eq!(state.select_resize_handle, Some(SelectHandle::Start));
}

#[test]
fn select_handle_detection_for_box_corners() {
    let action = AnnotationAction::Box {
        rect: Rect {
            x: 10,
            y: 12,
            width: 18,
            height: 14,
        },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    };

    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 10.0, y: 12.0 }),
        Some(SelectHandle::TopLeft)
    );
    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 28.0, y: 26.0 }),
        Some(SelectHandle::BottomRight)
    );
    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 19.0, y: 19.0 }),
        None
    );
}

#[test]
fn select_handle_detection_for_box_edges() {
    let action = AnnotationAction::Box {
        rect: Rect {
            x: 10,
            y: 12,
            width: 18,
            height: 14,
        },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    };

    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 19.0, y: 12.0 }),
        Some(SelectHandle::Top)
    );
    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 10.0, y: 19.0 }),
        Some(SelectHandle::Left)
    );
    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 28.0, y: 19.0 }),
        Some(SelectHandle::Right)
    );
    assert_eq!(
        action_resize_handle_at_point(&action, Point { x: 19.0, y: 26.0 }),
        Some(SelectHandle::Bottom)
    );
}

#[test]
fn select_resize_corner_updates_box_geometry() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
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

    assert!(state.select_action_at_point_with_scale(Point { x: 24.0, y: 20.0 }, 1.0));
    assert!(state.begin_select_drag_with_scale(Point { x: 20.0, y: 16.0 }, 1.0));
    assert!(state.update_select_drag(Point { x: 14.0, y: 12.0 }));
    state.end_select_drag();

    match &state.actions[0] {
        AnnotationAction::Box { rect, .. } => {
            assert_eq!(rect.x, 14);
            assert_eq!(rect.y, 12);
            assert_eq!(rect.width, 30);
            assert_eq!(rect.height, 22);
        }
        other => panic!("unexpected action after corner resize: {:?}", other),
    }
}

#[test]
fn select_resize_edge_updates_box_geometry() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
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

    assert!(state.select_action_at_point_with_scale(Point { x: 24.0, y: 20.0 }, 1.0));
    assert!(state.begin_select_drag_with_scale(Point { x: 32.0, y: 16.0 }, 1.0));
    assert!(state.update_select_drag(Point { x: 32.0, y: 11.0 }));
    state.end_select_drag();

    match &state.actions[0] {
        AnnotationAction::Box { rect, .. } => {
            assert_eq!(rect.x, 20);
            assert_eq!(rect.y, 11);
            assert_eq!(rect.width, 24);
            assert_eq!(rect.height, 23);
        }
        other => panic!("unexpected action after edge resize: {:?}", other),
    }
}

#[test]
fn select_resize_endpoint_updates_line_geometry() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
    state.push_action(AnnotationAction::Line {
        start: Point { x: 8.0, y: 8.0 },
        end: Point { x: 28.0, y: 22.0 },
        color: DRAW_COLORS[2],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert!(state.select_action_at_point_with_scale(Point { x: 12.0, y: 11.0 }, 1.0));
    assert!(state.begin_select_drag_with_scale(Point { x: 8.0, y: 8.0 }, 1.0));
    assert!(state.update_select_drag(Point { x: 3.0, y: 11.0 }));
    state.end_select_drag();

    match &state.actions[0] {
        AnnotationAction::Line { start, end, .. } => {
            assert_eq!(*start, Point { x: 3.0, y: 11.0 });
            assert_eq!(*end, Point { x: 28.0, y: 22.0 });
        }
        other => panic!("unexpected action after endpoint resize: {:?}", other),
    }
}

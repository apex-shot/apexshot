use super::*;

#[test]
fn draft_action_uses_selected_color() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Arrow);
    state.set_color_index(3);
    state.begin_drag(Point { x: 2.0, y: 2.0 });
    state.update_drag(Point { x: 8.0, y: 8.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Arrow { color, .. } => {
            assert_eq!(color, DRAW_COLORS[3]);
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_action_returns_pen_points_when_tool_is_pen() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Pen);
    state.set_color_index(2);
    state.begin_drag(Point { x: 1.0, y: 1.0 });
    state.update_drag(Point { x: 4.0, y: 4.0 });
    state.update_drag(Point { x: 7.0, y: 5.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Pen { points, color, .. } => {
            assert_eq!(color, DRAW_COLORS[2]);
            assert_eq!(points.len(), 3);
            assert_eq!(points[0], Point { x: 1.0, y: 1.0 });
            assert_eq!(points[2], Point { x: 7.0, y: 5.0 });
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_action_returns_highlighter_points_when_tool_is_highlighter() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Highlighter);
    state.set_color_index(0);
    state.begin_drag(Point { x: 3.0, y: 3.0 });
    state.update_drag(Point { x: 9.0, y: 6.0 });
    state.update_drag(Point { x: 14.0, y: 6.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Highlighter { points, color, .. } => {
            assert_eq!(color, DRAW_COLORS[0]);
            assert_eq!(points.len(), 3);
            assert_eq!(points[0], Point { x: 3.0, y: 3.0 });
            assert_eq!(points[2], Point { x: 14.0, y: 6.0 });
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_action_returns_line_when_tool_is_line() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Line);
    state.set_color_index(4);
    state.begin_drag(Point { x: 2.0, y: 3.0 });
    state.update_drag(Point { x: 11.0, y: 13.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Line {
            start, end, color, ..
        } => {
            assert_eq!(start, Point { x: 2.0, y: 3.0 });
            assert_eq!(end, Point { x: 11.0, y: 13.0 });
            assert_eq!(color, DRAW_COLORS[4]);
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_line_with_shift_snaps_to_aligned_angle() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Line);
    state.drag_shift_active = true;
    state.begin_drag(Point { x: 2.0, y: 3.0 });
    state.update_drag(Point { x: 11.0, y: 13.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Line { start, end, .. } => {
            assert_eq!(start, Point { x: 2.0, y: 3.0 });
            assert_eq!(end, Point { x: 12.0, y: 13.0 });
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_action_returns_circle_when_tool_is_circle() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Circle);
    state.set_color_index(1);
    state.begin_drag(Point { x: 4.0, y: 5.0 });
    state.update_drag(Point { x: 13.0, y: 16.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Circle { rect, color, .. } => {
            assert_eq!(rect.x, 4);
            assert_eq!(rect.y, 5);
            assert_eq!(rect.width, 9);
            assert_eq!(rect.height, 11);
            assert_eq!(color, DRAW_COLORS[1]);
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn finalize_pen_drag_returns_action_and_clears_drag_state() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Pen);
    state.begin_drag(Point { x: 2.0, y: 2.0 });
    state.update_drag(Point { x: 6.0, y: 6.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Pen { points, .. } => {
            assert!(points.len() >= 2);
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }

    assert!(state.drag_start.is_none());
    assert!(state.drag_current.is_none());
    assert!(state.drag_path.is_empty());
}

#[test]
fn finalize_highlighter_drag_returns_action_and_clears_drag_state() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Highlighter);
    state.begin_drag(Point { x: 2.0, y: 8.0 });
    state.update_drag(Point { x: 10.0, y: 8.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Highlighter { points, .. } => {
            assert!(points.len() >= 2);
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }

    assert!(state.drag_start.is_none());
    assert!(state.drag_current.is_none());
    assert!(state.drag_path.is_empty());
}

#[test]
fn finalize_highlighter_with_shift_flattens_to_horizontal_segment() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Highlighter);
    state.drag_shift_active = true;
    state.begin_drag(Point { x: 2.0, y: 8.0 });
    state.update_drag(Point { x: 10.0, y: 12.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Highlighter { points, .. } => {
            assert_eq!(points.len(), 2);
            assert_eq!(points[0], Point { x: 2.0, y: 8.0 });
            assert_eq!(points[1], Point { x: 10.0, y: 8.0 });
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }
}

#[test]
fn finalize_line_drag_returns_action_and_clears_drag_state() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Line);
    state.begin_drag(Point { x: 1.0, y: 2.0 });
    state.update_drag(Point { x: 8.0, y: 9.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Line { start, end, .. } => {
            assert_eq!(start, Point { x: 1.0, y: 2.0 });
            assert_eq!(end, Point { x: 8.0, y: 9.0 });
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }

    assert!(state.drag_start.is_none());
    assert!(state.drag_current.is_none());
}

#[test]
fn finalize_circle_drag_returns_action_and_clears_drag_state() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Circle);
    state.begin_drag(Point { x: 3.0, y: 4.0 });
    state.update_drag(Point { x: 10.0, y: 14.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Circle { rect, .. } => {
            assert_eq!(rect.x, 3);
            assert_eq!(rect.y, 4);
            assert_eq!(rect.width, 7);
            assert_eq!(rect.height, 10);
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }

    assert!(state.drag_start.is_none());
    assert!(state.drag_current.is_none());
}

#[test]
fn annotate_smooth_drawing_simplifies_pen_points() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.smooth_drawing_enabled = true;
    state.set_tool(Tool::Pen);
    state.begin_drag(Point { x: 0.0, y: 0.0 });
    state.drag_path = vec![
        Point { x: 0.0, y: 0.0 },
        Point { x: 5.0, y: 0.2 },
        Point { x: 10.0, y: 0.0 },
        Point { x: 15.0, y: 0.2 },
        Point { x: 20.0, y: 0.0 },
    ];
    state.update_drag(Point { x: 20.0, y: 0.0 });

    match state.finalize_drag_action().unwrap() {
        AnnotationAction::Pen { points, .. } => {
            assert_eq!(points.first(), Some(&Point { x: 0.0, y: 0.0 }));
            assert_eq!(points.last(), Some(&Point { x: 20.0, y: 0.0 }));
            assert!(points.len() < 5);
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }
}

#[test]
fn annotate_draw_shadow_applies_to_new_box_actions() {
    let mut state = EditorState::new(RgbaImage::new(80, 80));
    state.draw_object_shadow = true;
    state.set_tool(Tool::Box);
    state.begin_drag(Point { x: 10.0, y: 10.0 });
    state.update_drag(Point { x: 30.0, y: 32.0 });

    match state.finalize_drag_action().unwrap() {
        AnnotationAction::Box { shadow, .. } => assert!(shadow),
        other => panic!("unexpected finalized action: {:?}", other),
    }
}

#[test]
fn annotate_auto_expand_grows_canvas_for_new_action() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.auto_expand_canvas = true;
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 15,
            y: 16,
            width: 18,
            height: 12,
        },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert_eq!(state.working_image.width(), 33);
    assert_eq!(state.working_image.height(), 28);
}

#[test]
fn draft_action_returns_obfuscate_rect_when_tool_is_obfuscate() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Obfuscate);
    state.begin_drag(Point { x: 1.0, y: 1.0 });
    state.update_drag(Point { x: 9.0, y: 8.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Obfuscate {
            rect,
            method,
            amount,
        } => {
            assert_eq!(rect.x, 1);
            assert_eq!(rect.y, 1);
            assert_eq!(rect.width, 8);
            assert_eq!(rect.height, 7);
            assert_eq!(method, ObfuscateMethod::Pixelate);
            assert!(amount > 0.0);
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

#[test]
fn draft_action_returns_focus_rect_when_tool_is_focus() {
    let mut state = EditorState::new(RgbaImage::new(20, 20));
    state.set_tool(Tool::Focus);
    state.begin_drag(Point { x: 2.0, y: 3.0 });
    state.update_drag(Point { x: 13.0, y: 15.0 });

    match state.draft_action().unwrap() {
        AnnotationAction::Focus { rect, intensity } => {
            assert_eq!(rect.x, 2);
            assert_eq!(rect.y, 3);
            assert_eq!(rect.width, 11);
            assert_eq!(rect.height, 12);
            assert_eq!(intensity, 58.0);
        }
        other => panic!("unexpected draft action: {:?}", other),
    }
}

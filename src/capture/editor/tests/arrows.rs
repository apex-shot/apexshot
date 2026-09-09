use super::*;

#[test]
fn annotate_inverse_arrow_reverses_new_arrow_direction() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.inverse_arrow_direction = true;
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 10.0, y: 20.0 });
    state.update_drag(Point { x: 90.0, y: 20.0 });

    match state.finalize_drag_action().unwrap() {
        AnnotationAction::Arrow { start, end, .. } => {
            assert_eq!(start, Point { x: 90.0, y: 20.0 });
            assert_eq!(end, Point { x: 10.0, y: 20.0 });
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }
}

#[test]
fn arrow_style_persists_through_state() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    for style in ArrowStyle::ALL {
        state.set_arrow_style(style);
        assert_eq!(state.arrow_style, style);
    }
}

#[test]
fn zero_length_arrow_finalization_returns_none_for_all_styles() {
    for style in ArrowStyle::ALL {
        let mut state = EditorState::new(RgbaImage::new(100, 100));
        state.set_arrow_style(style);
        state.set_tool(Tool::Arrow);
        state.begin_drag(Point { x: 42.0, y: 24.0 });
        state.update_drag(Point { x: 42.0, y: 24.0 });

        assert!(
            state.finalize_drag_action().is_none(),
            "expected click-only arrow finalize to return None for style {style:?}"
        );
    }
}

#[test]
fn arrow_interaction_cleanup_clears_stale_drag_state_without_selection() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 12.0, y: 18.0 });
    state.update_drag(Point { x: 12.0, y: 18.0 });
    state.arrow_editing_controls = true;
    state.arrow_control_dragging = Some(1);

    state.finalize_arrow_interaction_cleanup();

    assert!(state.drag_start.is_none());
    assert!(state.drag_current.is_none());
    assert!(state.drag_start_view.is_none());
    assert!(state.select_drag_anchor.is_none());
    assert!(state.arrow_control_dragging.is_none());
    assert!(!state.arrow_editing_controls);
}

#[test]
fn arrow_control_points_initialized_for_curved_style() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_arrow_style(ArrowStyle::Curved);
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 10.0, y: 10.0 });
    state.update_drag(Point { x: 90.0, y: 90.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Arrow {
            style,
            control_points,
            start,
            end,
            ..
        } => {
            assert_eq!(style, ArrowStyle::Curved);
            let pts = control_points.expect("control_points should be set");
            assert_eq!(pts.len(), 3);
            assert_eq!(pts[0], start);
            assert_eq!(pts[2], end);
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn arrow_control_points_initialized_for_double_style() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_arrow_style(ArrowStyle::Double);
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 20.0, y: 20.0 });
    state.update_drag(Point { x: 80.0, y: 80.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Arrow {
            style,
            control_points,
            ..
        } => {
            assert_eq!(style, ArrowStyle::Double);
            let pts = control_points.expect("control_points should be set for Double arrow");
            assert_eq!(pts.len(), 3);
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn arrow_control_points_initialized_for_standard_style() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_arrow_style(ArrowStyle::Standard);
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 10.0, y: 50.0 });
    state.update_drag(Point { x: 90.0, y: 50.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Arrow {
            style,
            control_points,
            ..
        } => {
            assert_eq!(style, ArrowStyle::Standard);
            let pts = control_points.expect("control_points should be set for Standard arrow");
            assert_eq!(pts.len(), 2);
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn curved_arrow_control_handle_movement() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 10.0, y: 10.0 },
        end: Point { x: 90.0, y: 90.0 },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        style: ArrowStyle::Curved,
        control_points: Some(vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 90.0, y: 90.0 },
        ]),
        shadow: false,
    });
    state.selected_action_index = Some(0);

    // Move the start handle (index 0)
    state.move_arrow_control_handle(0, Point { x: 5.0, y: 15.0 });
    match &state.actions[0] {
        AnnotationAction::Arrow {
            start,
            control_points,
            ..
        } => {
            assert_eq!(*start, Point { x: 5.0, y: 15.0 });
            let pts = control_points.as_ref().unwrap();
            assert_eq!(pts[0], Point { x: 5.0, y: 15.0 });
        }
        other => panic!("unexpected action: {:?}", other),
    }

    // Move the end handle (index 2)
    state.move_arrow_control_handle(2, Point { x: 95.0, y: 85.0 });
    match &state.actions[0] {
        AnnotationAction::Arrow {
            end,
            control_points,
            ..
        } => {
            assert_eq!(*end, Point { x: 95.0, y: 85.0 });
            let pts = control_points.as_ref().unwrap();
            assert_eq!(pts[2], Point { x: 95.0, y: 85.0 });
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn curved_arrow_endpoint_move_keeps_middle_control_point_inside_image() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 10.0, y: 60.0 },
        end: Point { x: 80.0, y: 60.0 },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        style: ArrowStyle::Curved,
        control_points: Some(vec![
            Point { x: 10.0, y: 60.0 },
            Point { x: 120.0, y: 20.0 },
            Point { x: 80.0, y: 60.0 },
        ]),
        shadow: false,
    });
    state.selected_action_index = Some(0);

    state.move_arrow_control_handle(2, Point { x: 95.0, y: 60.0 });

    match &state.actions[0] {
        AnnotationAction::Arrow {
            end,
            control_points,
            ..
        } => {
            assert_eq!(*end, Point { x: 95.0, y: 60.0 });
            let pts = control_points.as_ref().unwrap();
            assert_eq!(pts[2], Point { x: 95.0, y: 60.0 });
            assert!((0.0..=100.0).contains(&pts[1].x));
            assert!((0.0..=100.0).contains(&pts[1].y));
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn double_arrow_control_handle_movement() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_arrow_style(ArrowStyle::Double);
    state.set_tool(Tool::Arrow);
    state.begin_drag(Point { x: 10.0, y: 10.0 });
    state.update_drag(Point { x: 90.0, y: 90.0 });

    let action = state.finalize_drag_action().unwrap();
    match &action {
        AnnotationAction::Arrow {
            style,
            control_points,
            ..
        } => {
            assert_eq!(*style, ArrowStyle::Double);
            let pts = control_points
                .as_ref()
                .expect("control_points should be initialized");
            assert_eq!(pts.len(), 3);
            assert_eq!(pts[0], Point { x: 10.0, y: 10.0 });
            assert_eq!(pts[1], Point { x: 50.0, y: 50.0 });
            assert_eq!(pts[2], Point { x: 90.0, y: 90.0 });
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }
    state.push_action(action);

    // Move the start handle (index 0) — affects the tail head
    state.move_arrow_control_handle(0, Point { x: 5.0, y: 15.0 });
    match &state.actions[0] {
        AnnotationAction::Arrow {
            start,
            end,
            control_points,
            ..
        } => {
            assert_eq!(*start, Point { x: 5.0, y: 15.0 });
            let pts = control_points.as_ref().unwrap();
            assert_eq!(pts[0], Point { x: 5.0, y: 15.0 });
            assert_eq!(pts[2], Point { x: 90.0, y: 90.0 });
            assert_eq!(*end, Point { x: 90.0, y: 90.0 });
        }
        other => panic!("unexpected action: {:?}", other),
    }

    // Move the mid handle (index 1) — adjusts the on-curve midpoint via Bézier inversion
    state.move_arrow_control_handle(1, Point { x: 50.0, y: 25.0 });
    match &state.actions[0] {
        AnnotationAction::Arrow { control_points, .. } => {
            let pts = control_points.as_ref().unwrap();
            // P1 = 2*B(0.5) - 0.5*P0 - 0.5*P2
            //    = 2*(50,25) - 0.5*(5,15) - 0.5*(90,90)
            //    = (100,50) - (2.5,7.5) - (45,45)
            //    = (52.5, -2.5) → clamped to (52.5, 0.0)
            assert!((pts[1].x - 52.5).abs() < 0.01);
            assert!((pts[1].y - 0.0).abs() < 0.01);
        }
        other => panic!("unexpected action: {:?}", other),
    }

    // Move the end handle (index 2) — affects the arrow head
    state.move_arrow_control_handle(2, Point { x: 95.0, y: 85.0 });
    match &state.actions[0] {
        AnnotationAction::Arrow {
            end,
            control_points,
            ..
        } => {
            assert_eq!(*end, Point { x: 95.0, y: 85.0 });
            let pts = control_points.as_ref().unwrap();
            assert_eq!(pts[2], Point { x: 95.0, y: 85.0 });
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn arrow_hit_testing_on_head_and_body_regions() {
    let mut state = EditorState::new(RgbaImage::new(200, 200));

    // Fancy arrow with larger stroke
    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 20.0, y: 100.0 },
        end: Point { x: 180.0, y: 100.0 },
        color: DRAW_COLORS[0],
        stroke_size: 8.0,
        style: ArrowStyle::Fancy,
        control_points: None,
        shadow: false,
    });

    // Click near the arrow head tip (should be selectable)
    assert!(state.select_action_at_point_with_scale(Point { x: 175.0, y: 100.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));

    // Click on the widened body (offset from centerline)
    state.selected_action_index = None;
    assert!(state.select_action_at_point_with_scale(Point { x: 100.0, y: 108.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));
}

#[test]
fn curved_arrow_hit_testing_on_head_and_body() {
    let mut state = EditorState::new(RgbaImage::new(200, 200));

    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 20.0, y: 100.0 },
        end: Point { x: 180.0, y: 50.0 },
        color: DRAW_COLORS[1],
        stroke_size: 6.0,
        style: ArrowStyle::Curved,
        control_points: Some(vec![
            Point { x: 20.0, y: 100.0 },
            Point { x: 100.0, y: 30.0 },
            Point { x: 180.0, y: 50.0 },
        ]),
        shadow: false,
    });

    // Click near the arrow head
    assert!(state.select_action_at_point_with_scale(Point { x: 175.0, y: 52.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));

    // Click on the body of the curved arrow (offset from centerline)
    state.selected_action_index = None;
    assert!(state.select_action_at_point_with_scale(Point { x: 100.0, y: 40.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));
}

#[test]
fn double_arrow_hit_testing_on_both_heads_and_body() {
    let mut state = EditorState::new(RgbaImage::new(200, 200));

    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 20.0, y: 100.0 },
        end: Point { x: 180.0, y: 100.0 },
        color: DRAW_COLORS[2],
        stroke_size: 6.0,
        style: ArrowStyle::Double,
        control_points: None,
        shadow: false,
    });

    // Click near the end head
    assert!(state.select_action_at_point_with_scale(Point { x: 175.0, y: 100.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));

    // Click near the start head
    state.selected_action_index = None;
    assert!(state.select_action_at_point_with_scale(Point { x: 25.0, y: 100.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));

    // Click on the body between the two heads (offset from centerline)
    state.selected_action_index = None;
    assert!(state.select_action_at_point_with_scale(Point { x: 100.0, y: 108.0 }, 1.0));
    assert_eq!(state.selected_action_index, Some(0));
}

#[test]
fn arrow_tool_switch_clears_editing_controls() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 10.0, y: 10.0 },
        end: Point { x: 90.0, y: 90.0 },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        style: ArrowStyle::Curved,
        control_points: Some(vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 90.0, y: 90.0 },
        ]),
        shadow: false,
    });
    state.selected_action_index = Some(0);
    state.arrow_editing_controls = true;

    // Switching away from Arrow tool should finalize editing
    state.set_tool(Tool::Pen);
    assert!(!state.arrow_editing_controls);
    assert!(state.arrow_control_dragging.is_none());
}

#[test]
fn arrow_interaction_cleanup_preserves_controls_for_selected_arrow() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 10.0, y: 10.0 },
        end: Point { x: 90.0, y: 90.0 },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        style: ArrowStyle::Curved,
        control_points: Some(vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 90.0, y: 90.0 },
        ]),
        shadow: false,
    });
    state.selected_action_index = Some(0);
    state.arrow_editing_controls = true;
    state.arrow_control_dragging = Some(1);

    state.finalize_arrow_interaction_cleanup();

    assert!(state.arrow_control_dragging.is_none());
    assert!(state.arrow_editing_controls);
}

#[test]
fn arrow_finalization_preserves_style_and_control_points() {
    let mut state = EditorState::new(RgbaImage::new(100, 100));
    state.set_tool(Tool::Arrow);
    state.set_arrow_style(ArrowStyle::Double);
    state.begin_drag(Point { x: 10.0, y: 10.0 });
    state.update_drag(Point { x: 90.0, y: 90.0 });

    let action = state.finalize_drag_action().unwrap();
    match action {
        AnnotationAction::Arrow {
            style,
            control_points,
            ..
        } => {
            assert_eq!(style, ArrowStyle::Double);
            // Double arrows have control_points initialized by finalize_drag_action
            let pts = control_points.expect("control_points should be set for Double arrow");
            assert_eq!(pts.len(), 3);
        }
        other => panic!("unexpected finalized action: {:?}", other),
    }
}

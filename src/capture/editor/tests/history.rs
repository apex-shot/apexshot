use super::*;

#[test]
fn history_availability_reflects_undo_redo_stack_state() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));
    assert_eq!(state.history_availability(), (false, false));

    state.push_action(AnnotationAction::Line {
        start: Point { x: 2.0, y: 2.0 },
        end: Point { x: 10.0, y: 8.0 },
        color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });
    assert_eq!(state.history_availability(), (true, false));

    assert!(state.undo());
    assert_eq!(state.history_availability(), (false, true));

    assert!(state.redo());
    assert_eq!(state.history_availability(), (true, false));
}

#[test]
fn undo_redo_stack_behaves_correctly() {
    let image = RgbaImage::new(32, 32);
    let mut state = EditorState::new(image);

    state.push_action(AnnotationAction::Arrow {
        start: Point { x: 2.0, y: 2.0 },
        end: Point { x: 10.0, y: 10.0 },
        color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
        stroke_size: STROKE_WIDTH,
        style: ArrowStyle::Standard,
        control_points: None,
        shadow: false,
    });
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 4,
            y: 4,
            width: 8,
            height: 8,
        },
        color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    assert_eq!(state.actions.len(), 2);
    assert!(state.undo());
    assert_eq!(state.actions.len(), 1);
    assert_eq!(state.redo_actions.len(), 1);
    assert!(state.redo());
    assert_eq!(state.actions.len(), 2);
    assert_eq!(state.redo_actions.len(), 0);
}

#[test]
fn static_transform_changes_participate_in_undo_and_redo() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));
    let transform = crate::capture::editor::image_transform::StaticImageTransform {
        yaw: 14.0,
        pos_x: 0.2,
        ..Default::default()
    };

    assert!(state.set_static_image_transform(transform));
    assert_eq!(state.static_image_transform, transform);
    assert!(state.undo());
    assert_eq!(
        state.static_image_transform,
        crate::capture::editor::image_transform::StaticImageTransform::default()
    );
    assert!(state.redo());
    assert_eq!(state.static_image_transform, transform);
}

#[test]
fn resetting_static_transform_is_undoable() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));
    let transform = crate::capture::editor::image_transform::StaticImageTransform {
        pitch: 10.0,
        pos_y: -0.3,
        perspective: 0.5,
        ..Default::default()
    };
    state.set_static_image_transform(transform);
    state.set_static_image_transform(Default::default());
    assert_eq!(
        state.static_image_transform,
        crate::capture::editor::image_transform::StaticImageTransform::default()
    );
    assert!(state.undo());
    assert_eq!(state.static_image_transform, transform);
}

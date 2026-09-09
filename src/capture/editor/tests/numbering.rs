use super::*;

#[test]
fn remove_selected_action_keeps_number_sequence_consistent() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.add_number_marker(Point { x: 8.0, y: 8.0 });
    state.add_number_marker(Point { x: 52.0, y: 52.0 });

    assert!(state.select_action_at_point_with_scale(Point { x: 8.0, y: 8.0 }, 1.0));
    assert!(state.remove_selected_action());

    let numbers: Vec<u32> = state
        .actions
        .iter()
        .filter_map(|action| match action {
            AnnotationAction::Number { number, .. } => Some(*number),
            _ => None,
        })
        .collect();

    assert_eq!(numbers, vec![2]);
    assert_eq!(state.next_number, 1); // reuses the removed number slot
}

#[test]
fn add_number_marker_assigns_incrementing_numbers() {
    let mut state = EditorState::new(RgbaImage::new(200, 200));
    state.set_color_index(4);

    state.add_number_marker(Point { x: 20.0, y: 20.0 });
    state.add_number_marker(Point { x: 40.0, y: 30.0 });

    assert_eq!(state.next_number, 3);
    assert_eq!(state.actions.len(), 2);

    match &state.actions[0] {
        AnnotationAction::Number {
            position,
            number,
            color,
            ..
        } => {
            assert_eq!(*position, Point { x: 20.0, y: 20.0 });
            assert_eq!(*number, 1);
            assert_eq!(*color, DRAW_COLORS[4]);
        }
        other => panic!("unexpected action: {:?}", other),
    }

    match &state.actions[1] {
        AnnotationAction::Number {
            position, number, ..
        } => {
            assert_eq!(*position, Point { x: 40.0, y: 30.0 });
            assert_eq!(*number, 2);
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

#[test]
fn undo_number_marker_reuses_number_slot() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));

    state.add_number_marker(Point { x: 3.0, y: 3.0 });
    state.add_number_marker(Point { x: 9.0, y: 9.0 });
    assert!(state.undo());

    state.add_number_marker(Point { x: 12.0, y: 12.0 });

    let numbers: Vec<u32> = state
        .actions
        .iter()
        .filter_map(|action| match action {
            AnnotationAction::Number { number, .. } => Some(*number),
            _ => None,
        })
        .collect();

    assert_eq!(numbers, vec![1, 2]);
    assert_eq!(state.next_number, 3);
}

#[test]
fn add_number_marker_clamps_center_inside_image_bounds() {
    let mut state = EditorState::new(RgbaImage::new(40, 40));

    state.add_number_marker(Point { x: 39.0, y: 39.0 });

    match &state.actions[0] {
        AnnotationAction::Number { position, .. } => {
            assert_eq!(*position, Point { x: 25.0, y: 25.0 });
        }
        other => panic!("unexpected action: {:?}", other),
    }
}

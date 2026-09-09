use super::*;

#[test]
fn set_text_size_clamps_to_allowed_range() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));
    assert!(state.set_text_size(2.0));
    assert_eq!(state.text_size, MIN_TEXT_SIZE);

    assert!(state.set_text_size(500.0));
    assert_eq!(state.text_size, MAX_TEXT_SIZE);
}

#[test]
fn set_stroke_size_clamps_to_allowed_range() {
    let mut state = EditorState::new(RgbaImage::new(32, 32));
    assert!(state.set_stroke_size(0.1));
    assert_eq!(state.stroke_size, MIN_STROKE_SIZE);

    assert!(state.set_stroke_size(500.0));
    assert_eq!(state.stroke_size, MAX_STROKE_SIZE);
}

#[test]
fn set_selected_action_stroke_size_updates_selected_annotation() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Line {
        start: Point { x: 6.0, y: 8.0 },
        end: Point { x: 20.0, y: 24.0 },
        color: DRAW_COLORS[0],
        stroke_size: 4.0,
        shadow: false,
    });

    state.selected_action_index = Some(0);
    assert_eq!(state.selected_action_stroke_size(), Some(4.0));
    assert!(state.set_selected_action_stroke_size(9.0));
    assert_eq!(state.selected_action_stroke_size(), Some(9.0));
}

#[test]
fn adjust_stroke_size_updates_selected_annotation_size() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 10,
            y: 10,
            width: 16,
            height: 16,
        },
        color: DRAW_COLORS[2],
        stroke_size: 6.0,
        shadow: false,
    });

    state.selected_action_index = Some(0);
    state.set_stroke_size(6.0);
    assert!(state.set_selected_action_stroke_size(7.0));
    assert_eq!(state.selected_action_stroke_size(), Some(7.0));
}

#[test]
fn set_selected_text_action_size_updates_selected_text_annotation() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Text {
        position: Point { x: 12.0, y: 16.0 },
        text: "text".to_string(),
        color: DRAW_COLORS[0],
        font: FontSettings {
            family: "Sans".to_string(),
            size: 20.0,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        },
        max_width: None,
        shadow: false,
        background_color: None,
    });

    state.selected_action_index = Some(0);
    assert_eq!(state.selected_text_action_size(), Some(20.0));
    assert!(state.set_selected_text_action_size(34.0));
    assert_eq!(state.selected_text_action_size(), Some(34.0));
}

#[test]
fn adjust_text_size_updates_selected_text_annotation_size() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Text {
        position: Point { x: 12.0, y: 16.0 },
        text: "text".to_string(),
        color: DRAW_COLORS[0],
        font: FontSettings {
            family: "Sans".to_string(),
            size: 20.0,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        },
        max_width: None,
        shadow: false,
        background_color: None,
    });

    state.selected_action_index = Some(0);
    state.set_text_size(20.0);
    assert!(state.set_selected_text_action_size(22.0));
    assert_eq!(state.selected_text_action_size(), Some(22.0));
}

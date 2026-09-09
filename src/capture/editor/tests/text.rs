use super::*;

#[test]
fn update_text_action_replaces_selected_text_content() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Text {
        position: Point { x: 18.0, y: 22.0 },
        text: "old text".to_string(),
        color: DRAW_COLORS[0],
        font: FontSettings {
            family: "Sans".to_string(),
            size: TEXT_SIZE,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        },
        max_width: None,
        shadow: false,
        background_color: None,
    });
    state.selected_action_index = Some(0);

    assert!(state.update_text_action(0, "new text".to_string()));
    assert_eq!(
        state.selected_text_action_data(),
        Some((
            0,
            "new text".to_string(),
            DRAW_COLORS[0],
            FontSettings {
                family: "Sans".to_string(),
                size: TEXT_SIZE,
                style: FontStyle::Normal,
                decoration: TextDecoration::None,
                alignment: TextAlignment::Left,
            },
            None,
            Point { x: 18.0, y: 22.0 },
            None,
        ))
    );
}

#[test]
fn update_text_action_with_empty_text_removes_annotation() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Text {
        position: Point { x: 20.0, y: 24.0 },
        text: "temporary".to_string(),
        color: DRAW_COLORS[1],
        font: FontSettings {
            family: "Sans".to_string(),
            size: TEXT_SIZE,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        },
        max_width: None,
        shadow: false,
        background_color: None,
    });
    state.selected_action_index = Some(0);

    assert!(state.update_text_action(0, "   ".to_string()));
    assert!(state.actions.is_empty());
    assert!(state.selected_action_index.is_none());
}

#[test]
fn text_input_preserves_selected_text_size_while_typing() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.set_text_size(48.0);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.add_text_input_char('H');
    state.fit_active_text_to_layout();

    assert_eq!(state.text_size, 48.0);
}

#[test]
fn text_input_shrinks_to_stay_within_bottom_image_boundary() {
    let mut state = EditorState::new(RgbaImage::new(140, 92));
    state.set_tool(Tool::Text);
    state.set_text_size(30.0);
    state.begin_text_input(Point { x: 12.0, y: 78.0 }, 64.0, 44.0);
    for ch in "this text should shrink instead of overflowing below the image".chars() {
        state.add_text_input_char(ch);
    }

    state.fit_active_text_to_layout();

    let bounds = state.get_text_bounds().expect("active text bounds");
    assert!(bounds.rect.y + bounds.rect.height <= state.base_image.height() as i32);
    assert!(
        state.text_size < 30.0,
        "expected font size to shrink when bottom boundary is reached, got {}",
        state.text_size
    );
}

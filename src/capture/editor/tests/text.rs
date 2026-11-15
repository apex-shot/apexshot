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

#[test]
fn insert_text_input_str_inserts_multibyte_at_scalar_caret() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("ab");
    state.set_text_cursor_position(1);

    state.insert_text_input_str("Tiếng");

    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "aTiếngb");
    assert_eq!(input.cursor_position, 6);
}

#[test]
fn insert_text_input_str_handles_emoji_and_combining_marks() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("x");
    state.set_text_cursor_position(0);

    state.insert_text_input_str("e\u{0301}😀");

    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "e\u{0301}\u{1F600}x");
    assert_eq!(input.cursor_position, 3);
}

#[test]
fn insert_text_input_str_empty_or_inactive_is_a_no_op() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.insert_text_input_str("nope");
    assert!(state.get_text_input().is_none());

    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("");
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "");
    assert_eq!(input.cursor_position, 0);
}

#[test]
fn normalize_pasted_line_endings_unifies_crlf_and_cr() {
    use super::super::state::normalize_pasted_line_endings;
    assert_eq!(normalize_pasted_line_endings("a\r\nb\rc\nd"), "a\nb\nc\nd");
    assert_eq!(
        normalize_pasted_line_endings("Tiếng Việt có dấu"),
        "Tiếng Việt có dấu"
    );
}

#[test]
fn delete_surrounding_replaces_vietnamese_letters_before_caret() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("Tiếeng");

    assert!(state.delete_surrounding_text_input(-3, 3));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "Tiế");
    assert_eq!(input.cursor_position, 3);

    state.insert_text_input_str("ng Việt có dấu");
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "Tiếng Việt có dấu");
}

#[test]
fn delete_surrounding_forward_range_and_caret_adjustment() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("abcdef");
    state.set_text_cursor_position(2);

    assert!(state.delete_surrounding_text_input(1, 2));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "abcf");
    assert_eq!(input.cursor_position, 2);

    state.set_text_cursor_position(4);
    assert!(state.delete_surrounding_text_input(-4, 2));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "cf");
    assert_eq!(input.cursor_position, 2);
}

#[test]
fn delete_surrounding_adjusts_caret_inside_deleted_range() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("abcdef");
    state.set_text_cursor_position(4);

    assert!(state.delete_surrounding_text_input(-3, 4));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "af");
    assert_eq!(input.cursor_position, 1);
}

#[test]
fn insert_text_input_str_filters_control_chars_except_newline() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("aé");
    state.set_text_cursor_position(1);

    state.insert_text_input_str("x\n\t\r\u{0}😀\u{0301}");

    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "ax\ny😀\u{0301}é");
    assert_eq!(input.cursor_position, 1 + "x\ny😀\u{0301}".chars().count());
}

#[test]
fn insert_text_input_str_control_only_is_a_no_op() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);

    state.insert_text_input_str("\t\r\u{0}\u{7}");

    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "");
    assert_eq!(input.cursor_position, 0);
}

#[test]
fn delete_surrounding_multibyte_scalar_range() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("é\u{0301}😀xy");
    state.set_text_cursor_position(5);

    assert!(state.delete_surrounding_text_input(-4, 4));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "é");
    assert_eq!(input.cursor_position, 1);
}

#[test]
fn delete_surrounding_rejects_out_of_bounds_ranges() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("abc");
    state.set_text_cursor_position(2);

    assert!(!state.delete_surrounding_text_input(-10, 5));
    assert!(!state.delete_surrounding_text_input(0, 99));
    assert!(!state.delete_surrounding_text_input(5, 2));
    assert!(!state.delete_surrounding_text_input(-1, -1));
    assert!(!state.delete_surrounding_text_input(0, -3));
    assert!(!state.delete_surrounding_text_input(i32::MIN, 1));
    assert!(!state.delete_surrounding_text_input(10, 0));
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "abc");
    assert_eq!(input.cursor_position, 2);

    assert!(state.delete_surrounding_text_input(0, 0));
}

#[test]
fn forward_delete_and_line_home_end_are_char_safe() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    state.insert_text_input_str("é😀\nxy");
    state.set_text_cursor_position(1);

    state.delete_text_input_char_forward();
    let input = state.get_text_input().expect("active text input");
    assert_eq!(input.text, "é\nxy");
    assert_eq!(input.cursor_position, 1);

    state.set_text_cursor_position(3);
    state.move_cursor_to_line_start();
    assert_eq!(state.get_text_input().unwrap().cursor_position, 2);
    state.move_cursor_to_line_end();
    assert_eq!(state.get_text_input().unwrap().cursor_position, 4);
    state.set_text_cursor_position(1);
    state.move_cursor_to_line_start();
    assert_eq!(state.get_text_input().unwrap().cursor_position, 0);
    state.move_cursor_to_line_end();
    assert_eq!(state.get_text_input().unwrap().cursor_position, 1);
}

#[test]
fn each_edit_session_gets_a_fresh_text_input_session_id() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    let first = state.text_input_session_id();
    state.insert_text_input_str("hello");
    state.cancel_text_input();
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    let second = state.text_input_session_id();
    assert_ne!(first, second);

    state.cancel_text_input();
    state.push_action(AnnotationAction::Text {
        position: Point { x: 18.0, y: 40.0 },
        text: "existing".to_string(),
        color: DRAW_COLORS[0],
        font: FontSettings {
            family: "Sans".to_string(),
            size: TEXT_SIZE,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        },
        max_width: Some(120.0),
        shadow: false,
        background_color: None,
    });
    state.selected_action_index = Some(0);
    assert!(state.begin_editing_selected_text());
    let third = state.text_input_session_id();
    assert_ne!(third, second);
}

#[test]
fn text_input_surrounding_reports_utf8_byte_caret() {
    let mut state = EditorState::new(RgbaImage::new(400, 300));
    state.set_tool(Tool::Text);
    state.begin_text_input(Point { x: 20.0, y: 80.0 }, 160.0, 60.0);
    assert_eq!(state.text_input_surrounding(), Some((String::new(), 0)));

    state.insert_text_input_str("Tiếng");
    state.set_text_cursor_position(3);
    let (text, caret_byte) = state.text_input_surrounding().expect("surrounding");
    assert_eq!(text, "Tiếng");
    assert_eq!(&text[..caret_byte as usize], "Tiế");
    assert_eq!(caret_byte, "Tiế".len() as i32);
}

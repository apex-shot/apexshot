use super::*;

#[test]
fn tool_shortcuts_map_to_expected_tools() {
    // Indices must match the tool_buttons vector in window/mod.rs
    // (Crop, Background, Select, Pen, Box, Circle, Arrow, Line, Text,
    //  Obfuscate, Number, Highlighter, Focus).
    assert_eq!(tool_shortcut_target('0'), Some((Tool::Select, 2)));
    assert_eq!(tool_shortcut_target('P'), Some((Tool::Pen, 3)));
    assert_eq!(tool_shortcut_target('t'), Some((Tool::Text, 8)));
    assert_eq!(tool_shortcut_target('l'), Some((Tool::Line, 7)));
    assert_eq!(tool_shortcut_target('a'), Some((Tool::Arrow, 6)));
    assert_eq!(tool_shortcut_target('r'), Some((Tool::Box, 4)));
    assert_eq!(tool_shortcut_target('o'), Some((Tool::Circle, 5)));
    assert_eq!(tool_shortcut_target('h'), Some((Tool::Highlighter, 11)));
    assert_eq!(tool_shortcut_target('7'), Some((Tool::Highlighter, 11)));
    assert_eq!(tool_shortcut_target('c'), Some((Tool::Obfuscate, 9)));
    assert_eq!(tool_shortcut_target('n'), Some((Tool::Number, 10)));
    assert_eq!(tool_shortcut_target('x'), Some((Tool::Crop, 0)));
    assert_eq!(tool_shortcut_target('b'), Some((Tool::Obfuscate, 9)));
    assert_eq!(tool_shortcut_target('f'), Some((Tool::Focus, 12)));
    assert_eq!(tool_shortcut_target('q'), None);
}

#[test]
fn tool_button_index_matches_toolbar_vector_order() {
    assert_eq!(tool_button_index(Tool::Crop), 0);
    assert_eq!(tool_button_index(Tool::Background), 1);
    assert_eq!(tool_button_index(Tool::Select), 2);
    assert_eq!(tool_button_index(Tool::Pen), 3);
    assert_eq!(tool_button_index(Tool::Box), 4);
    assert_eq!(tool_button_index(Tool::Circle), 5);
    assert_eq!(tool_button_index(Tool::Arrow), 6);
    assert_eq!(tool_button_index(Tool::Line), 7);
    assert_eq!(tool_button_index(Tool::Text), 8);
    assert_eq!(tool_button_index(Tool::Obfuscate), 9);
    assert_eq!(tool_button_index(Tool::Number), 10);
    assert_eq!(tool_button_index(Tool::Highlighter), 11);
    assert_eq!(tool_button_index(Tool::Focus), 12);
}

#[test]
fn constrained_drag_endpoint_snaps_line_and_box_when_shift_is_pressed() {
    assert_eq!(
        constrained_drag_endpoint(
            Tool::Line,
            Point { x: 4.0, y: 4.0 },
            Point { x: 18.0, y: 7.0 },
            true,
        ),
        Point { x: 18.0, y: 4.0 }
    );

    assert_eq!(
        constrained_drag_endpoint(
            Tool::Box,
            Point { x: 10.0, y: 10.0 },
            Point { x: 14.0, y: 22.0 },
            true,
        ),
        Point { x: 22.0, y: 22.0 }
    );
}

#[test]
fn constrained_drag_endpoint_snaps_line_to_diagonal_when_shift_is_pressed() {
    assert_eq!(
        constrained_drag_endpoint(
            Tool::Line,
            Point { x: 4.0, y: 4.0 },
            Point { x: 18.0, y: 13.0 },
            true,
        ),
        Point { x: 18.0, y: 18.0 }
    );
}

#[test]
fn constrained_drag_endpoint_keeps_highlighter_horizontal() {
    assert_eq!(
        constrained_drag_endpoint(
            Tool::Highlighter,
            Point { x: 6.0, y: 18.0 },
            Point { x: 30.0, y: 31.0 },
            true,
        ),
        Point { x: 30.0, y: 18.0 }
    );
}

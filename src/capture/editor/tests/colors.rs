use super::*;

#[test]
fn palette_index_for_color_prefers_nearest_palette_entry() {
    assert_eq!(palette_index_for_color(DRAW_COLORS[0]), 0);
    assert_eq!(palette_index_for_color(DRAW_COLORS[11]), 11);

    let near_blue = DrawColor::new(0.21, 0.55, 0.99, 0.95);
    assert_eq!(palette_index_for_color(near_blue), 8);
}

#[test]
fn move_custom_color_between_slots_swaps_and_moves_entries() {
    let mut slots = vec![Some(DRAW_COLORS[1]), None, Some(DRAW_COLORS[3])];

    assert!(move_custom_color_between_slots(&mut slots, 0, 1));
    assert_eq!(slots[0], None);
    assert_eq!(slots[1], Some(DRAW_COLORS[1]));

    assert!(move_custom_color_between_slots(&mut slots, 1, 2));
    assert_eq!(slots[1], Some(DRAW_COLORS[3]));
    assert_eq!(slots[2], Some(DRAW_COLORS[1]));
}

#[test]
fn move_custom_color_between_slots_rejects_invalid_sources() {
    let mut slots = vec![None, Some(DRAW_COLORS[0])];

    assert!(!move_custom_color_between_slots(&mut slots, 0, 1));
    assert!(!move_custom_color_between_slots(&mut slots, 1, 1));
    assert!(!move_custom_color_between_slots(&mut slots, 5, 0));
}

#[test]
fn set_selected_action_color_updates_selected_annotation() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Box {
        rect: Rect {
            x: 10,
            y: 10,
            width: 16,
            height: 16,
        },
        color: DRAW_COLORS[0],
        stroke_size: STROKE_WIDTH,
        shadow: false,
    });

    state.selected_action_index = Some(0);
    assert_eq!(state.selected_action_color(), Some(DRAW_COLORS[0]));
    assert!(state.set_selected_action_color(DRAW_COLORS[3]));
    assert_eq!(state.selected_action_color(), Some(DRAW_COLORS[3]));
}

#[test]
fn set_selected_action_color_ignores_non_color_annotations() {
    let mut state = EditorState::new(RgbaImage::new(64, 64));
    state.push_action(AnnotationAction::Obfuscate {
        rect: Rect {
            x: 10,
            y: 10,
            width: 18,
            height: 18,
        },
        method: ObfuscateMethod::Blur,
        amount: DEFAULT_OBFUSCATE_AMOUNT,
    });

    state.selected_action_index = Some(0);
    assert_eq!(state.selected_action_color(), None);
    assert!(!state.set_selected_action_color(DRAW_COLORS[2]));
}

mod color;
mod io_ops;
mod render;
mod selection;
mod state;
mod types;
mod ui_support;
mod window;

pub use types::EditorError;
pub use window::open_image_editor;

pub fn copy_file_uri_to_clipboard(path: &std::path::Path) -> Result<(), String> {
    io_ops::copy_uri_to_clipboard(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use color::*;
    use image::RgbaImage;
    use selection::*;
    use state::EditorState;
    use types::*;
    use window::cursor_name_for_view_point;

    #[test]
    fn tool_shortcuts_map_to_expected_tools() {
        assert_eq!(tool_shortcut_target('0'), Some((Tool::Select, 0)));
        assert_eq!(tool_shortcut_target('P'), Some((Tool::Pen, 2)));
        assert_eq!(tool_shortcut_target('t'), Some((Tool::Text, 7)));
        assert_eq!(tool_shortcut_target('l'), Some((Tool::Line, 6)));
        assert_eq!(tool_shortcut_target('a'), Some((Tool::Arrow, 5)));
        assert_eq!(tool_shortcut_target('r'), Some((Tool::Box, 3)));
        assert_eq!(tool_shortcut_target('o'), Some((Tool::Circle, 4)));
        assert_eq!(tool_shortcut_target('h'), Some((Tool::Highlighter, 11)));
        assert_eq!(tool_shortcut_target('c'), Some((Tool::Censor, 9)));
        assert_eq!(tool_shortcut_target('n'), Some((Tool::Number, 10)));
        assert_eq!(tool_shortcut_target('x'), Some((Tool::Crop, 1)));
        assert_eq!(tool_shortcut_target('b'), Some((Tool::Blur, 8)));
        assert_eq!(tool_shortcut_target('f'), Some((Tool::Focus, 12)));
        assert_eq!(tool_shortcut_target('q'), None);
    }

    #[test]
    fn constrained_drag_endpoint_snaps_line_and_box_when_shift_is_pressed() {
        assert_eq!(
            constrained_drag_endpoint(
                Tool::Line,
                Point { x: 4.0, y: 4.0 },
                Point { x: 18.0, y: 9.0 },
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

    #[test]
    fn select_handle_cursor_mapping_matches_expected_names() {
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::TopLeft),
            "nw-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::Top),
            "ns-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::TopRight),
            "ne-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::Left),
            "ew-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::Right),
            "ew-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::BottomLeft),
            "sw-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::Bottom),
            "ns-resize"
        );
        assert_eq!(
            cursor_name_for_select_handle(SelectHandle::BottomRight),
            "se-resize"
        );
        assert_eq!(cursor_name_for_select_handle(SelectHandle::Start), "move");
        assert_eq!(cursor_name_for_select_handle(SelectHandle::End), "move");
    }

    #[test]
    fn cursor_name_for_view_point_uses_select_handle_and_grab_states() {
        let mut state = EditorState::new(RgbaImage::new(80, 80));
        state.set_tool(Tool::Select);
        state.push_action(AnnotationAction::Box {
            rect: Rect {
                x: 20,
                y: 16,
                width: 24,
                height: 18,
            },
            color: DRAW_COLORS[1],
            stroke_size: STROKE_WIDTH,
        });
        state.selected_action_index = Some(0);

        let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 20.0, y: 16.0 }),
            "nw-resize"
        );
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 28.0, y: 22.0 }),
            "grab"
        );

        state.select_drag_anchor = Some(Point { x: 20.0, y: 16.0 });
        state.select_resize_handle = Some(SelectHandle::Bottom);
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 30.0 }),
            "ns-resize"
        );

        state.select_resize_handle = None;
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 30.0 }),
            "grabbing"
        );
    }

    #[test]
    fn cursor_name_for_view_point_matches_text_and_crosshair_modes() {
        let mut state = EditorState::new(RgbaImage::new(80, 80));
        let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);

        state.set_tool(Tool::Text);
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 12.0, y: 12.0 }),
            "text"
        );

        state.set_tool(Tool::Arrow);
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 12.0, y: 12.0 }),
            "crosshair"
        );
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: -4.0, y: -4.0 }),
            "default"
        );
    }

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
        });

        state.selected_action_index = Some(0);
        assert_eq!(state.selected_action_color(), Some(DRAW_COLORS[0]));
        assert!(state.set_selected_action_color(DRAW_COLORS[3]));
        assert_eq!(state.selected_action_color(), Some(DRAW_COLORS[3]));
    }

    #[test]
    fn set_selected_action_color_ignores_non_color_annotations() {
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        state.push_action(AnnotationAction::Blur {
            rect: Rect {
                x: 10,
                y: 10,
                width: 18,
                height: 18,
            },
        });

        state.selected_action_index = Some(0);
        assert_eq!(state.selected_action_color(), None);
        assert!(!state.set_selected_action_color(DRAW_COLORS[2]));
    }

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
        });

        state.selected_action_index = Some(0);
        state.set_stroke_size(6.0);
        assert!(state.adjust_stroke_size(STROKE_SIZE_STEP));
        assert_eq!(state.stroke_size, 7.0);
        assert_eq!(state.selected_action_stroke_size(), Some(7.0));
    }

    #[test]
    fn set_selected_text_action_size_updates_selected_text_annotation() {
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        state.push_action(AnnotationAction::Text {
            position: Point { x: 12.0, y: 16.0 },
            text: "text".to_string(),
            color: DRAW_COLORS[0],
            font_size: 20.0,
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
            font_size: 20.0,
        });

        state.selected_action_index = Some(0);
        state.set_text_size(20.0);
        assert!(state.adjust_text_size(TEXT_SIZE_STEP));
        assert_eq!(state.text_size, 22.0);
        assert_eq!(state.selected_text_action_size(), Some(22.0));
    }

    #[test]
    fn history_availability_reflects_undo_redo_stack_state() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        assert_eq!(state.history_availability(), (false, false));

        state.push_action(AnnotationAction::Line {
            start: Point { x: 2.0, y: 2.0 },
            end: Point { x: 10.0, y: 8.0 },
            color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
        });
        assert_eq!(state.history_availability(), (true, false));

        assert!(state.undo());
        assert_eq!(state.history_availability(), (false, true));

        assert!(state.redo());
        assert_eq!(state.history_availability(), (true, false));
    }

    #[test]
    fn can_remove_selected_action_requires_valid_selection() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.push_action(AnnotationAction::Box {
            rect: Rect {
                x: 4,
                y: 4,
                width: 10,
                height: 10,
            },
            color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
        });

        assert!(!state.can_remove_selected_action());

        state.selected_action_index = Some(0);
        assert!(state.can_remove_selected_action());

        state.selected_action_index = Some(9);
        assert!(!state.can_remove_selected_action());
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
        });

        assert!(state.begin_select_drag_with_scale(Point { x: 8.0, y: 10.0 }, 1.0));
        assert!(state.update_select_drag(Point { x: 15.0, y: 19.0 }));
        state.end_select_drag();

        match &state.actions[0] {
            AnnotationAction::Line { start, end, .. } => {
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
        assert_eq!(state.next_number, 3);
    }

    #[test]
    fn update_text_action_replaces_selected_text_content() {
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        state.push_action(AnnotationAction::Text {
            position: Point { x: 18.0, y: 22.0 },
            text: "old text".to_string(),
            color: DRAW_COLORS[0],
            font_size: TEXT_SIZE,
        });
        state.selected_action_index = Some(0);

        assert!(state.update_text_action(0, "new text".to_string()));
        assert_eq!(
            state.selected_text_action_data(),
            Some((0, "new text".to_string()))
        );
    }

    #[test]
    fn update_text_action_with_empty_text_removes_annotation() {
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        state.push_action(AnnotationAction::Text {
            position: Point { x: 20.0, y: 24.0 },
            text: "temporary".to_string(),
            color: DRAW_COLORS[1],
            font_size: TEXT_SIZE,
        });
        state.selected_action_index = Some(0);

        assert!(state.update_text_action(0, "   ".to_string()));
        assert!(state.actions.is_empty());
        assert!(state.selected_action_index.is_none());
    }

    #[test]
    fn add_number_marker_assigns_incrementing_numbers() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.set_color_index(4);

        state.add_number_marker(Point { x: 6.0, y: 8.0 });
        state.add_number_marker(Point { x: 14.0, y: 10.0 });

        assert_eq!(state.next_number, 3);
        assert_eq!(state.actions.len(), 2);

        match &state.actions[0] {
            AnnotationAction::Number {
                position,
                number,
                color,
            } => {
                assert_eq!(*position, Point { x: 6.0, y: 8.0 });
                assert_eq!(*number, 1);
                assert_eq!(*color, DRAW_COLORS[4]);
            }
            other => panic!("unexpected action: {:?}", other),
        }

        match &state.actions[1] {
            AnnotationAction::Number {
                position, number, ..
            } => {
                assert_eq!(*position, Point { x: 14.0, y: 10.0 });
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
    fn blur_changes_pixels_inside_rect_only() {
        let mut image = RgbaImage::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let value = if x < 5 { 0 } else { 255 };
                image.put_pixel(x, y, image::Rgba([value, value, value, 255]));
            }
        }

        let rect = Rect {
            x: 2,
            y: 2,
            width: 6,
            height: 6,
        };
        let before_outside = *image.get_pixel(0, 0);

        render::apply_blur_rect(&mut image, rect, 2);

        let inside = *image.get_pixel(4, 4);
        let outside = *image.get_pixel(0, 0);

        assert_ne!(inside[0], 0);
        assert_eq!(outside, before_outside);
    }

    #[test]
    fn censor_pixelates_pixels_inside_rect_only() {
        let mut image = RgbaImage::new(12, 12);
        for y in 0..12 {
            for x in 0..12 {
                image.put_pixel(
                    x,
                    y,
                    image::Rgba([(x * 7) as u8, (y * 11) as u8, ((x + y) * 5) as u8, 255]),
                );
            }
        }

        let rect = Rect {
            x: 2,
            y: 2,
            width: 7,
            height: 7,
        };

        let before_outside = *image.get_pixel(0, 0);
        let before_inside = *image.get_pixel(2, 2);

        render::apply_censor_rect(&mut image, rect, 3);

        let outside = *image.get_pixel(0, 0);
        let inside = *image.get_pixel(2, 2);

        assert_eq!(outside, before_outside);
        assert_ne!(inside, before_inside);

        let block_a = *image.get_pixel(2, 2);
        for y in 2..5 {
            for x in 2..5 {
                assert_eq!(*image.get_pixel(x, y), block_a);
            }
        }

        let block_b = *image.get_pixel(5, 2);
        for y in 2..5 {
            for x in 5..8 {
                assert_eq!(*image.get_pixel(x, y), block_b);
            }
        }
    }

    #[test]
    fn focus_darkens_pixels_outside_rect_only() {
        let mut image = RgbaImage::new(12, 12);
        for y in 0..12 {
            for x in 0..12 {
                image.put_pixel(x, y, image::Rgba([180, 170, 160, 255]));
            }
        }

        let rect = Rect {
            x: 3,
            y: 3,
            width: 6,
            height: 6,
        };

        let before_inside = *image.get_pixel(4, 4);
        let before_outside = *image.get_pixel(1, 1);

        render::apply_focus_rect(&mut image, rect);

        let inside = *image.get_pixel(4, 4);
        let outside = *image.get_pixel(1, 1);

        assert_eq!(inside, before_inside);
        assert!(outside[0] < before_outside[0]);
        assert!(outside[1] < before_outside[1]);
        assert!(outside[2] < before_outside[2]);
    }

    #[test]
    fn final_image_applies_focus_overlay() {
        let mut image = RgbaImage::new(12, 12);
        for y in 0..12 {
            for x in 0..12 {
                image.put_pixel(x, y, image::Rgba([180, 170, 160, 255]));
            }
        }

        let mut state = EditorState::new(image);
        state.actions.push(AnnotationAction::Focus {
            rect: Rect {
                x: 3,
                y: 3,
                width: 6,
                height: 6,
            },
        });

        let final_image = state.to_final_image().unwrap();
        let inside = *final_image.get_pixel(4, 4);
        let outside = *final_image.get_pixel(1, 1);

        assert_eq!(inside, image::Rgba([180, 170, 160, 255]));
        assert!(outside[0] < 180);
        assert!(outside[1] < 170);
        assert!(outside[2] < 160);
    }

    #[test]
    fn rect_from_points_normalizes_values() {
        let start = Point { x: 20.0, y: 10.0 };
        let end = Point { x: 2.0, y: 3.0 };
        let rect = Rect::from_points(start, end).unwrap();

        assert_eq!(rect.x, 2);
        assert_eq!(rect.y, 3);
        assert_eq!(rect.width, 18);
        assert_eq!(rect.height, 7);
    }

    #[test]
    fn final_image_applies_crop_selection() {
        let mut image = RgbaImage::new(12, 10);
        for y in 0..10 {
            for x in 0..12 {
                image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 80, 255]));
            }
        }

        let mut state = EditorState::new(image.clone());
        state.crop_selection = Some(Rect {
            x: 3,
            y: 2,
            width: 5,
            height: 4,
        });

        let final_image = state.to_final_image().unwrap();
        assert_eq!(final_image.dimensions(), (5, 4));
        assert_eq!(*final_image.get_pixel(0, 0), *image.get_pixel(3, 2));
        assert_eq!(*final_image.get_pixel(4, 3), *image.get_pixel(7, 5));
    }

    #[test]
    fn set_tool_clears_crop_selection_when_leaving_crop_mode() {
        let mut state = EditorState::new(RgbaImage::new(20, 20));
        state.set_tool(Tool::Crop);
        state.crop_selection = Some(Rect {
            x: 2,
            y: 2,
            width: 10,
            height: 10,
        });

        state.set_tool(Tool::Arrow);
        assert!(state.crop_selection.is_none());
    }

    #[test]
    fn apply_crop_selection_flattens_editor_state() {
        let mut image = RgbaImage::new(16, 12);
        for y in 0..12 {
            for x in 0..16 {
                image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 120, 255]));
            }
        }

        let mut state = EditorState::new(image);
        state.crop_selection = Some(Rect {
            x: 4,
            y: 3,
            width: 8,
            height: 5,
        });

        assert!(state.apply_crop_selection().unwrap());
        assert_eq!(state.base_image.dimensions(), (8, 5));
        assert_eq!(state.working_image.dimensions(), (8, 5));
        assert!(state.actions.is_empty());
        assert!(state.redo_actions.is_empty());
        assert!(state.crop_selection.is_none());
    }

    #[test]
    fn draft_action_uses_selected_color() {
        let mut state = EditorState::new(RgbaImage::new(20, 20));
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
    fn draft_line_with_shift_snaps_to_axis() {
        let mut state = EditorState::new(RgbaImage::new(20, 20));
        state.set_tool(Tool::Line);
        state.drag_shift_active = true;
        state.begin_drag(Point { x: 2.0, y: 3.0 });
        state.update_drag(Point { x: 11.0, y: 13.0 });

        match state.draft_action().unwrap() {
            AnnotationAction::Line { start, end, .. } => {
                assert_eq!(start, Point { x: 2.0, y: 3.0 });
                assert_eq!(end, Point { x: 2.0, y: 13.0 });
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
    fn draft_action_returns_censor_rect_when_tool_is_censor() {
        let mut state = EditorState::new(RgbaImage::new(20, 20));
        state.set_tool(Tool::Censor);
        state.begin_drag(Point { x: 1.0, y: 1.0 });
        state.update_drag(Point { x: 9.0, y: 8.0 });

        match state.draft_action().unwrap() {
            AnnotationAction::Censor { rect } => {
                assert_eq!(rect.x, 1);
                assert_eq!(rect.y, 1);
                assert_eq!(rect.width, 8);
                assert_eq!(rect.height, 7);
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
            AnnotationAction::Focus { rect } => {
                assert_eq!(rect.x, 2);
                assert_eq!(rect.y, 3);
                assert_eq!(rect.width, 11);
                assert_eq!(rect.height, 12);
            }
            other => panic!("unexpected draft action: {:?}", other),
        }
    }

    #[test]
    fn view_transform_scales_to_fit() {
        let t = ViewTransform::fit(4000.0, 2000.0, 1000.0, 500.0);

        assert!((t.scale - 0.25).abs() < f64::EPSILON);
        assert!((t.offset_x - 0.0).abs() < f64::EPSILON);
        assert!((t.offset_y - 0.0).abs() < f64::EPSILON);

        let mapped = t.view_to_image_clamped(Point { x: 500.0, y: 250.0 });
        assert!((mapped.x - 2000.0).abs() < f64::EPSILON);
        assert!((mapped.y - 1000.0).abs() < f64::EPSILON);
    }
}

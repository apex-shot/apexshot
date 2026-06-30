mod color;
mod composition;
mod io_ops;
#[allow(dead_code)]
pub mod numbering_style;
#[allow(dead_code)]
mod pen_weight;
pub mod preferences;
pub mod preprocess;
mod render;
mod selection;
mod state;
#[allow(dead_code)]
mod text_detect;
pub mod types;
pub(crate) mod ui_support;
#[path = "editor/window/mod.rs"]
pub mod window;

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
        assert_eq!(tool_shortcut_target('0'), Some((Tool::Select, 2)));
        assert_eq!(tool_shortcut_target('P'), Some((Tool::Pen, 3)));
        assert_eq!(tool_shortcut_target('t'), Some((Tool::Text, 8)));
        assert_eq!(tool_shortcut_target('l'), Some((Tool::Line, 7)));
        assert_eq!(tool_shortcut_target('a'), Some((Tool::Arrow, 6)));
        assert_eq!(tool_shortcut_target('r'), Some((Tool::Box, 4)));
        assert_eq!(tool_shortcut_target('o'), Some((Tool::Circle, 5)));
        assert_eq!(tool_shortcut_target('h'), Some((Tool::Highlighter, 12)));
        assert_eq!(tool_shortcut_target('c'), Some((Tool::Obfuscate, 9)));
        assert_eq!(tool_shortcut_target('n'), Some((Tool::Number, 11)));
        assert_eq!(tool_shortcut_target('x'), Some((Tool::Crop, 0)));
        assert_eq!(tool_shortcut_target('b'), Some((Tool::Obfuscate, 9)));
        assert_eq!(tool_shortcut_target('f'), Some((Tool::Focus, 10)));
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
            shadow: false,
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
    fn cursor_name_for_view_point_uses_crop_drag_and_resize_states() {
        let mut state = EditorState::new(RgbaImage::new(80, 80));
        let transform = ViewTransform::fit(80.0, 80.0, 80.0, 80.0);
        state.set_tool(Tool::Crop);
        state.crop_selection = Some(Rect {
            x: 20,
            y: 16,
            width: 24,
            height: 18,
        });

        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 32.0, y: 16.0 }),
            "ns-resize"
        );
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 28.0, y: 22.0 }),
            "grab"
        );

        state.select_drag_anchor = Some(Point { x: 32.0, y: 16.0 });
        state.select_resize_handle = Some(SelectHandle::Right);
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 36.0, y: 22.0 }),
            "ew-resize"
        );

        state.select_resize_handle = None;
        assert_eq!(
            cursor_name_for_view_point(&state, transform, Point { x: 30.0, y: 24.0 }),
            "grabbing"
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
            shadow: false,
        });

        // push_action auto-selects the last action
        assert!(state.can_remove_selected_action());

        state.selected_action_index = None;
        assert!(!state.can_remove_selected_action());

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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
        });

        // Click near the midpoint of the line (away from endpoints) to trigger drag, not resize
        assert!(state.begin_select_drag_with_scale(Point { x: 11.0, y: 13.0 }, 1.0));
        assert!(state.update_select_drag(Point { x: 18.0, y: 22.0 }));
        state.end_select_drag();

        match &state.actions[0] {
            AnnotationAction::Line { start, end, .. } => {
                // Delta: (18.0-11.0, 22.0-13.0) = (7.0, 9.0)
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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
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
            shadow: false,
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
        assert_eq!(state.next_number, 1); // reuses the removed number slot
    }

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

        render::apply_blur_rect(&mut image, rect, 2.0, false);

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

        render::apply_censor_rect(&mut image, rect, 3.0);

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

        render::apply_focus_rect(&mut image, rect, 58.0);

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
            intensity: 58.0,
        });
        state.rebuild_effect_layer();

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
    fn final_image_keeps_background_for_tall_capture_crop() {
        let mut image = RgbaImage::from_pixel(120, 1200, image::Rgba([240, 240, 240, 255]));
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));

        let mut state = EditorState::new(image);
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.05, 0.05, 0.05, 1.0));
        state.background_padding = 36.0;
        state.background_shadow = 30.0;
        state.crop_selection = Some(Rect {
            x: -20,
            y: -10,
            width: 180,
            height: 1240,
        });

        let final_image = state.to_final_image().expect("final image");

        assert!(final_image.width() > 180);
        assert!(final_image.height() > 1240);
        assert_eq!(*final_image.get_pixel(0, 0), image::Rgba([12, 12, 12, 255]));
    }

    #[test]
    fn final_image_shadow_does_not_replace_background_with_black_mask() {
        let image = RgbaImage::from_pixel(400, 240, image::Rgba([220, 220, 220, 255]));
        let mut state = EditorState::new(image);
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
        state.background_padding = 40.0;
        state.background_shadow = 45.0;

        let final_image = state.to_final_image().expect("final image");
        let corner = *final_image.get_pixel(0, 0);

        assert_ne!(corner, image::Rgba([0, 0, 0, 255]));
        assert_eq!(corner, image::Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn final_image_background_keeps_screenshot_at_native_scale_by_default() {
        let mut image = RgbaImage::new(400, 300);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.put_pixel(399, 299, image::Rgba([0, 0, 255, 255]));

        let mut state = EditorState::new(image.clone());
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
        state.background_shadow = 0.0;
        state.background_corner_radius = 0.0;

        let final_image = state.to_final_image().expect("final image");

        assert_eq!(final_image.dimensions(), (448, 348));
        assert_eq!(*final_image.get_pixel(24, 24), *image.get_pixel(0, 0));
        assert_eq!(*final_image.get_pixel(423, 323), *image.get_pixel(399, 299));
    }

    #[test]
    fn final_image_background_shadow_visibly_darkens_pixels_below_card() {
        let image = RgbaImage::from_pixel(400, 240, image::Rgba([220, 220, 220, 255]));
        let mut state = EditorState::new(image);
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(1.0, 1.0, 1.0, 1.0));
        state.background_padding = 40.0;
        state.background_insert = 0.0;
        state.background_shadow = 45.0;
        state.background_corner_radius = 18.0;

        let final_image = state.to_final_image().expect("final image");
        let shadow_pixel = *final_image.get_pixel(final_image.width() / 2, 290);

        assert!(
            shadow_pixel[0] < 235,
            "expected visible shadow below the card, got pixel {:?}",
            shadow_pixel
        );
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
    fn ensure_crop_selection_initialized_creates_default_crop_frame() {
        let mut state = EditorState::new(RgbaImage::new(200, 120));

        assert!(state.ensure_crop_selection_initialized());
        let crop = state.crop_selection.unwrap();
        assert_eq!(crop.x, 0);
        assert_eq!(crop.y, 0);
        assert_eq!(crop.width, 200);
        assert_eq!(crop.height, 120);
        assert!(!state.ensure_crop_selection_initialized());
    }

    #[test]
    fn crop_drag_moves_existing_selection_without_redrawing_first() {
        let mut state = EditorState::new(RgbaImage::new(160, 100));
        state.set_tool(Tool::Crop);
        state.crop_selection = Some(Rect {
            x: 20,
            y: 18,
            width: 80,
            height: 48,
        });

        assert!(state.begin_crop_drag_with_scale(Point { x: 40.0, y: 30.0 }, 1.0));
        assert!(state.update_crop_drag(Point { x: 52.0, y: 41.0 }));
        state.end_crop_drag();

        let crop = state.crop_selection.unwrap();
        assert_eq!(crop.x, 32);
        assert_eq!(crop.y, 29);
        assert_eq!(crop.width, 80);
        assert_eq!(crop.height, 48);
    }

    #[test]
    fn crop_edge_drag_resizes_selection_by_moving_single_edge() {
        let mut state = EditorState::new(RgbaImage::new(160, 100));
        state.set_tool(Tool::Crop);
        state.crop_selection = Some(Rect {
            x: 20,
            y: 18,
            width: 80,
            height: 48,
        });

        // Click on right edge (at x=100 which is 20+80)
        assert!(state.begin_crop_drag_with_scale(Point { x: 100.0, y: 42.0 }, 1.0));
        // Drag left by 8 pixels
        assert!(state.update_crop_drag(Point { x: 92.0, y: 42.0 }));
        state.end_crop_drag();

        let crop = state.crop_selection.unwrap();
        // Left edge should stay at 20, right edge should move to 92
        // Width = 92 - 20 = 72
        assert_eq!(crop.x, 20);
        assert_eq!(crop.y, 18);
        assert_eq!(crop.width, 72);
        assert_eq!(crop.height, 48);
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
}

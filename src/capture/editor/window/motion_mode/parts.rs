use gtk4::{Box as GtkBox, Button, DrawingArea, Entry, Label, Overlay, ToggleButton};
use std::cell::Cell;
use std::rc::Rc;

use crate::recording::editor::model::{MotionTextAnimation, MotionTextScope, MotionTimingKind};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::anchor_pad::MotionAnchorPad;
use super::position_pad::MotionPositionPad;
use super::text_pad::MotionTextPad;

pub(in crate::capture::editor::window) struct MotionModeParts {
    pub shell: MotionModeShellParts,
    pub timeline: MotionTimelineParts,
    pub panels: MotionPanelParts,
    pub shared: MotionSharedControlParts,
    pub text: MotionTextControlParts,
    pub transform: MotionTransformControlParts,
}

pub(in crate::capture::editor::window) struct MotionModeShellParts {
    pub static_toolbar: GtkBox,
    pub static_btn: Button,
    pub motion_btn: Button,
    pub preview: DrawingArea,
    pub preview_shell: Overlay,
    pub page: GtkBox,
    pub confirm_overlay: GtkBox,
}

pub(in crate::capture::editor::window) struct MotionTimelineParts {
    pub play_btn: Button,
    pub skip_back: Button,
    pub skip_forward: Button,
    pub add_btn: Button,
    pub add_text_btn: Button,
    pub undo_btn: Button,
    pub redo_btn: Button,
    pub playhead_clock: Label,
    pub duration_clock: Label,
    #[allow(dead_code)]
    pub timeline_card: GtkBox,
    pub ruler: DrawingArea,
    pub source_track: DrawingArea,
    pub motion_track: DrawingArea,
    pub text_track: DrawingArea,
    pub playhead_overlay: DrawingArea,
    pub hover_playhead: DrawingArea,
    pub playhead_dragging: Rc<Cell<bool>>,
    pub playhead_hovered: Rc<Cell<bool>>,
}

pub(in crate::capture::editor::window) struct MotionPanelParts {
    pub inspector: GtkBox,
    pub appearance_inspector: GtkBox,
    pub watermark_inspector: GtkBox,
    pub text_inspector: GtkBox,
}

pub(in crate::capture::editor::window) struct MotionSharedControlParts {
    pub duration_slider: FillSlider,
    pub duration_value: Label,
    pub blur_slider: FillSlider,
    pub blur_value: Label,
    pub blur_shutter_slider: FillSlider,
    pub blur_shutter_value: Label,
    pub clip_hint: Label,
    pub delete_btn: Button,
    pub inspector_syncing: Rc<Cell<bool>>,
}

pub(in crate::capture::editor::window) struct MotionTextControlParts {
    pub text_empty_box: GtkBox,
    pub text_editor_box: GtkBox,
    pub text_add_btn: Button,
    pub text_delete_btn: Button,
    pub text_entry: Entry,
    pub text_pos_pad: MotionTextPad,
    pub text_pos_readout: Label,
    pub text_size_slider: FillSlider,
    pub text_size_value: Label,
    pub text_anim_buttons: Vec<(MotionTextAnimation, ToggleButton)>,
    pub text_scope_buttons: Vec<(MotionTextScope, ToggleButton)>,
}

pub(in crate::capture::editor::window) struct MotionTransformControlParts {
    pub clip_box: GtkBox,
    pub scale_slider: FillSlider,
    pub intensity_slider: FillSlider,
    pub intensity_value: Label,
    pub anchor_pad: MotionAnchorPad,
    pub yaw_slider: FillSlider,
    pub yaw_value: Label,
    pub pitch_slider: FillSlider,
    pub pitch_value: Label,
    pub roll_slider: FillSlider,
    pub roll_value: Label,
    pub perspective_slider: FillSlider,
    pub perspective_value: Label,
    pub position_pad: MotionPositionPad,
    pub pos_x_slider: FillSlider,
    pub pos_x_value: Label,
    pub pos_y_slider: FillSlider,
    pub pos_y_value: Label,
    pub ease_slider: FillSlider,
    pub ease_value: Label,
    pub timing_kind_buttons: Vec<(MotionTimingKind, ToggleButton)>,
    pub custom_timing_btn: ToggleButton,
    pub custom_easing_rows: GtkBox,
    pub custom_spring_rows: GtkBox,
    pub spring_bounce_slider: FillSlider,
    pub spring_bounce_value: Label,
    pub easing_x1_slider: FillSlider,
    pub easing_x1_value: Label,
    pub easing_y1_slider: FillSlider,
    pub easing_y1_value: Label,
    pub easing_x2_slider: FillSlider,
    pub easing_x2_value: Label,
    pub easing_y2_slider: FillSlider,
    pub easing_y2_value: Label,
}

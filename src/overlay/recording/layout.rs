//! Recording panel layout computation.

use super::state::SettingsTab;
use crate::overlay::layout::{
    RectF, ACTION_CARD_GAP, FEATURE_PANEL_HEIGHT, FEATURE_PANEL_ITEM_WIDTH, FEATURE_PANEL_MARGIN,
    FEATURE_PANEL_TOP_GAP, TOOL_RAIL_GAP,
};

pub(crate) const REC_TOP_CLUSTER_WIDTH: f64 = 292.0;
pub(crate) const REC_TOP_CLUSTER_HEIGHT: f64 = 56.0;
pub(crate) const REC_ACTION_WIDTH: f64 = 120.0;
pub(crate) const REC_ACTION_HEIGHT: f64 = 50.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordingDeckLayout {
    pub(crate) left_toggle_rail: RectF,
    pub(crate) top_cluster: RectF,
    pub(crate) bottom_action_bar: RectF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordPanelTile {
    Controls,
    Size,
    Crop,
    Mic,
    Speaker,

    RecordVideo,
    RecordGif,
}

pub(crate) fn compute_recording_deck_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> RecordingDeckLayout {
    let rail_height = FEATURE_PANEL_HEIGHT * 2.0;
    let center_y = selection_y + selection_height / 2.0;
    let rail_x = (selection_x - TOOL_RAIL_GAP - FEATURE_PANEL_ITEM_WIDTH).max(FEATURE_PANEL_MARGIN);
    let rail_y = (center_y - rail_height / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - rail_height - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_x = (selection_x + (selection_width - REC_TOP_CLUSTER_WIDTH) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - REC_TOP_CLUSTER_WIDTH - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_y = (selection_y - FEATURE_PANEL_TOP_GAP - REC_TOP_CLUSTER_HEIGHT).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - REC_TOP_CLUSTER_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let action_width = REC_ACTION_WIDTH * 2.0 + ACTION_CARD_GAP;
    let action_x = (selection_x + (selection_width - action_width) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - action_width - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let below_y = selection_y + selection_height + FEATURE_PANEL_TOP_GAP;
    let action_y = if below_y + REC_ACTION_HEIGHT + FEATURE_PANEL_MARGIN <= screen_height {
        below_y
    } else {
        (screen_height - REC_ACTION_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN)
    };

    RecordingDeckLayout {
        left_toggle_rail: RectF {
            x: rail_x,
            y: rail_y,
            width: FEATURE_PANEL_ITEM_WIDTH,
            height: rail_height,
        },
        top_cluster: RectF {
            x: top_x,
            y: top_y,
            width: REC_TOP_CLUSTER_WIDTH,
            height: REC_TOP_CLUSTER_HEIGHT,
        },
        bottom_action_bar: RectF {
            x: action_x,
            y: action_y,
            width: action_width,
            height: REC_ACTION_HEIGHT,
        },
    }
}

pub(crate) fn compute_dropdown_popup_y(menu_y: f64, item_idx: usize, tab: SettingsTab) -> f64 {
    let start_y = menu_y + 110.0;
    match tab {
        SettingsTab::Video => match item_idx {
            3 => start_y,               // res dropdown button at curr_y=110
            4 => start_y + 35.0 + 50.0, // fps dropdown at curr_y=195
            _ => start_y,
        },
        SettingsTab::Gif => match item_idx {
            6 => start_y + 50.0 + 60.0 + 45.0, // size dropdown at curr_y=265
            _ => start_y,
        },
        _ => start_y,
    }
}

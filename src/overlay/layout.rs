use super::icons::TOOLBAR_ICONS;
use super::state::SettingsTab;

pub(crate) const DEFAULT_SELECTION_WIDTH: f64 = 600.0;
pub(crate) const DEFAULT_SELECTION_HEIGHT: f64 = 744.0;
pub(crate) const MIN_SELECTION_WIDTH: f64 = 24.0;
pub(crate) const MIN_SELECTION_HEIGHT: f64 = 24.0;
pub(crate) const BORDER_HANDLE_THRESHOLD: f64 = 10.0;
pub(crate) const HANDLE_MARKER_LENGTH: f64 = 20.0;
pub(crate) const HANDLE_MARKER_THICKNESS: f64 = 2.5;
pub(crate) const BRAND_ORANGE_R: f64 = 1.0;
pub(crate) const BRAND_ORANGE_G: f64 = 0.4;
pub(crate) const BRAND_ORANGE_B: f64 = 0.0;
pub(crate) const FEATURE_PANEL_ITEM_WIDTH: f64 = 76.0;
pub(crate) const FEATURE_PANEL_HEIGHT: f64 = 62.0;
pub(crate) const FEATURE_PANEL_RADIUS: f64 = 13.0;
pub(crate) const FEATURE_PANEL_TOP_GAP: f64 = 12.0;
pub(crate) const FEATURE_PANEL_MARGIN: f64 = 16.0;
pub(crate) const TOOL_RAIL_GAP: f64 = 18.0;
pub(crate) const ACTION_CARD_GAP: f64 = 8.0;
pub(crate) const SIZE_CARD_WIDTH: f64 = 152.0;
pub(crate) const SIZE_CARD_HEIGHT: f64 = 56.0;
pub(crate) const CROP_CARD_WIDTH: f64 = 62.0;
pub(crate) const REC_TOP_CLUSTER_WIDTH: f64 = 292.0;
pub(crate) const REC_TOP_CLUSTER_HEIGHT: f64 = 56.0;
pub(crate) const REC_ACTION_WIDTH: f64 = 120.0;
pub(crate) const REC_ACTION_HEIGHT: f64 = 50.0;
#[derive(Debug, Clone, Copy)]
pub(crate) struct RectF {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl RectF {
    pub(crate) fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolbarLayout {
    pub(crate) tools_panel: RectF,
    pub(crate) size_panel: RectF,
    pub(crate) crop_panel: RectF,
    pub(crate) item_cells: [RectF; 7],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarHit {
    Tool(usize),
    SizePanel,
    CropPanel,
}

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
    Webcam,
    Clicks,
    Keystrokes,
    RecordVideo,
    RecordGif,
}

pub(crate) const ASPECT_RATIO_OPTIONS: &[&str] = &[
    "Freeform",
    "1 : 1 (Square)",
    "5 : 4 (10 : 8)",
    "4 : 3",
    "7 : 5",
    "3 : 2",
    "16 : 10",
    "16 : 9",
    "2.35 : 1",
    "2 : 3",
    "9 : 16",
];

pub(crate) fn compute_toolbar_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> ToolbarLayout {
    let tool_panel_height = FEATURE_PANEL_HEIGHT * TOOLBAR_ICONS.len() as f64;
    let center_y = selection_y + selection_height / 2.0;
    let tool_x = (selection_x - TOOL_RAIL_GAP - FEATURE_PANEL_ITEM_WIDTH).max(FEATURE_PANEL_MARGIN);
    let tool_y = (center_y - tool_panel_height / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - tool_panel_height - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );

    let tools_panel = RectF {
        x: tool_x,
        y: tool_y,
        width: FEATURE_PANEL_ITEM_WIDTH,
        height: tool_panel_height,
    };

    let top_width = SIZE_CARD_WIDTH + ACTION_CARD_GAP + CROP_CARD_WIDTH;
    let top_x = (selection_x + (selection_width - top_width) / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_width - top_width - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );
    let top_y = (selection_y - FEATURE_PANEL_TOP_GAP - SIZE_CARD_HEIGHT).clamp(
        FEATURE_PANEL_MARGIN,
        (screen_height - SIZE_CARD_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
    );

    let size_panel = RectF {
        x: top_x,
        y: top_y,
        width: SIZE_CARD_WIDTH,
        height: SIZE_CARD_HEIGHT,
    };
    let crop_panel = RectF {
        x: top_x + SIZE_CARD_WIDTH + ACTION_CARD_GAP,
        y: top_y,
        width: CROP_CARD_WIDTH,
        height: SIZE_CARD_HEIGHT,
    };

    let mut item_cells = [RectF {
        x: 0.0,
        y: 0.0,
        width: FEATURE_PANEL_ITEM_WIDTH,
        height: FEATURE_PANEL_HEIGHT,
    }; 7];

    for (index, cell) in item_cells.iter_mut().enumerate() {
        cell.x = tools_panel.x;
        cell.y = tools_panel.y + index as f64 * FEATURE_PANEL_HEIGHT;
    }

    ToolbarLayout {
        tools_panel,
        size_panel,
        crop_panel,
        item_cells,
    }
}

pub(crate) fn compute_recording_deck_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> RecordingDeckLayout {
    let rail_height = FEATURE_PANEL_HEIGHT * 5.0;
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
    let above_y = selection_y - FEATURE_PANEL_TOP_GAP - REC_ACTION_HEIGHT;
    let action_y = if below_y + REC_ACTION_HEIGHT + FEATURE_PANEL_MARGIN <= screen_height {
        below_y
    } else {
        above_y.clamp(
            FEATURE_PANEL_MARGIN,
            (screen_height - REC_ACTION_HEIGHT - FEATURE_PANEL_MARGIN).max(FEATURE_PANEL_MARGIN),
        )
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

pub(crate) fn compute_aspect_menu_rects(
    anchor_rect: RectF,
    screen_width: f64,
    screen_height: f64,
) -> (RectF, Vec<RectF>) {
    let item_h = 34.0;
    let menu_w = 196.0;
    let menu_h = (ASPECT_RATIO_OPTIONS.len() as f64 * item_h) + 10.0;
    let menu_x = (anchor_rect.x + anchor_rect.width / 2.0 - menu_w / 2.0)
        .clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y =
        (anchor_rect.y + anchor_rect.height + 8.0).clamp(10.0, screen_height - menu_h - 10.0);
    let panel_rect = RectF {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: menu_h,
    };
    let mut item_rects = Vec::with_capacity(ASPECT_RATIO_OPTIONS.len());
    for i in 0..ASPECT_RATIO_OPTIONS.len() {
        item_rects.push(RectF {
            x: menu_x + 5.0,
            y: menu_y + 5.0 + i as f64 * item_h,
            width: menu_w - 10.0,
            height: item_h,
        });
    }
    (panel_rect, item_rects)
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

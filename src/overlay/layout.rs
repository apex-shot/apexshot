use super::icons::TOOLBAR_ICONS;

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

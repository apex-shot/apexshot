use super::icons::{ToolbarIcon, TOOLBAR_ICONS};
use super::layout::*;
// Recording-specific hit-testing lives in recording/hit_testing.rs

pub(crate) fn capture_crop_menu_hit_item(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<usize> {
    let layout = compute_toolbar_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    let anchor = layout.crop_panel;
    let (_panel, items) = compute_aspect_menu_rects(anchor, screen_width, screen_height);
    items.iter().position(|r| r.contains(x, y))
}

pub(crate) fn toolbar_item_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<ToolbarIcon> {
    match toolbar_hit_at(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
        x,
        y,
    ) {
        Some(ToolbarHit::Tool(index)) => Some(TOOLBAR_ICONS[index]),
        _ => None,
    }
}

pub(crate) fn toolbar_hit_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<ToolbarHit> {
    let layout = compute_toolbar_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );

    for (index, cell) in layout.item_cells.iter().enumerate() {
        if cell.contains(x, y) {
            return Some(ToolbarHit::Tool(index));
        }
    }

    if layout.size_panel.contains(x, y) {
        return Some(ToolbarHit::SizePanel);
    }
    if layout.crop_panel.contains(x, y) {
        return Some(ToolbarHit::CropPanel);
    }

    None
}

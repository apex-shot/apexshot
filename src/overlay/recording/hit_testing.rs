//! Recording panel hit-testing.

use super::layout::{compute_recording_deck_layout, RecordPanelTile, REC_ACTION_WIDTH};
use super::state::SettingsTab;
use crate::overlay::layout::{
    compute_aspect_menu_rects, RectF, ACTION_CARD_GAP, CROP_CARD_WIDTH, FEATURE_PANEL_HEIGHT,
    SIZE_CARD_WIDTH,
};

pub(crate) fn recording_crop_menu_hit_item(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<usize> {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    let top = deck.top_cluster;
    let anchor = RectF {
        x: top.x + 62.0 + 8.0 + 152.0 + 8.0,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let (_panel, items) = compute_aspect_menu_rects(anchor, screen_width, screen_height);
    items.iter().position(|r| r.contains(x, y))
}

pub(crate) fn recording_tile_at(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<RecordPanelTile> {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    let top = deck.top_cluster;
    let controls = RectF {
        x: top.x,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let size = RectF {
        x: controls.x + controls.width + ACTION_CARD_GAP,
        y: top.y,
        width: SIZE_CARD_WIDTH,
        height: top.height,
    };
    let crop = RectF {
        x: size.x + size.width + ACTION_CARD_GAP,
        y: top.y,
        width: CROP_CARD_WIDTH,
        height: top.height,
    };
    let rail = deck.left_toggle_rail;
    let rail_tiles = [
        (
            RecordPanelTile::Mic,
            RectF {
                x: rail.x,
                y: rail.y,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Speaker,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
        (
            RecordPanelTile::Webcam,
            RectF {
                x: rail.x,
                y: rail.y + FEATURE_PANEL_HEIGHT * 2.0,
                width: rail.width,
                height: FEATURE_PANEL_HEIGHT,
            },
        ),
    ];
    for (tile, rect) in [
        (RecordPanelTile::Controls, controls),
        (RecordPanelTile::Size, size),
        (RecordPanelTile::Crop, crop),
    ] {
        if rect.contains(x, y) {
            return Some(tile);
        }
    }
    for (tile, rect) in rail_tiles {
        if rect.contains(x, y) {
            return Some(tile);
        }
    }
    let actions = deck.bottom_action_bar;
    let video = RectF {
        x: actions.x,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    let gif = RectF {
        x: video.x + video.width + ACTION_CARD_GAP,
        y: actions.y,
        width: REC_ACTION_WIDTH,
        height: actions.height,
    };
    if video.contains(x, y) {
        return Some(RecordPanelTile::RecordVideo);
    }
    if gif.contains(x, y) {
        return Some(RecordPanelTile::RecordGif);
    }

    None
}

pub(crate) fn settings_menu_hit_item(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    _selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
    tab: SettingsTab,
) -> Option<i32> {
    let menu_w = 440.0;
    let menu_x = (selection_x + (selection_width - 440.0) / 2.0).clamp(10.0, screen_width - 450.0);
    let menu_y = (selection_y + 24.0).clamp(10.0, screen_height - 570.0);

    // Tab rects (always check, any tab)
    let tab_w = 78.0;
    let tab_h = 32.0;
    let tab_start_x = menu_x + (menu_w - 3.0 * tab_w) / 2.0;
    let tab_y = menu_y + 64.0;
    for i in 0..3 {
        let tr = RectF {
            x: tab_start_x + i as f64 * tab_w,
            y: tab_y,
            width: tab_w,
            height: tab_h,
        };
        if tr.contains(x, y) {
            return Some(i);
        }
    }

    let row_at = |cy: f64| -> bool {
        let w = menu_w - (130.0 - menu_x) - 25.0;
        RectF {
            x: menu_x + 130.0,
            y: cy,
            width: w,
            height: 32.0,
        }
        .contains(x, y)
    };

    match tab {
        SettingsTab::General => {
            let check_area_at = |cy: f64| -> bool {
                let value_x = menu_x + 140.0;
                RectF {
                    x: value_x,
                    y: cy,
                    width: menu_w - 160.0,
                    height: 32.0,
                }
                .contains(x, y)
            };
            let mut cy = menu_y + 110.0;
            let mut idx = 3;
            for _ in 0..4 {
                if check_area_at(cy) {
                    return Some(idx);
                }
                idx += 1;
                cy += 32.0;
            }
            cy += 10.0;
            for _ in 0..2 {
                if check_area_at(cy) {
                    return Some(idx);
                }
                idx += 1;
                cy += 32.0;
            }
            cy += 10.0;
            {
                if check_area_at(cy) {
                    return Some(idx);
                }
                idx += 1;
                cy += 32.0;
            }
            cy += 10.0;
            for _ in 0..3 {
                if check_area_at(cy) {
                    return Some(idx);
                }
                idx += 1;
                cy += 32.0;
            }
        }
        SettingsTab::Video => {
            let mut cy = menu_y + 110.0;
            if row_at(cy) {
                return Some(3);
            }
            cy += 35.0;
            cy += 50.0;
            if row_at(cy) {
                return Some(4);
            }
            cy += 45.0;
            if row_at(cy) {
                return Some(5);
            }
            cy += 50.0;
            if row_at(cy) {
                return Some(6);
            }
        }
        SettingsTab::Gif => {
            let mut cy = menu_y + 110.0;
            if row_at(cy) {
                return Some(3);
            }
            cy += 50.0;
            if row_at(cy) {
                return Some(4);
            }
            let optimize_rect = RectF {
                x: menu_x + 130.0 + 160.0 + 10.0,
                y: cy,
                width: 120.0,
                height: 30.0,
            };
            if optimize_rect.contains(x, y) {
                return Some(5);
            }
            cy += 115.0;
            if row_at(cy) {
                return Some(6);
            }
        }
    }

    None
}

pub(crate) fn webcam_options_hit_item(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    _selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> Option<i32> {
    let menu_w = 320.0;
    let item_h = 28.0;
    let header_h = 30.0;
    let pad = 8.0;

    let camera_ids: Vec<i32> = std::iter::once(0)
        .chain(
            crate::overlay::webcam::enumerate_webcam_devices()
                .into_iter()
                .map(|d| 100 + d),
        )
        .collect();
    let sections: Vec<Vec<i32>> = vec![
        camera_ids,
        vec![1, 2, 3, 4],
        vec![5],
        vec![6, 7, 8, 9],
        vec![10],
    ];

    let mut total_h = pad * 2.0;
    for section in &sections {
        total_h += header_h + section.len() as f64 * item_h;
    }

    let menu_x =
        (selection_x + (selection_width - menu_w) / 2.0).clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = (selection_y + 24.0).clamp(10.0, screen_height - total_h - 10.0);
    if !(RectF {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: total_h,
    })
    .contains(x, y)
    {
        return None;
    }

    let mut curr_y = menu_y + pad;
    for section in &sections {
        curr_y += header_h; // skip header
        for &item_id in section {
            let item_rect = RectF {
                x: menu_x + 4.0,
                y: curr_y + 1.0,
                width: menu_w - 8.0,
                height: item_h - 2.0,
            };
            if item_rect.contains(x, y) {
                return Some(item_id);
            }
            curr_y += item_h;
        }
    }
    None
}

pub(crate) fn recording_crop_menu_contains(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> bool {
    let deck = compute_recording_deck_layout(
        selection_x,
        selection_y,
        selection_width,
        selection_height,
        screen_width,
        screen_height,
    );
    let top = deck.top_cluster;
    let anchor = RectF {
        x: top.x + 62.0 + 8.0 + 152.0 + 8.0,
        y: top.y,
        width: 62.0,
        height: top.height,
    };
    let (panel, _items) = compute_aspect_menu_rects(anchor, screen_width, screen_height);
    panel.contains(x, y)
}

pub(crate) fn settings_menu_contains(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> bool {
    let menu_w = 440.0;
    let menu_x = (selection_x + (selection_width - menu_w) / 2.0).clamp(10.0, screen_width - 450.0);
    let menu_y = (selection_y + 24.0).clamp(10.0, screen_height - 570.0);
    RectF {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: 560.0,
    }
    .contains(x, y)
}

pub(crate) fn webcam_options_menu_contains(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
) -> bool {
    let menu_w = 320.0;
    let item_h = 28.0;
    let header_h = 30.0;
    let pad = 8.0;

    let camera_count = 1 + crate::overlay::webcam::enumerate_webcam_devices().len();
    let item_counts: &[usize] = &[4, 1, 4, 1];
    let mut total_h = pad * 2.0 + header_h + camera_count as f64 * item_h;
    for &c in item_counts {
        total_h += header_h + c as f64 * item_h;
    }

    let menu_x =
        (selection_x + (selection_width - menu_w) / 2.0).clamp(10.0, screen_width - menu_w - 10.0);
    let menu_y = (selection_y + 24.0).clamp(10.0, screen_height - total_h - 10.0);
    RectF {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: total_h,
    }
    .contains(x, y)
}

//! Recording panel hit-testing.

use super::layout::{
    compute_dropdown_popup_y, compute_recording_deck_layout, RecordPanelTile, REC_ACTION_WIDTH,
};
use super::state::SettingsTab;
use crate::overlay::layout::{
    compute_aspect_menu_rects, compute_settings_menu_layout, RectF, ACTION_CARD_GAP,
    CROP_CARD_WIDTH, FEATURE_PANEL_HEIGHT, SIZE_CARD_WIDTH,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_audio_rail_has_only_mic_and_speaker_slots() {
        let selection_x = 300.0;
        let selection_y = 220.0;
        let selection_width = 640.0;
        let selection_height = 360.0;
        let screen_width = 1600.0;
        let screen_height = 1000.0;

        let deck = compute_recording_deck_layout(
            selection_x,
            selection_y,
            selection_width,
            selection_height,
            screen_width,
            screen_height,
        );
        let rail = deck.left_toggle_rail;

        assert_eq!(rail.height, FEATURE_PANEL_HEIGHT * 2.0);
        assert_eq!(
            recording_tile_at(
                selection_x,
                selection_y,
                selection_width,
                selection_height,
                screen_width,
                screen_height,
                rail.x + rail.width / 2.0,
                rail.y + FEATURE_PANEL_HEIGHT / 2.0,
            ),
            Some(RecordPanelTile::Mic)
        );
        assert_eq!(
            recording_tile_at(
                selection_x,
                selection_y,
                selection_width,
                selection_height,
                screen_width,
                screen_height,
                rail.x + rail.width / 2.0,
                rail.y + FEATURE_PANEL_HEIGHT + FEATURE_PANEL_HEIGHT / 2.0,
            ),
            Some(RecordPanelTile::Speaker)
        );
        assert_eq!(
            recording_tile_at(
                selection_x,
                selection_y,
                selection_width,
                selection_height,
                screen_width,
                screen_height,
                rail.x + rail.width / 2.0,
                rail.y + FEATURE_PANEL_HEIGHT * 2.0 + 1.0,
            ),
            None
        );
    }

    #[test]
    fn settings_general_rows_keep_six_ordered_click_targets() {
        let selection_x = 300.0;
        let selection_y = 220.0;
        let selection_width = 640.0;
        let selection_height = 360.0;
        let screen_width = 1600.0;
        let screen_height = 1000.0;
        let panel = compute_settings_menu_layout(
            selection_x,
            selection_y,
            selection_width,
            screen_width,
            screen_height,
        )
        .panel;
        let row_centers = [128.0, 172.0, 216.0, 260.0, 304.0, 348.0];

        for (offset_y, expected_index) in row_centers.into_iter().zip(3..=8) {
            assert_eq!(
                settings_menu_hit_item(
                    selection_x,
                    selection_y,
                    selection_width,
                    selection_height,
                    screen_width,
                    screen_height,
                    panel.x + 30.0,
                    panel.y + offset_y,
                    SettingsTab::General,
                ),
                Some(expected_index)
            );
        }
        assert_eq!(
            settings_menu_hit_item(
                selection_x,
                selection_y,
                selection_width,
                selection_height,
                screen_width,
                screen_height,
                panel.x + 30.0,
                panel.y + 99.0,
                SettingsTab::General,
            ),
            None
        );
    }

    #[test]
    fn settings_dropdown_hover_geometry_matches_each_option_row() {
        let selection_x = 300.0;
        let selection_y = 220.0;
        let selection_width = 640.0;
        let screen_width = 1600.0;
        let screen_height = 1000.0;
        let panel = compute_settings_menu_layout(
            selection_x,
            selection_y,
            selection_width,
            screen_width,
            screen_height,
        )
        .panel;

        for (tab, drop_idx, option_count) in [
            (SettingsTab::Video, 3, 3),
            (SettingsTab::Video, 4, 4),
            (SettingsTab::Gif, 6, 4),
        ] {
            let popup_y = compute_dropdown_popup_y(panel.y, drop_idx, tab);
            let popup_width = if matches!((tab, drop_idx), (SettingsTab::Gif, 6)) {
                180.0
            } else {
                160.0
            };
            let popup_x = panel.x + 408.0 - popup_width;
            for expected in 0..option_count {
                assert_eq!(
                    settings_dropdown_hit_item(
                        selection_x,
                        selection_y,
                        selection_width,
                        screen_width,
                        screen_height,
                        popup_x + 10.0,
                        popup_y + expected as f64 * 30.0 + 15.0,
                        tab,
                        drop_idx,
                    ),
                    Some(expected)
                );
            }
            assert_eq!(
                settings_dropdown_hit_item(
                    selection_x,
                    selection_y,
                    selection_width,
                    screen_width,
                    screen_height,
                    popup_x - 1.0,
                    popup_y + 15.0,
                    tab,
                    drop_idx,
                ),
                None
            );
            assert_eq!(
                settings_dropdown_hit_item(
                    selection_x,
                    selection_y,
                    selection_width,
                    screen_width,
                    screen_height,
                    popup_x + 10.0,
                    popup_y + option_count as f64 * 30.0 + 1.0,
                    tab,
                    drop_idx,
                ),
                None
            );
        }
    }
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
    let settings = compute_settings_menu_layout(
        selection_x,
        selection_y,
        selection_width,
        screen_width,
        screen_height,
    );
    let menu_w = settings.panel.width;
    let menu_x = settings.panel.x;
    let menu_y = settings.panel.y;

    // Tab rects (always check, any tab)
    let tab_container_w = menu_w - 36.0;
    let tab_w = (tab_container_w - 8.0) / 3.0;
    let tab_h = 30.0;
    let tab_start_x = menu_x + 22.0;
    let tab_y = menu_y + 54.0;
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

    match tab {
        SettingsTab::General => {
            let check_area_at = |cy: f64| -> bool {
                RectF {
                    x: menu_x + 18.0,
                    y: cy,
                    width: menu_w - 36.0,
                    height: 44.0,
                }
                .contains(x, y)
            };
            let mut cy = menu_y + 106.0;
            for idx in 3..9 {
                if check_area_at(cy) {
                    return Some(idx);
                }
                cy += 44.0;
            }
        }
        SettingsTab::Video => {
            if (RectF {
                x: menu_x + 272.0,
                y: menu_y + 128.0,
                width: 136.0,
                height: 30.0,
            })
            .contains(x, y)
            {
                return Some(3);
            }
            if (RectF {
                x: menu_x + 332.0,
                y: menu_y + 193.0,
                width: 76.0,
                height: 30.0,
            })
            .contains(x, y)
            {
                return Some(4);
            }
            if (RectF {
                x: menu_x + 18.0,
                y: menu_y + 234.0,
                width: menu_w - 36.0,
                height: 52.0,
            })
            .contains(x, y)
            {
                return Some(5);
            }
            if (RectF {
                x: menu_x + 18.0,
                y: menu_y + 286.0,
                width: menu_w - 36.0,
                height: 76.0,
            })
            .contains(x, y)
            {
                return Some(6);
            }
        }
        SettingsTab::Gif => {
            if (RectF {
                x: menu_x + 200.0,
                y: menu_y + 116.0,
                width: 208.0,
                height: 44.0,
            })
            .contains(x, y)
            {
                return Some(3);
            }
            if (RectF {
                x: menu_x + 160.0,
                y: menu_y + 180.0,
                width: 248.0,
                height: 46.0,
            })
            .contains(x, y)
            {
                return Some(4);
            }
            if (RectF {
                x: menu_x + 18.0,
                y: menu_y + 242.0,
                width: menu_w - 36.0,
                height: 52.0,
            })
            .contains(x, y)
            {
                return Some(5);
            }
            if (RectF {
                x: menu_x + 228.0,
                y: menu_y + 311.0,
                width: 180.0,
                height: 30.0,
            })
            .contains(x, y)
            {
                return Some(6);
            }
        }
    }

    None
}

pub(crate) fn settings_dropdown_hit_item(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    screen_width: f64,
    screen_height: f64,
    x: f64,
    y: f64,
    tab: SettingsTab,
    drop_idx: usize,
) -> Option<usize> {
    let option_count = match (tab, drop_idx) {
        (SettingsTab::Video, 3) => 3,
        (SettingsTab::Video, 4) | (SettingsTab::Gif, 6) => 4,
        _ => return None,
    };
    let settings = compute_settings_menu_layout(
        selection_x,
        selection_y,
        selection_width,
        screen_width,
        screen_height,
    );
    let popup_width = if matches!((tab, drop_idx), (SettingsTab::Gif, 6)) {
        180.0
    } else {
        160.0
    };
    let popup = RectF {
        x: settings.panel.x + 408.0 - popup_width,
        y: compute_dropdown_popup_y(settings.panel.y, drop_idx, tab),
        width: popup_width,
        height: option_count as f64 * 30.0,
    };
    if !popup.contains(x, y) {
        return None;
    }

    Some(((y - popup.y) / 30.0).floor() as usize).filter(|index| *index < option_count)
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
    let settings = compute_settings_menu_layout(
        selection_x,
        selection_y,
        selection_width,
        screen_width,
        screen_height,
    );
    settings.panel.contains(x, y)
}

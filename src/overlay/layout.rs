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
pub(crate) const FEATURE_PANEL_TOP_GAP: f64 = 12.0;
pub(crate) const FEATURE_PANEL_MARGIN: f64 = 16.0;
/// Keep bottom-anchored chrome above the app dock.
pub(crate) const DOCK_LIFT: f64 = 76.0;
pub(crate) const ACTION_CARD_GAP: f64 = 8.0;

// ── Top-center instruction bar ("Draw an area" frame) ────────────────────────
// Screen-fixed chrome mirroring capture-overlay's computeTopBarLayout: it is
// never attached to the selection, so dragging near/over it is safe. The
// final screenshot crops the pre-overlay freeze, so the bar never appears in
// the saved image.
pub(crate) const TOP_BAR_H: f64 = 44.0;
pub(crate) const TOP_BAR_Y: f64 = 16.0;
pub(crate) const TOP_BAR_RADIUS: f64 = 14.0;
pub(crate) const TOP_BAR_ASPECT_H: f64 = 28.0;
pub(crate) const TOP_BAR_BTN_SIZE: f64 = 32.0;

/// Pill labels and aspect ratios (W/H). Index 0 is Free (`0.0`).
pub(crate) const TOP_BAR_PILL_LABELS: [&str; 4] = ["Free", "16:9", "4:3", "1:1"];
pub(crate) const TOP_BAR_PILL_RATIOS: [f64; 4] = [0.0, 16.0 / 9.0, 4.0 / 3.0, 1.0];
/// Legacy capture-menu aspect indices for each pill (Free, 16:9, 4:3, 1:1).
pub(crate) const TOP_BAR_PILL_LEGACY_INDICES: [usize; 4] = [0, 7, 3, 1];
/// Button slots: 0 opens the ratio dropdown, 1 cancels the selection.
pub(crate) const TOP_BAR_CROP_BUTTON: usize = 0;
pub(crate) const TOP_BAR_CANCEL_BUTTON: usize = 1;

/// Crop dropdown rows. Row 0 resets the selection, row 10 toggles snapping,
/// the rest apply fixed ratios (0.0 entries are Free / N/A for action rows).
pub(crate) const TOP_BAR_CROP_LABELS: [&str; 11] = [
    "Reset selection",
    "Free",
    "1:1",
    "2:1",
    "3:2",
    "4:3",
    "9:16",
    "16:9",
    "16:10",
    "21:9",
    "Snap to ratios",
];
pub(crate) const TOP_BAR_CROP_RATIOS: [f64; 11] = [
    0.0,
    0.0,
    1.0,
    2.0,
    3.0 / 2.0,
    4.0 / 3.0,
    9.0 / 16.0,
    16.0 / 9.0,
    16.0 / 10.0,
    21.0 / 9.0,
    0.0,
];
pub(crate) const TOP_BAR_CROP_ROW_RESET: usize = 0;
pub(crate) const TOP_BAR_CROP_ROW_SNAP: usize = 10;
pub(crate) const TOP_BAR_CROP_MENU_W: f64 = 196.0;
pub(crate) const TOP_BAR_CROP_ITEM_H: f64 = 32.0;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarHit {
    // Retired with the legacy left rail: nothing constructs these on the
    // capture path anymore (see hit_testing). Kept for API compat.
    #[allow(dead_code)]
    Tool(usize),
    // Legacy FRAME panels: no longer hit-tested on the capture path (the
    // top-center bar owns aspects now). Kept for API compat with defensive
    // match arms in drag/motion.
    #[allow(dead_code)]
    SizePanel,
    #[allow(dead_code)]
    CropPanel,
}

/// Top-center instruction bar layout. All rects are screen coordinates.
/// `pills`: 0=Free, 1=16:9, 2=4:3, 3=1:1. `buttons`: 0=Crop, 1=Cancel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TopBarLayout {
    pub(crate) bar: RectF,
    pub(crate) label: RectF,
    pub(crate) pills: [RectF; 4],
    pub(crate) buttons: [RectF; 2],
}

/// Screen-fixed top-center bar. Returns `None` when the screen is too narrow
/// to fit the bar (same math as C++ `computeTopBarLayout`).
pub(crate) fn compute_top_bar_layout(screen_width: f64) -> Option<TopBarLayout> {
    const PAD_X: f64 = 14.0;
    const LABEL_W: f64 = 108.0;
    const SEP_GAP: f64 = 12.0;
    const SEP_W: f64 = 1.0;
    const PILL_GAP: f64 = 6.0;
    const BTN_GAP: f64 = 6.0;
    const PILL_W: [f64; 4] = [52.0, 52.0, 48.0, 44.0];
    let pills_w = PILL_W[0] + PILL_W[1] + PILL_W[2] + PILL_W[3] + PILL_GAP * 3.0;
    let btns_w = TOP_BAR_BTN_SIZE * 2.0 + BTN_GAP;
    let total_w =
        PAD_X * 2.0 + LABEL_W + SEP_GAP * 2.0 + SEP_W + SEP_GAP * 2.0 + SEP_W + pills_w + btns_w;
    if total_w > screen_width - FEATURE_PANEL_MARGIN * 2.0 {
        return None;
    }
    let bar_x = (FEATURE_PANEL_MARGIN).max((screen_width - total_w) / 2.0);
    let bar_y = TOP_BAR_Y;
    let bar = RectF {
        x: bar_x,
        y: bar_y,
        width: total_w,
        height: TOP_BAR_H,
    };
    let mut cx = bar_x + PAD_X;
    let label = RectF {
        x: cx,
        y: bar_y,
        width: LABEL_W,
        height: TOP_BAR_H,
    };
    cx += LABEL_W + SEP_GAP;
    cx += SEP_W + SEP_GAP; // first separator (painted, not hit-tested)
    let pill_y = bar_y + (TOP_BAR_H - TOP_BAR_ASPECT_H) / 2.0;
    let mut pills = [RectF {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }; 4];
    for (i, pill) in pills.iter_mut().enumerate() {
        *pill = RectF {
            x: cx,
            y: pill_y,
            width: PILL_W[i],
            height: TOP_BAR_ASPECT_H,
        };
        cx += PILL_W[i] + PILL_GAP;
    }
    cx -= PILL_GAP;
    cx += SEP_GAP;
    cx += SEP_W + SEP_GAP; // second separator
    let btn_y = bar_y + (TOP_BAR_H - TOP_BAR_BTN_SIZE) / 2.0;
    let mut buttons = [RectF {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }; 2];
    for button in buttons.iter_mut() {
        *button = RectF {
            x: cx,
            y: btn_y,
            width: TOP_BAR_BTN_SIZE,
            height: TOP_BAR_BTN_SIZE,
        };
        cx += TOP_BAR_BTN_SIZE + BTN_GAP;
    }
    Some(TopBarLayout {
        bar,
        label,
        pills,
        buttons,
    })
}

/// Crop dropdown anchored under the crop button (`anchor`), opening below the
/// bar. Returns the panel rect plus one item rect per crop row.
pub(crate) fn compute_top_bar_crop_menu(
    anchor: RectF,
    bar_bottom: f64,
    screen_width: f64,
) -> (RectF, Vec<RectF>) {
    let menu_w = TOP_BAR_CROP_MENU_W;
    let menu_h = (TOP_BAR_CROP_LABELS.len() as f64 * TOP_BAR_CROP_ITEM_H) + 10.0;
    let menu_x = (anchor.x + anchor.width / 2.0 - menu_w / 2.0)
        .max(10.0)
        .min(screen_width - menu_w - 10.0);
    let menu_y = (bar_bottom + 4.0).max(anchor.y + anchor.height + 8.0);
    let panel = RectF {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: menu_h,
    };
    let mut items = Vec::with_capacity(TOP_BAR_CROP_LABELS.len());
    for i in 0..TOP_BAR_CROP_LABELS.len() {
        items.push(RectF {
            x: menu_x + 5.0,
            y: menu_y + 5.0 + i as f64 * TOP_BAR_CROP_ITEM_H,
            width: menu_w - 10.0,
            height: TOP_BAR_CROP_ITEM_H,
        });
    }
    (panel, items)
}

// ── Shared popup / menu layouts (drawing + hit-testing consume these) ─────────

pub(crate) const SCROLL_POPUP_WIDTH: f64 = 360.0;
pub(crate) const SCROLL_POPUP_HEIGHT: f64 = 170.0;
pub(crate) const SCROLL_POPUP_CLOSE_SIZE: f64 = 22.0;
pub(crate) const SCROLL_POPUP_DOWNLOAD_W: f64 = 182.0;
pub(crate) const SCROLL_POPUP_DOWNLOAD_H: f64 = 34.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollPopupLayout {
    pub(crate) panel: RectF,
    pub(crate) close: RectF,
    pub(crate) download: RectF,
}

/// Centered scroll-capture extension CTA. Drawing and hit-testing share this.
pub(crate) fn compute_scroll_popup_layout(center_x: f64, center_y: f64) -> ScrollPopupLayout {
    let panel = RectF {
        x: center_x - SCROLL_POPUP_WIDTH / 2.0,
        y: center_y - SCROLL_POPUP_HEIGHT / 2.0,
        width: SCROLL_POPUP_WIDTH,
        height: SCROLL_POPUP_HEIGHT,
    };
    let close = RectF {
        x: panel.x + panel.width - SCROLL_POPUP_CLOSE_SIZE - 10.0,
        y: panel.y + 10.0,
        width: SCROLL_POPUP_CLOSE_SIZE,
        height: SCROLL_POPUP_CLOSE_SIZE,
    };
    let download = RectF {
        x: panel.x + (panel.width - SCROLL_POPUP_DOWNLOAD_W) / 2.0,
        y: panel.y + 102.0,
        width: SCROLL_POPUP_DOWNLOAD_W,
        height: SCROLL_POPUP_DOWNLOAD_H,
    };
    ScrollPopupLayout {
        panel,
        close,
        download,
    }
}

pub(crate) const WINDOW_PICKER_MENU_W: f64 = 320.0;
pub(crate) const WINDOW_PICKER_ITEM_H: f64 = 28.0;
pub(crate) const WINDOW_PICKER_HEADER_H: f64 = 30.0;
pub(crate) const WINDOW_PICKER_PAD: f64 = 8.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowPickerLayout {
    pub(crate) panel: RectF,
    pub(crate) list_y: f64,
    pub(crate) item_h: f64,
}

pub(crate) fn compute_window_picker_layout(
    center_x: f64,
    center_y: f64,
    screen_width: f64,
    screen_height: f64,
    window_count: usize,
) -> WindowPickerLayout {
    let total_h = WINDOW_PICKER_PAD * 2.0
        + WINDOW_PICKER_HEADER_H
        + window_count as f64 * WINDOW_PICKER_ITEM_H;
    let popup_x = (center_x - WINDOW_PICKER_MENU_W / 2.0)
        .clamp(10.0, (screen_width - WINDOW_PICKER_MENU_W - 10.0).max(10.0));
    let popup_y =
        (center_y - total_h / 2.0).clamp(10.0, (screen_height - total_h - 10.0).max(10.0));
    WindowPickerLayout {
        panel: RectF {
            x: popup_x,
            y: popup_y,
            width: WINDOW_PICKER_MENU_W,
            height: total_h,
        },
        list_y: popup_y + WINDOW_PICKER_PAD + WINDOW_PICKER_HEADER_H,
        item_h: WINDOW_PICKER_ITEM_H,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_bar_layout_centers_and_fits_pills_and_buttons() {
        // Legacy rail geometry is retired; the top-center bar carries the
        // aspect UI. Pin its centered layout instead.
        let layout = compute_top_bar_layout(1920.0).expect("bar must fit 1080p");
        let bar_cx = layout.bar.x + layout.bar.width / 2.0;
        assert!((bar_cx - 960.0).abs() < 1e-9);
        assert_eq!(layout.bar.height, TOP_BAR_H);
        assert_eq!(layout.pills.len(), TOP_BAR_PILL_LABELS.len());
        assert_eq!(layout.buttons.len(), 2);
        assert!(compute_top_bar_layout(300.0).is_none());
    }

    #[test]
    fn scroll_popup_layout_has_close_and_download_inside_panel() {
        let layout = compute_scroll_popup_layout(500.0, 400.0);
        assert!(layout
            .panel
            .contains(layout.close.x + 1.0, layout.close.y + 1.0));
        assert!(layout
            .panel
            .contains(layout.download.x + 1.0, layout.download.y + 1.0));
        assert_eq!(layout.panel.width, SCROLL_POPUP_WIDTH);
        assert_eq!(layout.panel.height, SCROLL_POPUP_HEIGHT);
    }

    #[test]
    fn window_picker_layout_scales_with_entry_count() {
        let one = compute_window_picker_layout(500.0, 400.0, 1920.0, 1080.0, 1);
        let three = compute_window_picker_layout(500.0, 400.0, 1920.0, 1080.0, 3);
        assert!((three.panel.height - one.panel.height - 2.0 * WINDOW_PICKER_ITEM_H).abs() < 1e-9);
        assert_eq!(one.item_h, WINDOW_PICKER_ITEM_H);
    }

    #[test]
    fn top_bar_layout_is_centered_with_fixed_chrome() {
        let layout = compute_top_bar_layout(1920.0).expect("bar must fit 1920px");
        // Total width: 28 pad + 108 label + 25 + 25 seps + 214 pills + 70 buttons.
        assert!((layout.bar.width - 470.0).abs() < 1e-9);
        assert!((layout.bar.height - TOP_BAR_H).abs() < 1e-9);
        assert!((layout.bar.y - TOP_BAR_Y).abs() < 1e-9);
        assert!((layout.bar.x - (1920.0 - 470.0) / 2.0).abs() < 1e-9);
        // Label sits at the left pad; pills/buttons are vertically centered.
        assert!((layout.label.x - (layout.bar.x + 14.0)).abs() < 1e-9);
        assert!((layout.label.width - 108.0).abs() < 1e-9);
        let pill_widths = [52.0, 52.0, 48.0, 44.0];
        for (i, pill) in layout.pills.iter().enumerate() {
            assert!((pill.width - pill_widths[i]).abs() < 1e-9);
            assert!((pill.height - TOP_BAR_ASPECT_H).abs() < 1e-9);
            assert!((pill.y - (TOP_BAR_Y + 8.0)).abs() < 1e-9);
            assert!(layout.bar.contains(pill.x + 1.0, pill.y + 1.0));
            if i > 0 {
                let prev = layout.pills[i - 1];
                assert!((pill.x - (prev.x + prev.width + 6.0)).abs() < 1e-9);
            }
        }
        for button in layout.buttons.iter() {
            assert!((button.width - TOP_BAR_BTN_SIZE).abs() < 1e-9);
            assert!((button.height - TOP_BAR_BTN_SIZE).abs() < 1e-9);
            assert!((button.y - (TOP_BAR_Y + 6.0)).abs() < 1e-9);
            assert!(layout.bar.contains(button.x + 1.0, button.y + 1.0));
        }
        // Buttons start after the pills plus the second separator lane.
        let last_pill = layout.pills[3];
        assert!((layout.buttons[0].x - (last_pill.x + last_pill.width + 25.0)).abs() < 1e-9);
    }

    #[test]
    fn top_bar_layout_returns_none_when_too_narrow() {
        assert!(compute_top_bar_layout(400.0).is_none());
        assert!(compute_top_bar_layout(1920.0).is_some());
    }

    #[test]
    fn top_bar_crop_menu_anchors_under_crop_button() {
        let layout = compute_top_bar_layout(1920.0).expect("bar must fit");
        let anchor = layout.buttons[TOP_BAR_CROP_BUTTON];
        let bar_bottom = layout.bar.y + layout.bar.height;
        let (panel, items) = compute_top_bar_crop_menu(anchor, bar_bottom, 1920.0);
        assert_eq!(panel.width, TOP_BAR_CROP_MENU_W);
        assert_eq!(items.len(), TOP_BAR_CROP_LABELS.len());
        assert!((panel.height - (11.0 * TOP_BAR_CROP_ITEM_H + 10.0)).abs() < 1e-9);
        assert!((panel.x - (anchor.x + anchor.width / 2.0 - panel.width / 2.0)).abs() < 1e-9);
        assert!(panel.y >= bar_bottom + 4.0 - 1e-9);
        assert!(panel.y >= anchor.y + anchor.height + 8.0 - 1e-9);
        for (i, item) in items.iter().enumerate() {
            assert!((item.y - (panel.y + 5.0 + i as f64 * TOP_BAR_CROP_ITEM_H)).abs() < 1e-9);
            assert!(panel.contains(item.x + 1.0, item.y + 1.0));
        }
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later

// Matches the Qt toolbar's own top offset and the layer-shell menu's top margin.
export const CAPTURE_MENU_TOP_MARGIN = 28;

function isRect(rect) {
    return Boolean(rect) &&
        ['x', 'y', 'width', 'height'].every(key =>
            typeof rect[key] === 'number' && Number.isFinite(rect[key]));
}

/// Top-centre position for the capture toolbar within a monitor's work area.
///
/// Wayland clients cannot place themselves, so the shell moves the frame. A
/// frame wider than the work area keeps its left edge visible.
export function captureMenuPosition(workArea, frameRect, margin = CAPTURE_MENU_TOP_MARGIN) {
    if (!isRect(workArea) || !isRect(frameRect))
        return null;
    if (frameRect.width <= 0 || frameRect.height <= 0)
        return null;

    let x = workArea.x + Math.round((workArea.width - frameRect.width) / 2);
    if (frameRect.width > workArea.width)
        x = workArea.x;
    return {x, y: workArea.y + margin};
}

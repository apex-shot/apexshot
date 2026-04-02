// SPDX-License-Identifier: AGPL-3.0-or-later

const CONTROLS_MARGIN = 24;
const CONTROLS_DOCK_SAFE = 64;
const CONTROLS_GAP = 2;

export function createRuntimeOverlayMenuStyle(width) {
    return [
        `width: ${width}px;`,
        "background-color: #141414;",
        "border: 1px solid rgba(255, 255, 255, 0.10);",
        "border-radius: 12px;",
        "padding: 8px 4px;",
        "spacing: 0;",
        "box-shadow: 0 8px 24px rgba(0, 0, 0, 0.36);",
    ].join(" ");
}

export function createRuntimeOverlayHeaderStyle() {
    return "padding: 10px 14px 6px 14px; font-size: 10px; font-weight: 800; color: rgba(255, 255, 255, 0.45); background: transparent; letter-spacing: 0.8px;";
}

export function createWarningPopupStyle() {
    return [
        "background-color: #141414;",
        "border: 1px solid rgba(255, 255, 255, 0.12);",
        "border-radius: 10px;",
        "padding: 12px 16px;",
        "spacing: 4px;",
        "box-shadow: 0 14px 34px rgba(0, 0, 0, 0.34);",
    ].join(" ");
}

export function createRuntimeOverlayRowStyles(hasSupportMessage) {
    return {
        button: [
            "padding: 6px 8px 6px 8px;",
            "border-radius: 6px;",
            "margin: 1px 4px;",
            "background-color: transparent;",
        ].join(" "),
        baseButton: "padding: 6px 8px 6px 8px; border-radius: 6px; margin: 1px 4px;",
        layout: {
            style: "spacing: 8px;",
            expandHorizontally: true,
        },
        checkSlot: {
            width: 18,
        },
        label: {
            style: `font-size: 13px; text-align: left; color: ${hasSupportMessage ? "rgba(255, 255, 255, 0.5)" : "#F1F1F3"};`,
            hoverStyle: "font-size: 13px; text-align: left; color: white;",
            expandHorizontally: true,
            xAlign: "start",
        },
        infoButton: [
            "width: 18px;",
            "height: 18px;",
            "padding: 0;",
            "border-radius: 9px;",
            "background-color: rgba(255, 255, 255, 0.06);",
            "border: 1px solid rgba(255, 255, 255, 0.1);",
        ].join(" "),
    };
}

export function computeAdjacentPopupPosition({
    anchorRect,
    popupSize,
    monitor,
    gap = 10,
    margin = 10,
}) {
    const minX = monitor.x + margin;
    const maxX = Math.max(minX, monitor.x + monitor.width - popupSize.width - margin);
    const minY = monitor.y + margin;
    const maxY = Math.max(minY, monitor.y + monitor.height - popupSize.height - margin);

    const preferredRightX = anchorRect.x + anchorRect.width + gap;
    const preferredLeftX = anchorRect.x - popupSize.width - gap;
    const x = preferredRightX <= maxX
        ? preferredRightX
        : Math.max(minX, Math.min(preferredLeftX, maxX));

    const centeredY = anchorRect.y + Math.floor((anchorRect.height - popupSize.height) / 2);
    const y = Math.max(minY, Math.min(centeredY, maxY));

    return {x, y};
}

export function computeControlsDockPosition({
    rect,
    isFullscreen,
    visibilityPolicy = isFullscreen ? "visible" : "area-outside-capture",
    monitor,
    controlsSize,
}) {
    if (visibilityPolicy === "hidden")
        return null;

    const minX = monitor.x + CONTROLS_MARGIN;
    const maxX = Math.max(
        minX,
        monitor.x + monitor.width - controlsSize.width - CONTROLS_MARGIN
    );
    const topY = monitor.y + CONTROLS_MARGIN;

    if (isFullscreen || !rect || rect.width <= 0 || rect.height <= 0) {
        return {
            x: monitor.x + Math.floor((monitor.width - controlsSize.width) / 2),
            y: topY,
        };
    }

    const x = Math.max(minX, Math.min(
        rect.x + Math.floor((rect.width - controlsSize.width) / 2),
        maxX
    ));
    const belowY = rect.y + rect.height + CONTROLS_GAP;
    if (belowY + controlsSize.height + CONTROLS_DOCK_SAFE <= monitor.y + monitor.height) {
        return {x, y: belowY};
    }

    const aboveY = rect.y - controlsSize.height - CONTROLS_GAP;
    if (aboveY >= topY) {
        return {x, y: aboveY};
    }

    const maxY = monitor.y + monitor.height - controlsSize.height - CONTROLS_MARGIN;
    return {
        x,
        y: Math.max(topY, Math.min(aboveY, maxY)),
    };
}

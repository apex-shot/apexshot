// SPDX-License-Identifier: AGPL-3.0-or-later

export function createRuntimeOverlayMenuStyle(width) {
    return [
        `width: ${width}px;`,
        "background-color: rgba(30, 30, 30, 0.92);",
        "border-radius: 12px;",
        "padding: 8px 4px;",
        "spacing: 0;",
    ].join(" ");
}

export function createRuntimeOverlayHeaderStyle() {
    return "padding: 10px 14px 4px 14px; font-size: 11px; font-weight: 700; color: rgba(255, 255, 255, 110); background: transparent;";
}

export function createWarningPopupStyle() {
    return [
        "background-color: rgba(40, 40, 45, 0.95);",
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
            style: `font-size: 13px; text-align: left; color: ${hasSupportMessage ? "rgba(255,255,255,110)" : "#F1F1F3"};`,
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

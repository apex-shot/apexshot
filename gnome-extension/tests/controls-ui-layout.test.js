// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    computeAdjacentPopupPosition,
    createRuntimeOverlayHeaderStyle,
    createRuntimeOverlayMenuStyle,
    createRuntimeOverlayRowStyles,
    createWarningPopupStyle,
} from "../controls-ui-layout.js";

function assert(condition, message) {
    if (!condition)
        throw new Error(message);
}

function assertEqual(actual, expected, message) {
    if (actual !== expected)
        throw new Error(`${message}: expected ${expected}, got ${actual}`);
}

function runTest(name, fn) {
    try {
        fn();
        print(`ok - ${name}`);
    } catch (error) {
        printerr(`not ok - ${name}`);
        printerr(error?.stack ?? String(error));
        throw error;
    }
}

runTest("positions popup next to the anchor actor in stage coordinates", () => {
    const position = computeAdjacentPopupPosition({
        anchorRect: {x: 260, y: 180, width: 18, height: 18},
        popupSize: {width: 140, height: 64},
        monitor: {x: 0, y: 0, width: 640, height: 480},
        gap: 10,
        margin: 10,
    });

    assertEqual(position.x, 288, "popup should open to the right of the anchor");
    assertEqual(position.y, 157, "popup should vertically center on the anchor");
});

runTest("clamps popup inside the monitor when there is no room to the right", () => {
    const position = computeAdjacentPopupPosition({
        anchorRect: {x: 610, y: 30, width: 18, height: 18},
        popupSize: {width: 120, height: 80},
        monitor: {x: 0, y: 0, width: 640, height: 480},
        gap: 10,
        margin: 10,
    });

    assertEqual(position.x, 480, "popup should flip to the left when needed");
    assertEqual(position.y, 10, "popup should clamp to the top monitor margin");
});

runTest("runtime overlay rows reserve full width for left-aligned labels", () => {
    const styles = createRuntimeOverlayRowStyles(false);

    assert(styles.layout.expandHorizontally, "row layout should expand across the menu");
    assertEqual(styles.checkSlot.width, 18, "tick slot should keep a fixed leading column");
    assert(styles.label.expandHorizontally, "label should claim the remaining row width");
    assertEqual(styles.label.xAlign, "start", "label should be pinned to the left edge");
    assert(styles.label.style.includes("text-align: left;"), "label text should be explicitly left aligned");
});

runTest("menu and popup styles drop the white border outline", () => {
    assert(!createRuntimeOverlayMenuStyle(200).includes("border:"), "menu should not render a border outline");
    assert(!createWarningPopupStyle().includes("border:"), "warning popup should not render a border outline");
});

runTest("overlay header matches the webcam menu heading treatment", () => {
    const style = createRuntimeOverlayHeaderStyle();

    assertEqual(
        style,
        "padding: 10px 14px 4px 14px; font-size: 11px; font-weight: 700; color: rgba(255, 255, 255, 110); background: transparent;",
        "header style should match the webcam menu disabled header"
    );
});

runTest("runtime overlay menu style is compact and shadowless", () => {
    const style = createRuntimeOverlayMenuStyle(168);

    assert(style.includes("width: 168px;"), "menu should use the reduced compact width");
    assert(!style.includes("box-shadow:"), "menu should not render a drop shadow");
});

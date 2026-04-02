// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    computeControlsDockPosition,
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

runTest("menu and popup styles include the updated border treatment", () => {
    assert(createRuntimeOverlayMenuStyle(200).includes("border:"), "menu should render the updated border outline");
    assert(createWarningPopupStyle().includes("border:"), "warning popup should render the updated border outline");
});

runTest("overlay header matches the webcam menu heading treatment", () => {
    const style = createRuntimeOverlayHeaderStyle();

    assertEqual(
        style,
        "padding: 10px 14px 6px 14px; font-size: 10px; font-weight: 800; color: rgba(255, 255, 255, 0.45); background: transparent; letter-spacing: 0.8px;",
        "header style should match the webcam menu disabled header"
    );
});

runTest("runtime overlay menu style is compact and retains the new shadow", () => {
    const style = createRuntimeOverlayMenuStyle(168);

    assert(style.includes("width: 168px;"), "menu should use the reduced compact width");
    assert(style.includes("box-shadow:"), "menu should render the updated drop shadow");
});

runTest("docks area recording controls outside the captured rectangle", () => {
    const position = computeControlsDockPosition({
        rect: {x: 100, y: 100, width: 640, height: 360},
        isFullscreen: false,
        visibilityPolicy: "area-outside-capture",
        monitor: {x: 0, y: 0, width: 1280, height: 720},
        controlsSize: {width: 420, height: 46},
    });

    assertEqual(position.x, 210, "controls should remain centered to the capture width");
    assertEqual(position.y, 462, "controls should dock below the captured area");
});

runTest("suppresses fullscreen controls when the visibility policy is hidden", () => {
    const position = computeControlsDockPosition({
        rect: {x: 0, y: 0, width: 1920, height: 1080},
        isFullscreen: true,
        visibilityPolicy: "hidden",
        monitor: {x: 0, y: 0, width: 1920, height: 1080},
        controlsSize: {width: 420, height: 46},
    });

    assertEqual(position, null, "hidden fullscreen controls should not produce a dock position");
});

// SPDX-License-Identifier: AGPL-3.0-or-later

import {activateWindowRecord, buildWindowListPayload} from "../window-list.js";

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

runTest("buildWindowListPayload keeps normal windows and strips apexshot/skip-taskbar entries", () => {
    const payload = buildWindowListPayload([
        {
            id: 7,
            title: "Firefox",
            app: "Firefox",
            x: 12,
            y: 24,
            width: 1280,
            height: 720,
            visible: true,
            minimized: false,
            skipTaskbar: false,
            apexshot: false,
        },
        {
            id: 8,
            title: "Skip me",
            app: "Dock",
            x: 0,
            y: 0,
            width: 800,
            height: 48,
            visible: true,
            minimized: false,
            skipTaskbar: true,
            apexshot: false,
        },
        {
            id: 9,
            title: "ApexShot Capture",
            app: "ApexShot",
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            visible: true,
            minimized: false,
            skipTaskbar: false,
            apexshot: true,
        },
        {
            id: 10,
            title: "Background app",
            app: "Notes",
            x: 40,
            y: 40,
            width: 600,
            height: 400,
            visible: false,
            minimized: true,
            skipTaskbar: false,
            apexshot: false,
        },
    ]);

    assertEqual(payload.length, 2, "normal + minimized windows should remain");
    assertEqual(payload[0].id, 7, "eligible window id should be preserved");
    assertEqual(payload[0].title, "Firefox", "title should be preserved");
    assertEqual(payload[0].app, "Firefox", "app should be preserved");
    assertEqual(payload[1].id, 10, "minimized windows should be listed");
    assertEqual(payload[0].thumbnail_b64, "", "static payload starts without inline thumbnails");
});

runTest("buildWindowListPayload clamps geometry to positive sizes", () => {
    const payload = buildWindowListPayload([
        {
            id: 10,
            title: "Terminal",
            app: "Terminal",
            x: -5,
            y: -9,
            width: 0,
            height: -30,
            visible: true,
            minimized: false,
            skipTaskbar: false,
            apexshot: false,
        },
    ]);

    assertEqual(payload.length, 1, "window should still be serializable");
    assert(payload[0].width >= 1, "width should be clamped to at least 1");
    assert(payload[0].height >= 1, "height should be clamped to at least 1");
});

runTest("buildWindowListPayload does not exclude non-apexshot apps whose titles mention apexshot", () => {
    const payload = buildWindowListPayload([
        {
            id: 11,
            title: "~/Desktop/apexshot",
            app: "Ghostty",
            x: 50,
            y: 60,
            width: 900,
            height: 700,
            visible: true,
            minimized: false,
            skipTaskbar: false,
            apexshot: true,
        },
    ]);

    assertEqual(payload.length, 1, "ghostty should remain eligible even if title mentions apexshot");
    assertEqual(payload[0].app, "Ghostty", "non-ApexShot app should be preserved");
});

runTest("activateWindowRecord unminimizes and activates the selected window", () => {
    let unminimized = false;
    let activatedAt = null;
    const metaWindow = {
        minimized: true,
        unminimize() {
            unminimized = true;
            this.minimized = false;
        },
        activate(timestamp) {
            activatedAt = timestamp;
        },
    };

    const activated = activateWindowRecord(metaWindow, 4242);

    assertEqual(activated, true, "window should be activated");
    assertEqual(unminimized, true, "minimized window should be restored before capture");
    assertEqual(activatedAt, 4242, "activation should use the requested timestamp");
});

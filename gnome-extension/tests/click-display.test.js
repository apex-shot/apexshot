// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createClickOverlayModel,
    getActiveClickIndicator,
    recordPointerSample,
} from "../click-display.js";

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

runTest("records a left-click only on the press transition", () => {
    const model = createClickOverlayModel();

    const first = recordPointerSample(model, {
        x: 640,
        y: 360,
        left: false,
        middle: false,
        right: false,
        timestampMs: 100,
    });
    assertEqual(first.length, 0, "no click should be recorded before a press");

    const second = recordPointerSample(model, {
        x: 640,
        y: 360,
        left: true,
        middle: false,
        right: false,
        timestampMs: 120,
    });
    assertEqual(second.length, 1, "one click should be recorded on press");
    assertEqual(second[0].button, "left", "left button should be reported");
    assertEqual(second[0].x, 640, "click x should be preserved");
    assertEqual(second[0].y, 360, "click y should be preserved");

    const third = recordPointerSample(model, {
        x: 642,
        y: 362,
        left: true,
        middle: false,
        right: false,
        timestampMs: 140,
    });
    assertEqual(third.length, 0, "holding the button should not create duplicate clicks");
});

runTest("records left and right presses independently", () => {
    const model = createClickOverlayModel();

    recordPointerSample(model, {
        x: 100,
        y: 200,
        left: false,
        middle: false,
        right: false,
        timestampMs: 10,
    });

    const clicks = recordPointerSample(model, {
        x: 100,
        y: 200,
        left: true,
        middle: false,
        right: true,
        timestampMs: 20,
    });

    assertEqual(clicks.length, 2, "left and right transitions should both be captured");
    assert(clicks.some(click => click.button === "left"), "left click should be present");
    assert(clicks.some(click => click.button === "right"), "right click should be present");
});

runTest("active click indicator expires quickly after the press", () => {
    const model = createClickOverlayModel();

    recordPointerSample(model, {
        x: 300,
        y: 220,
        left: false,
        middle: false,
        right: false,
        timestampMs: 0,
    });
    recordPointerSample(model, {
        x: 300,
        y: 220,
        left: true,
        middle: false,
        right: false,
        timestampMs: 10,
    });

    assert(getActiveClickIndicator(model, 120), "click indicator should still be active early");
    assertEqual(getActiveClickIndicator(model, 260), null, "click indicator should expire quickly");
});

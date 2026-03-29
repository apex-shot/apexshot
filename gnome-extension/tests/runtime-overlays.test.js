// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createRenderableRuntimeOverlayVisibility,
    hasRenderableRuntimeOverlays,
} from "../runtime-overlays-visibility.js";

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

runTest("renderable visibility ignores mic and speaker overlays", () => {
    const visibility = createRenderableRuntimeOverlayVisibility({
        mic: true,
        speaker: true,
        webcam: false,
        clicks: true,
        keystrokes: false,
    });

    assertEqual(visibility.mic, false, "mic should not render as an on-screen overlay");
    assertEqual(visibility.speaker, false, "speaker should not render as an on-screen overlay");
    assertEqual(visibility.clicks, true, "clicks should still render");
});

runTest("renderable overlays require webcam clicks or keystrokes", () => {
    assert(
        !hasRenderableRuntimeOverlays({mic: true, speaker: true, webcam: false, clicks: false, keystrokes: false}),
        "audio-only visibility should not create GNOME on-screen overlays"
    );
    assert(
        hasRenderableRuntimeOverlays({mic: false, speaker: false, webcam: true, clicks: false, keystrokes: false}),
        "webcam visibility should still create GNOME on-screen overlays"
    );
});

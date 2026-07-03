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
    });

    assertEqual(visibility.mic, false, "mic should not render as an on-screen overlay");
    assertEqual(visibility.speaker, false, "speaker should not render as an on-screen overlay");
});

runTest("audio-only visibility does not create renderable shell overlays", () => {
    assert(
        !hasRenderableRuntimeOverlays({mic: true, speaker: true}),
        "audio-only visibility should not create GNOME on-screen overlays"
    );
});

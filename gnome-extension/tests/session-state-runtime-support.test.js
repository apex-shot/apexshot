// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createSessionState,
    setControlsState,
} from "../session-state.js";

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

runTest("parses mic and speaker visibility from the runtime snapshot", () => {
    const session = createSessionState();
    setControlsState(session, {
        dbusDest: "org.apexshot.RecordingControl",
        sessionId: "recording-audio-overlays",
        rect: {x: 0, y: 0, width: 100, height: 100},
        isFullscreen: false,
        showTimer: true,
        runtimeOverlaySnapshot: JSON.stringify({
            mic_visible: true,
            speaker_visible: true,
        }),
    }, 0);

    assertEqual(session.runtimeOverlaySnapshot.mic_visible, true, "mic visibility should be preserved");
    assertEqual(session.runtimeOverlaySnapshot.speaker_visible, true, "speaker visibility should be preserved");
});

runTest("stores the recording controls visibility policy on the active session", () => {
    const session = createSessionState();
    setControlsState(session, {
        dbusDest: "org.apexshot.RecordingControl",
        sessionId: "recording-controls-policy",
        rect: {x: 20, y: 30, width: 640, height: 360},
        isFullscreen: false,
        showTimer: true,
        visibilityPolicy: "area-outside-capture",
        runtimeOverlaySnapshot: null,
    }, 0);

    assertEqual(
        session.controlsState.visibilityPolicy,
        "area-outside-capture",
        "controls state should preserve the runtime visibility policy"
    );
});

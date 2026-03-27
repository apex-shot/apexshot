// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createSessionState,
    pushRuntimeOverlayKeystrokeText,
    setControlsState,
} from "../session-state.js";
import {getKeystrokeOverlayText} from "../keystroke-display.js";

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

runTest("pushes runtime keystroke text into the active session overlay state", () => {
    const session = createSessionState();
    setControlsState(session, {
        dbusDest: "org.apexshot.RecordingControl",
        sessionId: "recording-123",
        rect: {x: 0, y: 0, width: 100, height: 100},
        isFullscreen: false,
        showTimer: true,
        runtimeOverlaySnapshot: JSON.stringify({
            keystrokes_enabled: true,
            key_filter: 0,
        }),
    }, 0);

    const changed = pushRuntimeOverlayKeystrokeText(session, "Ctrl + K", 100);
    assert(changed, "push should succeed when keystrokes are enabled");
    assertEqual(
        getKeystrokeOverlayText(session.runtimeOverlayState.keystrokeOverlay, 100),
        "Ctrl + K",
        "pushed text should be visible"
    );
});

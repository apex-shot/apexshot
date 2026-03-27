// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createSessionState,
    getRuntimeOverlaySupportMessage,
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

runTest("reads the keystroke support message from the runtime snapshot", () => {
    const session = createSessionState();
    setControlsState(session, {
        dbusDest: "org.apexshot.RecordingControl",
        sessionId: "recording-unsupported",
        rect: {x: 0, y: 0, width: 100, height: 100},
        isFullscreen: false,
        showTimer: true,
        runtimeOverlaySnapshot: JSON.stringify({
            keystrokes_enabled: true,
            keystrokes_supported: false,
            keystrokes_support_message: "Not supported on GNOME Wayland yet",
        }),
    }, 0);

    assertEqual(
        getRuntimeOverlaySupportMessage(session, "keystrokes"),
        "Not supported on GNOME Wayland yet",
        "support message should come from the snapshot"
    );
});

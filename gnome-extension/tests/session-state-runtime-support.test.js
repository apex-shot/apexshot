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

runTest("parses webcam preview manifest path from the runtime snapshot", () => {
    const session = createSessionState();
    setControlsState(session, {
        dbusDest: "org.apexshot.RecordingControl",
        sessionId: "recording-webcam-preview",
        rect: {x: 0, y: 0, width: 100, height: 100},
        isFullscreen: false,
        showTimer: true,
        runtimeOverlaySnapshot: JSON.stringify({
            webcam_enabled: true,
            webcam_preview_manifest_path: "/tmp/apexshot-gnome-webcam-preview/recording-webcam-preview/manifest.json",
        }),
    }, 0);

    assertEqual(
        session.runtimeOverlaySnapshot.webcam_preview_manifest_path,
        "/tmp/apexshot-gnome-webcam-preview/recording-webcam-preview/manifest.json",
        "manifest path should be preserved for GNOME live webcam preview"
    );
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

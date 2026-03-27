// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    createKeystrokeOverlayModel,
    recordKeystrokeEvent,
} from "../keystroke-display.js";

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

runTest("records live command keystrokes in order", () => {
    const model = createKeystrokeOverlayModel();

    const changed = recordKeystrokeEvent(model, {
        keySymbol: "A",
        unicodeChar: "a",
        ctrl: true,
        alt: false,
        shift: true,
        meta: false,
        timestampMs: 100,
    });

    assert(changed, "command shortcut should update overlay");
    assertEqual(model.entries.length, 1, "one entry should be recorded");
    assertEqual(model.entries[0].text, "Ctrl + Shift + A", "shortcut text should match live keys");
});

runTest("ignores modifier-only presses", () => {
    const model = createKeystrokeOverlayModel();

    const changed = recordKeystrokeEvent(model, {
        keySymbol: "Shift_L",
        unicodeChar: "",
        ctrl: false,
        alt: false,
        shift: true,
        meta: false,
        timestampMs: 100,
    });

    assert(!changed, "modifier-only presses should not create overlay text");
    assertEqual(model.entries.length, 0, "modifier-only press should not be stored");
});

runTest("command-only filter hides normal typing", () => {
    const model = createKeystrokeOverlayModel();

    const changed = recordKeystrokeEvent(model, {
        keySymbol: "k",
        unicodeChar: "k",
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        timestampMs: 100,
        filterMode: 1,
    });

    assert(!changed, "plain typing should be filtered out");
    assertEqual(model.entries.length, 0, "plain typing should not be recorded");
});

runTest("command-only filter keeps shortcuts", () => {
    const model = createKeystrokeOverlayModel();

    const changed = recordKeystrokeEvent(model, {
        keySymbol: "k",
        unicodeChar: "k",
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
        timestampMs: 100,
        filterMode: 1,
    });

    assert(changed, "shortcut should pass command-only filter");
    assertEqual(model.entries[0].text, "Ctrl + K", "shortcut label should be preserved");
});

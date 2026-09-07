// SPDX-License-Identifier: AGPL-3.0-or-later

import {applyAlwaysOnTop} from '../preview-stacking.js';

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
        printerr(error.stack);
        throw error;
    }
}

function fakeWindow(overrides = {}) {
    const calls = [];
    const window = {
        above: false,
        minimized: false,
        get_compositor_private() {
            return true;
        },
        unminimize() {
            calls.push('unminimize');
            this.minimized = false;
        },
        make_above() {
            calls.push('make_above');
            this.above = true;
        },
        raise() {
            calls.push('raise');
        },
        ...overrides,
    };
    window.calls = calls;
    return window;
}

runTest('applyAlwaysOnTop raises and marks the window above', () => {
    const window = fakeWindow();
    assertEqual(applyAlwaysOnTop(window), true, 'live window should be raised');
    assertEqual(window.calls.join(','), 'make_above,raise',
        'window should be made above then restacked');
    assertEqual(window.above, true, 'above flag should be set');
});

runTest('applyAlwaysOnTop unminimizes before making above', () => {
    const window = fakeWindow({minimized: true});
    applyAlwaysOnTop(window);
    assertEqual(window.calls.join(','), 'unminimize,make_above,raise',
        'minimized previews should be restored before stacking');
});

runTest('applyAlwaysOnTop skips windows that are not on the compositor', () => {
    const window = fakeWindow({
        get_compositor_private() {
            return null;
        },
    });
    assertEqual(applyAlwaysOnTop(window), false, 'unmanaged window should be ignored');
    assertEqual(window.calls.join(','), '', 'unmanaged window should not be touched');
});

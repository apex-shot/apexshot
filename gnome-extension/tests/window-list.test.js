// SPDX-License-Identifier: AGPL-3.0-or-later

import {activateWindowRecord, buildWindowListPayload} from '../window-list.js';

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

function window(overrides) {
    return Object.assign({
        id: 1,
        title: 'Window',
        app: 'App',
        x: 0,
        y: 0,
        width: 800,
        height: 600,
        minimized: false,
        skipTaskbar: false,
        wmClass: '',
    }, overrides);
}

runTest('window list keeps normal windows, including minimized ones', () => {
    const payload = buildWindowListPayload([
        window({id: 7, title: 'Firefox', app: 'Firefox', x: 12, y: 24}),
        window({id: 10, title: 'Notes', app: 'Notes', minimized: true}),
    ]);

    assertEqual(payload.length, 2, 'both windows should be listed');
    assertEqual(payload[0].id, 7, 'window id should be preserved');
    assertEqual(payload[0].title, 'Firefox', 'title should be preserved');
    assertEqual(payload[0].x, 12, 'position should be preserved');
    assertEqual(payload[1].minimized, true, 'minimized state should be reported');
});

runTest('window list drops recording overlays and windows outside the window list, keeps normal ApexShot windows', () => {
    const payload = buildWindowListPayload([
        window({id: 8, app: 'Dock', skipTaskbar: true}),
        window({id: 9, app: 'Recording', wmClass: 'com.apexshot.recording'}),
        window({id: 10, app: 'ApexShot', wmClass: 'io.github.codegoddy.apexshot'}),
    ]);

    assertEqual(payload.length, 1, 'only the normal ApexShot window should be offered to the picker');
    assertEqual(payload[0].id, 10, 'the normal ApexShot window should be listed');
});

runTest('window list clamps sizes so the picker can always lay out a card', () => {
    const [entry] = buildWindowListPayload([window({width: 0, height: -30})]);

    assertEqual(entry.width, 1, 'width should be clamped to at least 1');
    assertEqual(entry.height, 1, 'height should be clamped to at least 1');
});

runTest('activating a window restores it before focusing it', () => {
    const calls = [];
    const metaWindow = {
        minimized: true,
        unminimize() {
            calls.push('unminimize');
            this.minimized = false;
        },
        activate(timestamp) {
            calls.push(`activate:${timestamp}`);
        },
    };

    assertEqual(activateWindowRecord(metaWindow, 4242), true, 'window should be activated');
    assertEqual(calls.join(','), 'unminimize,activate:4242',
        'window should be unminimized before being focused');
});

runTest('activating a window that no longer exists fails cleanly', () => {
    assertEqual(activateWindowRecord(null, 0), false, 'missing window should not activate');
});

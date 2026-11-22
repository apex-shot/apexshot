// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    CAPTURE_MENU_TOP_MARGIN,
    captureMenuPosition,
} from '../capture-menu-placement.js';

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

runTest('centres the toolbar on a primary work area', () => {
    const pos = captureMenuPosition(
        {x: 0, y: 32, width: 1920, height: 1080},
        {x: 580, y: 500, width: 700, height: 58});
    assertEqual(pos.x, 610, 'x should be centred in the work area');
    assertEqual(pos.y, 60, 'y should sit below the work area top by the margin');
    assertEqual(CAPTURE_MENU_TOP_MARGIN, 28, 'default margin should stay at 28px');
});

runTest('centres the toolbar on a work area offset to a second monitor', () => {
    const workArea = {x: 1920, y: 0, width: 1920, height: 1080};
    const pos = captureMenuPosition(workArea, {x: 1300, y: 400, width: 700, height: 58});
    assertEqual(pos.x, 1920 + Math.round((1920 - 700) / 2),
        'x should be relative to the monitor origin');
    assertEqual(pos.y, CAPTURE_MENU_TOP_MARGIN, 'y should use the margin below the monitor top');
});

runTest('clamps a frame wider than the work area to its left edge', () => {
    const workArea = {x: 300, y: 32, width: 800, height: 600};
    const pos = captureMenuPosition(workArea, {x: 0, y: 0, width: 900, height: 58});
    assertEqual(pos.x, 300, 'x should clamp to the work area origin');
    assertEqual(pos.y, 60, 'y should still use the margin');
});

runTest('returns null for a zero-size frame', () => {
    const workArea = {x: 0, y: 32, width: 1920, height: 1080};
    assertEqual(captureMenuPosition(workArea, {x: 0, y: 0, width: 0, height: 58}),
        null, 'zero width frame should be deferred');
    assertEqual(captureMenuPosition(workArea, {x: 0, y: 0, width: 700, height: 0}),
        null, 'zero height frame should be deferred');
});

runTest('returns null when an input rect is missing', () => {
    assertEqual(captureMenuPosition(null, {x: 0, y: 0, width: 700, height: 58}),
        null, 'missing work area should be deferred');
    assertEqual(captureMenuPosition({x: 0, y: 32, width: 1920, height: 1080}, null),
        null, 'missing frame should be deferred');
});

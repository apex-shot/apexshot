// SPDX-License-Identifier: AGPL-3.0-or-later

import {classifyCursorGeometry, classifyCursorTracker} from '../cursor-classifier.js';

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

runTest('classifies Adwaita arrow, hand, and text hotspots', () => {
    assertEqual(classifyCursorGeometry(24, 24, 3, 1), 'default', 'arrow');
    assertEqual(classifyCursorGeometry(24, 24, 7, 5), 'hand', 'hand');
    assertEqual(classifyCursorGeometry(24, 24, 11, 12), 'text', 'text');
});

runTest('classification scales with HiDPI cursor sprites', () => {
    assertEqual(classifyCursorGeometry(48, 48, 14, 10), 'hand', 'scaled hand');
    assertEqual(classifyCursorGeometry(48, 48, 22, 24), 'text', 'scaled text');
});

runTest('reads cursor geometry from the Mutter tracker', () => {
    const tracker = {
        get_hot() {
            return [7, 5];
        },
        get_sprite() {
            return {
                get_width: () => 24,
                get_height: () => 24,
            };
        },
    };
    assertEqual(classifyCursorTracker(tracker), 'hand', 'tracker hand');
});

runTest('falls back safely when cursor metadata is unavailable', () => {
    assertEqual(classifyCursorTracker(null), 'default', 'missing tracker');
    assertEqual(classifyCursorGeometry(0, 24, 0, 0), 'default', 'invalid geometry');
});
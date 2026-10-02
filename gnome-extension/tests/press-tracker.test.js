// SPDX-License-Identifier: AGPL-3.0-or-later

import {PressTracker, DRAG_THRESHOLD_PX, MAX_PRESSES} from '../press-tracker.js';

function assertEqual(actual, expected, message) {
    if (actual !== expected)
        throw new Error(`${message}: expected ${expected}, got ${actual}`);
}

function assertDeepEqual(actual, expected, message) {
    if (JSON.stringify(actual) !== JSON.stringify(expected))
        throw new Error(`${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
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

runTest('a still click is one closed interval and not a drag', () => {
    const tracker = new PressTracker();
    tracker.press(1, 1.0, 100, 100);
    tracker.move(100, 100);
    tracker.release(1, 1.2);
    assertDeepEqual(tracker.take(), [[1.0, 1.2, 1, false]], 'click interval');
});

runTest('movement past the threshold marks the press as a drag', () => {
    const tracker = new PressTracker();
    tracker.press(1, 1.0, 100, 100);
    tracker.move(100 + DRAG_THRESHOLD_PX + 1, 100);
    tracker.release(1, 1.5);
    assertDeepEqual(tracker.take(), [[1.0, 1.5, 1, true]], 'drag interval');
});

runTest('sub-threshold jitter does not mark a drag', () => {
    const tracker = new PressTracker();
    tracker.press(1, 2.0, 50, 50);
    tracker.move(50 + DRAG_THRESHOLD_PX - 1, 50);
    tracker.release(1, 2.1);
    assertDeepEqual(tracker.take(), [[2.0, 2.1, 1, false]], 'jitter stays a click');
});

runTest('a press still held at stop closes at the stop instant', () => {
    const tracker = new PressTracker();
    tracker.press(3, 4.0, 10, 10);
    tracker.closeAll(4.8);
    assertDeepEqual(tracker.take(), [[4.0, 4.8, 3, false]], 'open press closed at stop');
});

runTest('take resets the tracker for the next recording', () => {
    const tracker = new PressTracker();
    tracker.press(1, 0.0, 1, 1);
    tracker.release(1, 0.1);
    assertEqual(tracker.take().length, 1, 'first take');
    assertEqual(tracker.take().length, 0, 'second take is empty');
});

runTest('the oldest interval is dropped once the cap is reached', () => {
    const tracker = new PressTracker();
    for (let i = 0; i <= MAX_PRESSES; i++) {
        tracker.press(1, i, 0, 0);
        tracker.release(1, i + 0.5);
    }
    const presses = tracker.take();
    assertEqual(presses.length, MAX_PRESSES, 'capped length');
    assertEqual(presses[0][0], 1, 'oldest dropped');
    assertEqual(presses[presses.length - 1][0], MAX_PRESSES, 'newest kept');
});
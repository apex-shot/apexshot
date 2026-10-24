// SPDX-License-Identifier: AGPL-3.0-or-later

import {classifyCursorGeometry, classifyCursorTracker} from '../cursor-classifier.js';
import {
    buildCursorShapeMap,
    classifyCursorFromMap,
    loadCursorThemeData,
} from '../cursor-classifier.js';
import Gio from 'gi://Gio';

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

function fixtureMap(rows) {
    const cursors = new Map();
    for (const [name, size, hotX, hotY] of rows) {
        const records = cursors.get(name) || [];
        records.push({size, width: size, height: size, hotX, hotY});
        cursors.set(name, records);
    }
    return buildCursorShapeMap(cursors);
}

function assertThemeSignatures(theme, expected) {
    const cursors = loadCursorThemeData(theme, ['/usr/share/icons']);
    for (const [name, [hotX, hotY]] of Object.entries(expected)) {
        const record = (cursors.get(name) || []).find(cursor => cursor.size === 24);
        if (!record)
            throw new Error(`${theme}/${name} has no 24px Xcursor image`);
        assertEqual(record.hotX, hotX, `${theme}/${name} hotX`);
        assertEqual(record.hotY, hotY, `${theme}/${name} hotY`);
    }
}

function classifyNamedCursor(cursors, shapeMap, name) {
    const record = (cursors.get(name) || []).find(cursor => cursor.size === 24);
    if (!record)
        throw new Error(`${name} has no 24px Xcursor image`);
    return classifyCursorFromMap(24, 24, record.hotX, record.hotY, shapeMap);
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

runTest('classifies Adwaita and Yaru 24px cursor signatures', () => {
    const adwaita = fixtureMap([
        ['left_ptr', 24, 3, 1], ['move', 24, 3, 1], ['pointer', 24, 7, 5], ['xterm', 24, 11, 12],
        ['crosshair', 24, 11, 11], ['wait', 24, 11, 11], ['nwse-resize', 24, 11, 11],
        ['fleur', 24, 12, 11], ['ew-resize', 24, 12, 12], ['ns-resize', 24, 12, 13],
        ['grab', 24, 11, 2], ['grabbing', 24, 9, 5],
    ]);
    const yaru = fixtureMap([
        ['arrow', 24, 4, 4], ['pointer', 24, 8, 5], ['xterm', 24, 11, 12],
        ['crosshair', 24, 11, 11], ['wait', 24, 11, 11], ['tcross', 24, 12, 12],
        ['fleur', 24, 12, 11], ['move', 24, 12, 11],
        ['ew-resize', 24, 12, 12], ['ns-resize', 24, 12, 13], ['grab', 24, 11, 7],
    ]);

    for (const [shapeMap, cases] of [
        [adwaita, [[3, 1, 'default'], [7, 5, 'hand'], [11, 12, 'text'],
            [11, 11, 'crosshair'], [12, 11, 'move'], [12, 12, 'resize_h'],
            [12, 13, 'resize_v'], [11, 2, 'hand'], [9, 5, 'move']]],
        [yaru, [[4, 4, 'default'], [8, 5, 'hand'], [11, 12, 'text'],
            [11, 11, 'crosshair'], [12, 11, 'move'], [12, 12, 'resize_h'],
            [12, 13, 'resize_v'], [11, 7, 'hand']]],
    ]) {
        for (const [hotX, hotY, expected] of cases) {
            assertEqual(
                classifyCursorFromMap(24, 24, hotX, hotY, shapeMap),
                expected,
                `${hotX},${hotY}`);
        }
    }
});

runTest('normalizes a 48px sprite against 24px theme signatures', () => {
    const shapeMap = fixtureMap([['xterm', 24, 11, 12]]);
    assertEqual(classifyCursorFromMap(48, 48, 22, 24, shapeMap), 'text', 'scaled text');

    const mixedSizes = fixtureMap([
        ['xterm', 24, 11, 12], ['fleur', 48, 24, 22],
    ]);
    mixedSizes.cursorSize = 24;
    assertEqual(classifyCursorFromMap(48, 48, 22, 24, mixedSizes), 'text', 'scaled text with unrelated 48px signatures');
});

runTest('tries another nominal size when the nearest size lacks a signature', () => {
    const shapeMap = fixtureMap([
        ['xterm', 24, 11, 12], ['fleur', 48, 24, 22],
    ]);
    assertEqual(classifyCursorFromMap(40, 40, 18, 20, shapeMap), 'text', 'available 24px signature');
});

runTest('resolves ambiguous signatures using shape priority', () => {
    const shapeMap = fixtureMap([
        ['wait', 24, 11, 11], ['nwse-resize', 24, 11, 11], ['crosshair', 24, 11, 11],
    ]);
    assertEqual(classifyCursorFromMap(24, 24, 11, 11, shapeMap), 'crosshair', 'priority');
});

runTest('falls back to geometry when no theme signature is available', () => {
    assertEqual(classifyCursorFromMap(24, 24, 11, 12, {map: new Map(), sizes: []}), 'text', 'unknown theme');
});

runTest('rejects invalid geometry before consulting theme signatures', () => {
    const shapeMap = fixtureMap([['xterm', 24, 11, 12]]);
    assertEqual(classifyCursorFromMap(24, 24, 24, 12, shapeMap), 'default', 'out-of-range hotspot');
});

runTest('parses installed Adwaita and Yaru cursor signatures', () => {
    const themes = [
        ['Adwaita', {
            default: [3, 1], left_ptr: [3, 1], pointer: [7, 5], xterm: [11, 12],
            crosshair: [11, 11], cross: [11, 11], 'all-scroll': [11, 11], cell: [11, 11],
            wait: [11, 11], 'nesw-resize': [11, 11], 'nwse-resize': [11, 11], tcross: [11, 11],
            fleur: [12, 11], 'all-resize': [12, 11], 'vertical-text': [12, 11],
            'ew-resize': [12, 12], 'col-resize': [12, 12], 'not-allowed': [12, 12],
            'ns-resize': [12, 13], 'row-resize': [12, 13], grab: [11, 2], hand1: [11, 2],
            grabbing: [9, 5], help: [12, 21],
        }],
        ['Yaru', {
            default: [4, 4], arrow: [4, 4], left_ptr: [4, 4], pointer: [8, 5], hand: [8, 5], hand2: [8, 5],
            text: [11, 12], ibeam: [11, 12], xterm: [11, 12], crosshair: [11, 11],
            cross: [11, 11], 'all-scroll': [11, 11], move: [12, 11], fleur: [12, 11],
            grabbing: [12, 11], closedhand: [12, 11], 'ew-resize': [12, 12],
            'col-resize': [12, 12], 'ns-resize': [12, 13], 'row-resize': [12, 13],
            grab: [11, 7], hand1: [11, 7], wait: [11, 11],
        }],
    ];

    for (const [theme, expected] of themes) {
        const path = Gio.File.new_for_path(`/usr/share/icons/${theme}/cursors`);
        if (path.query_file_type(Gio.FileQueryInfoFlags.NONE, null) !== Gio.FileType.DIRECTORY) {
            print(`skip - installed ${theme} cursor tree is absent`);
            continue;
        }
        assertThemeSignatures(theme, expected);
        const cursors = loadCursorThemeData(theme, ['/usr/share/icons']);
        const shapeMap = buildCursorShapeMap(cursors);
        const expectedShapes = [
            ['left_ptr', 'default'], ['default', 'default'], ['arrow', 'default'],
            ['fleur', 'move'], ['text', 'text'], ['xterm', 'text'],
            ['crosshair', 'crosshair'], ['ew-resize', 'resize_h'], ['col-resize', 'resize_h'],
            ['ns-resize', 'resize_v'], ['row-resize', 'resize_v'], ['pointer', 'hand'],
            ['hand2', 'hand'], ['help', 'default'], ['question_arrow', 'default'],
            ['grabbing', 'move'],
        ];
        for (const [name, shape] of expectedShapes) {
            assertEqual(classifyNamedCursor(cursors, shapeMap, name), shape, `${theme}/${name}`);
        }

        if (theme === 'Adwaita')
            assertEqual(classifyNamedCursor(cursors, shapeMap, 'move'), 'default', 'Adwaita/move alias');
        else {
            assertEqual(classifyNamedCursor(cursors, shapeMap, 'move'), 'move', 'Yaru/move');
            assertEqual(classifyNamedCursor(cursors, shapeMap, 'wait'), 'crosshair', 'Yaru/wait collision');
            assertEqual(classifyNamedCursor(cursors, shapeMap, 'progress'), 'wait', 'Yaru/progress');
        }
    }
});

runTest('loads DMZ-Black and DMZ-White cursor data independently of index metadata', () => {
    for (const theme of ['DMZ-Black', 'DMZ-White']) {
        const path = Gio.File.new_for_path(`/usr/share/icons/${theme}/cursors`);
        if (path.query_file_type(Gio.FileQueryInfoFlags.NONE, null) !== Gio.FileType.DIRECTORY) {
            print(`skip - installed ${theme} cursor tree is absent`);
            continue;
        }

        const cursors = loadCursorThemeData(theme, ['/usr/share/icons']);
        if (cursors.size === 0)
            throw new Error(`${theme} did not load any cursor files`);
        const shapeMap = buildCursorShapeMap(cursors);
        for (const [name, shape] of [
            ['hand2', 'hand'], ['xterm', 'crosshair'], ['fleur', 'move'],
            ['left_ptr', 'default'], ['help', 'default'],
        ]) {
            assertEqual(classifyNamedCursor(cursors, shapeMap, name), shape, `${theme}/${name}`);
        }
    }
});

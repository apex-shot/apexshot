// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    applyAlwaysOnTop,
    findTrackedWindow,
    PreviewStacker,
} from '../preview-stacking.js';

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
    let nextSignalId = 1;
    const signalIds = new Set();
    const window = {
        above: false,
        minimized: false,
        connect() {
            const signalId = nextSignalId++;
            signalIds.add(signalId);
            return signalId;
        },
        disconnect(signalId) {
            signalIds.delete(signalId);
        },
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
        unmake_above() {
            calls.push('unmake_above');
            this.above = false;
        },
        raise() {
            calls.push('raise');
        },
        ...overrides,
    };
    window.calls = calls;
    return window;
}

function trackedWindow(overrides = {}) {
    return {
        get_title() {
            return 'ApexShot Preview';
        },
        get_pid() {
            return 100;
        },
        get_gtk_application_id() {
            return 'org.apexshot.ApexShot';
        },
        get_wm_class() {
            return null;
        },
        ...overrides,
    };
}

const previewIdentity = {
    pid: 200,
    title: 'ApexShot Preview',
    role: 'preview',
    namespace: 'apexshot-capture-preview',
    appId: 'org.apexshot.ApexShot',
};

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

runTest('capture UI registry tracks resolved preview and overlay, but not the editor', () => {
    const stacker = new PreviewStacker();
    const preview = fakeWindow();
    const captureOverlay = fakeWindow();
    const editor = fakeWindow();
    let resolvedWindow = null;
    stacker._findWindow = () => resolvedWindow;

    stacker._onOpened('preview', 100, 'ApexShot Preview', 'preview',
        'apexshot-capture-preview', 'io.github.codegoddy.apexshot');
    assertEqual(stacker.isCaptureUiWindow(preview), false,
        'a pending identity without its MetaWindow should not filter other windows');

    resolvedWindow = preview;
    stacker._resolvePending();
    assertEqual(stacker.isCaptureUiWindow(preview), true,
        'a resolved preview should be treated as transient capture UI');

    resolvedWindow = captureOverlay;
    stacker._onOpened('capture', 100, 'ApexShot Capture Overlay', 'capture-overlay',
        'screenshot', 'org.apexshot.ApexShot');
    assertEqual(stacker.isCaptureUiWindow(captureOverlay), true,
        'a resolved capture overlay should be treated as transient capture UI');

    resolvedWindow = editor;
    stacker._onOpened('editor', 100, 'ApexShot Editor', 'annotate-editor',
        'apexshot-annotate-editor', 'io.github.codegoddy.apexshot');
    assertEqual(stacker.isCaptureUiWindow(editor), false,
        'the regular editor role should not be treated as transient capture UI');

    stacker._onClosed('preview');
    assertEqual(stacker.isCaptureUiWindow(preview), true,
        'unpinning a preview should not make its live window selectable');
    stacker._onClosed('capture');
    assertEqual(stacker.isCaptureUiWindow(captureOverlay), true,
        'dropping a tracking event should not erase the live window identity');
    stacker._onClosed('editor');
});

runTest('capture UI registry includes unpinned previews and resets on disable', () => {
    const stacker = new PreviewStacker();
    const preview = fakeWindow();

    stacker.registerCaptureUiWindow(preview);
    assertEqual(stacker.isCaptureUiWindow(preview), true,
        'a preview registered by quick-access placement should be hidden while unpinned');

    stacker.disable();
    assertEqual(stacker.isCaptureUiWindow(preview), false,
        'disabling the extension should reset the capture UI registry');
});

runTest('tracked Flatpak window matches app identity even when its PID differs', () => {
    const window = trackedWindow();
    assertEqual(findTrackedWindow([window], previewIdentity), window,
        'matching app identity and role should beat a sandbox PID mismatch');
});

runTest('tracked Flatpak window accepts the localized title carried by the event', () => {
    const title = 'Aperçu ApexShot';
    const window = trackedWindow({
        get_title() {
            return title;
        },
        get_pid() {
            return 100;
        },
    });
    assertEqual(findTrackedWindow([window], {...previewIdentity, title}), window,
        'the exact localized title should match when app identity agrees');
});

runTest('native tracked-window matching preserves its unique PID-first behavior', () => {
    const title = 'Aperçu ApexShot';
    const window = trackedWindow({
        get_title() {
            return title;
        },
        get_pid() {
            return previewIdentity.pid;
        },
    });
    assertEqual(findTrackedWindow([window], {...previewIdentity, appId: ''}), window,
        'a unique native PID match should not depend on an English title');
});

runTest('native tracking ignores misleading titles when the PID identifies the window', () => {
    const unrelatedTitle = trackedWindow({
        get_title() {
            return 'ApexShot Preview';
        },
        get_pid() {
            return previewIdentity.pid + 1;
        },
    });
    const nativeWindow = trackedWindow({
        get_title() {
            return 'ApexShot Editor';
        },
        get_pid() {
            return previewIdentity.pid;
        },
        get_gtk_application_id() {
            return null;
        },
        get_wm_class() {
            return null;
        },
    });

    assertEqual(findTrackedWindow([unrelatedTitle, nativeWindow], {
        ...previewIdentity,
        appId: '',
    }), nativeWindow, 'the native PID identity should beat a misleading title');
});

runTest('Flatpak tracking selects the matching app among same-title windows', () => {
    const wrongApp = trackedWindow({
        get_gtk_application_id() {
            return 'org.example.OtherApp';
        },
    });
    const flatpakWindow = trackedWindow();

    assertEqual(findTrackedWindow([wrongApp, flatpakWindow], previewIdentity), flatpakWindow,
        'the sandbox app ID should disambiguate same-title windows');
});

runTest('tracked window does not fall back to a same-title window from another app', () => {
    const wrongApp = trackedWindow({
        get_gtk_application_id() {
            return 'org.example.OtherApp';
        },
    });
    assertEqual(findTrackedWindow([wrongApp], previewIdentity), null,
        'a different application ID must not be matched by title alone');
});

runTest('tracked window title fallback requires a unique candidate without app identity', () => {
    const noIdentity = trackedWindow({
        get_gtk_application_id() {
            return null;
        },
        get_wm_class() {
            return null;
        },
    });
    assertEqual(findTrackedWindow([noIdentity], previewIdentity), noIdentity,
        'one exact title is an acceptable fallback when Mutter exposes no app ID');
    assertEqual(findTrackedWindow([noIdentity, noIdentity], previewIdentity), null,
        'duplicate title-only candidates must not match');
});

runTest('tracked window rejects an unexpected role or namespace', () => {
    assertEqual(findTrackedWindow([trackedWindow()], {
        ...previewIdentity,
        role: 'capture-overlay',
    }), null, 'role/title mismatches must not pin an unrelated window');
});

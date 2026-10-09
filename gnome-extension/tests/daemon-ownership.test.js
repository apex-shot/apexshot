// SPDX-License-Identifier: AGPL-3.0-or-later

import {
    daemonCanChangeResource,
    resourceBelongsToDaemon,
    SHELL_OVERLAY_BUS_NAMES,
    WINDOW_LIST_BUS_NAMES,
} from '../daemon-ownership.js';

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

runTest('legacy and Flatpak aliases share only the extension APIs', () => {
    assertEqual(SHELL_OVERLAY_BUS_NAMES.join(','),
        'org.apexshot.ShellOverlay,org.apexshot.ApexShot.ShellOverlay',
        'ShellOverlay owns its native and Flatpak names');
    assertEqual(WINDOW_LIST_BUS_NAMES.join(','),
        'org.apexshot.WindowList,org.apexshot.ApexShot.WindowList',
        'WindowList owns its native and Flatpak names');
});

runTest('interleaved native and Flatpak calls keep their resource ownership separate', () => {
    const nativeDaemon = 'org.apexshot.Daemon';
    const flatpakDaemon = 'org.apexshot.ApexShot.Daemon';
    let maskOwner = null;

    if (daemonCanChangeResource(maskOwner, flatpakDaemon))
        maskOwner = flatpakDaemon;
    if (daemonCanChangeResource(maskOwner, nativeDaemon))
        maskOwner = nativeDaemon;

    assertEqual(maskOwner, flatpakDaemon,
        'a native call cannot replace an interleaved Flatpak-owned mask');
    assertEqual(daemonCanChangeResource(maskOwner, flatpakDaemon), true,
        'the Flatpak owner can still update its mask');
});

runTest('daemon disappearance cleans only resources belonging to that daemon', () => {
    const nativeDaemon = 'org.apexshot.Daemon';
    const flatpakDaemon = 'org.apexshot.ApexShot.Daemon';
    let countdownOwner = flatpakDaemon;

    if (resourceBelongsToDaemon(countdownOwner, nativeDaemon))
        countdownOwner = null;
    assertEqual(countdownOwner, flatpakDaemon,
        'native daemon exit leaves Flatpak countdown intact');
    if (resourceBelongsToDaemon(countdownOwner, flatpakDaemon))
        countdownOwner = null;
    assertEqual(countdownOwner, null,
        'Flatpak daemon exit removes its countdown');
});

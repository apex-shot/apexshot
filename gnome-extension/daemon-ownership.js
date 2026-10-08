// SPDX-License-Identifier: AGPL-3.0-or-later

export const SHELL_OVERLAY_BUS_NAMES = Object.freeze([
    'org.apexshot.ShellOverlay',
    'org.apexshot.ApexShot.ShellOverlay',
]);

export const WINDOW_LIST_BUS_NAMES = Object.freeze([
    'org.apexshot.WindowList',
    'org.apexshot.ApexShot.WindowList',
]);

export function resourceBelongsToDaemon(owner, daemonName) {
    return owner === daemonName;
}

export function daemonCanChangeResource(owner, daemonName) {
    return owner === null || resourceBelongsToDaemon(owner, daemonName);
}

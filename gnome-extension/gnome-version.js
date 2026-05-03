// SPDX-License-Identifier: AGPL-3.0-or-later

import * as Config from "resource:///org/gnome/shell/misc/config.js";

const SHELL_VERSION = Config.PACKAGE_VERSION;

function shellVersionAtLeast(major) {
    const parts = SHELL_VERSION.split(".");
    const current = parseInt(parts[0], 10);
    return current >= major;
}

export function gnomeVersion() {
    return SHELL_VERSION;
}

export function shellVersionAtLeast50() {
    return shellVersionAtLeast(50);
}

// SPDX-License-Identifier: AGPL-3.0-or-later

import Clutter from "gi://Clutter";

export function shouldExcludeOverlayEvent(sessionState, actor) {
    const ownedActors = sessionState?.runtimeOverlayState?.selfOwnedActors ?? null;
    let current = actor ?? null;
    while (current) {
        if (current._apexshotSelfOwned || ownedActors?.has(current))
            return true;
        current = typeof current.get_parent === "function" ? current.get_parent() : null;
    }
    return false;
}

export function attachRuntimeOverlays(_sessionState) {
    // Click and keystroke runtime overlays were removed. Webcam overlay support
    // is handled by the native controls/session state path.
}

export function destroyRuntimeOverlays(sessionState) {
    const overlayState = sessionState?.runtimeOverlayState;
    if (!overlayState)
        return;
    if (overlayState.chrome) {
        try {
            overlayState.chrome.destroy();
        } catch (_) {}
        overlayState.chrome = null;
    }
}

export function updateRuntimeOverlaySnapshot(_sessionState) {
    // No click/keystroke actors remain to update.
}

export {Clutter};

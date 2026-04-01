// SPDX-License-Identifier: AGPL-3.0-or-later

// Screenshot lock is intentionally deprecated. ApexShot keeps the GNOME
// extension for preview stacking and recording support, but screenshot
// modality/topmost behavior is owned by the C++ overlay itself.
export class ScreenshotLockController {
    constructor(_deps) {
    }

    isActive() {
        return false;
    }

    begin(_sessionId) {
        return false;
    }

    end() {
    }

    refreshGeometry() {
    }

    handleKeyPress(event) {
        return event?.eventPropagate ?? "propagate";
    }
}

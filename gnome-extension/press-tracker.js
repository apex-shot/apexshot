// SPDX-License-Identifier: AGPL-3.0-or-later

/// How far a held pointer must move, in logical pixels, to count as a drag.
///
/// Small enough that a deliberate drag is never missed, large enough that
/// pointer jitter during a plain click does not mark it as one.
export const DRAG_THRESHOLD_PX = 4;

/// Upper bound on the press intervals a single recording keeps.
export const MAX_PRESSES = 500;

/// Records mouse press intervals and whether each one became a drag.
///
/// Built from button state only — button identity and timing, never a key,
/// a keycode, or a typed character. A press closes when its button comes up,
/// or at the stop instant if the recording ends while it is still held, so an
/// open press is never silently dropped.
export class PressTracker {
    constructor() {
        this._open = new Map();
        this._presses = [];
    }

    /// Open an interval for `button` at recording time `t` (seconds).
    press(button, t, x, y) {
        if (this._open.has(button))
            return;
        this._open.set(button, {t, x, y, dragged: false});
    }

    /// Mark every open press as dragged once the pointer has left its start.
    move(x, y) {
        for (const press of this._open.values()) {
            if (Math.hypot(x - press.x, y - press.y) > DRAG_THRESHOLD_PX)
                press.dragged = true;
        }
    }

    /// Close the interval for `button` at `t`, keeping it as a drag if it moved.
    release(button, t) {
        const press = this._open.get(button);
        if (!press)
            return;
        this._open.delete(button);
        this._presses.push([press.t, t, button, press.dragged]);
        if (this._presses.length > MAX_PRESSES)
            this._presses.shift();
    }

    /// Close every still-held press at `t` (recording stopped mid-press).
    closeAll(t) {
        for (const button of [...this._open.keys()])
            this.release(button, t);
    }

    /// Hand back the recorded intervals and start clean for the next take.
    take() {
        const presses = this._presses.slice();
        this._presses = [];
        this._open.clear();
        return presses;
    }
}
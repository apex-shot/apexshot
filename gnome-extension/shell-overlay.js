// SPDX-License-Identifier: AGPL-3.0-or-later

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import Meta from 'gi://Meta';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {classifyCursorTracker} from './cursor-classifier.js';

const DBUS_NAME = 'org.apexshot.ShellOverlay';
const DBUS_PATH = '/org/apexshot/ShellOverlay';
const DAEMON_BUS_NAME = 'org.apexshot.Daemon';

const DBUS_INTERFACE = `
<node>
  <interface name="org.apexshot.ShellOverlay">
    <method name="ShowMask">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
    </method>
    <method name="HideMask"/>
    <method name="ShowCountdown">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
      <arg type="u" name="seconds" direction="in"/>
    </method>
    <method name="HideCountdown"/>
    <method name="StartPointerTrack"/>
    <method name="StopPointerTrack">
      <arg type="x" name="t0" direction="out"/>
      <arg type="a(diis)" name="samples" direction="out"/>
      <arg type="a(diii)" name="clicks" direction="out"/>
    </method>
    <method name="GetPointerSnapshot">
      <arg type="i" name="x" direction="out"/>
      <arg type="i" name="y" direction="out"/>
      <arg type="s" name="kind" direction="out"/>
      <arg type="b" name="valid" direction="out"/>
    </method>
  </interface>
</node>`;

const MASK_STYLE = 'background-color: rgba(0, 0, 0, 0.55);';
const COUNTDOWN_SIZE = 184;
const POINTER_POLL_MS = 16;
const MAX_POINTER_SAMPLES = 8000;
const COUNTDOWN_STYLE = 'background-color: rgba(0, 0, 0, 0.94); border-radius: 92px;';
const COUNTDOWN_LABEL_STYLE = 'color: white; font-size: 72px; font-weight: bold; font-family: Inter, Cantarell, sans-serif;';

/// Dims everything outside the area ApexShot is recording.
///
/// The mask is four plain widgets (above, left, right, below the capture
/// rect) parented to `global.window_group`, so it dims windows without
/// covering the shell chrome.
export class ShellOverlayService {
    constructor() {
        this._dbus = null;
        this._nameId = 0;
        this._daemonWatchId = 0;
        this._monitorsChangedId = 0;
        this._maskGroup = null;
        this._rect = null;
        this._countdown = null;
        this._countdownTimerId = 0;
        this._tracking = false;
        this._t0 = 0;
        this._samples = [];
        this._clicks = [];
        this._pollId = 0;
        this._tracker = null;
        this._cursorChangedId = 0;
        this._buttonPressId = 0;
        this._cursorKind = 'default';
        this._x = 0;
        this._y = 0;
        this._modifiers = 0;
        this._buttonMask = 0;
    }

    enable() {
        this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS_INTERFACE, this);
        this._dbus.export(Gio.DBus.session, DBUS_PATH);

        this._nameId = Gio.DBus.session.own_name(
            DBUS_NAME,
            Gio.BusNameOwnerFlags.REPLACE,
            null,
            null);

        this._monitorsChangedId = Main.layoutManager.connect('monitors-changed',
            () => this._redraw());

        this._daemonWatchId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            DAEMON_BUS_NAME,
            Gio.BusNameWatcherFlags.NONE,
            () => {},
            () => {
                this.HideMask();
                this.HideCountdown();
                this._stopPointerTrackInternal(false);
            });
    }

    disable() {
        if (this._daemonWatchId) {
            Gio.bus_unwatch_name(this._daemonWatchId);
            this._daemonWatchId = 0;
        }
        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }

        this._stopPointerTrackInternal(false);
        this._destroyMask();
        this._destroyCountdown();

        if (this._nameId) {
            Gio.DBus.session.unown_name(this._nameId);
            this._nameId = 0;
        }

        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
    }

    ShowMask(x, y, width, height) {
        if (width <= 0 || height <= 0) {
            this.HideMask();
            return;
        }

        this._rect = {x, y, width, height};
        this._redraw();
    }

    HideMask() {
        this._rect = null;
        this._destroyMask();
    }

    ShowCountdown(x, y, width, height, seconds) {
        this._destroyCountdown();
        if (width <= 0 || height <= 0 || seconds <= 0)
            return;

        let remaining = seconds;
        const label = new St.Label({
            text: `${remaining}`,
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
            style: COUNTDOWN_LABEL_STYLE,
        });
        this._countdown = new St.Bin({
            reactive: false,
            x: Math.round(x + width / 2 - COUNTDOWN_SIZE / 2),
            y: Math.round(y + height / 2 - COUNTDOWN_SIZE / 2),
            width: COUNTDOWN_SIZE,
            height: COUNTDOWN_SIZE,
            style: COUNTDOWN_STYLE,
        });
        this._countdown.set_child(label);
        global.window_group.add_child(this._countdown);

        this._countdownTimerId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            remaining--;
            if (remaining <= 0) {
                this._countdownTimerId = 0;
                this._destroyCountdown();
                return GLib.SOURCE_REMOVE;
            }
            label.text = `${remaining}`;
            return GLib.SOURCE_CONTINUE;
        });
    }

    HideCountdown() {
        this._destroyCountdown();
    }

    StartPointerTrack() {
        this._stopPointerTrackInternal(false);
        this._samples = [];
        this._clicks = [];
        this._t0 = GLib.get_monotonic_time();
        this._tracking = true;
        this._setupCursorTracking();
        this._readPointer();
        this._buttonMask = this._pressedButtonMask();
        this._setupClickTracking();
        this._samplePointer(true);
        this._pollId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, POINTER_POLL_MS, () => {
            if (!this._tracking)
                return GLib.SOURCE_REMOVE;
            this._samplePointer(false);
            return GLib.SOURCE_CONTINUE;
        });
    }

    StopPointerTrack() {
        return this._stopPointerTrackInternal(true);
    }

    GetPointerSnapshot() {
        this._readPointer();
        return [this._x, this._y, this._cursorKind, true];
    }

    _setupCursorTracking() {
        try {
            if (global.backend && typeof global.backend.get_cursor_tracker === 'function') {
                this._tracker = global.backend.get_cursor_tracker();
            } else if (Meta.CursorTracker && typeof Meta.CursorTracker.get_for_display === 'function') {
                this._tracker = Meta.CursorTracker.get_for_display(global.display);
            }
            if (this._tracker) {
                this._cursorChangedId = this._tracker.connect('cursor-changed', () => {
                    this._updateCursorKind();
                });
                this._updateCursorKind();
            }
        } catch (e) {
            log(`ApexShot: cursor tracker setup failed: ${e.message}`);
        }
    }

    _updateCursorKind() {
        this._cursorKind = classifyCursorTracker(this._tracker);
    }

    _setupClickTracking() {
        try {
            this._buttonPressId = global.stage.connect('captured-event', (_stage, event) => {
                if (!this._tracking)
                    return Clutter.EVENT_PROPAGATE;
                try {
                    if (event.type() !== Clutter.EventType.BUTTON_PRESS)
                        return Clutter.EVENT_PROPAGATE;
                    const button = event.get_button();
                    if (button < 1 || button > 3)
                        return Clutter.EVENT_PROPAGATE;
                    const [x, y] = event.get_coords();
                    const t = (GLib.get_monotonic_time() - this._t0) / 1_000_000;
                    this._recordClick(t, Math.floor(x), Math.floor(y), button);
                    this._buttonMask |= this._maskForButton(button);
                } catch (e) {
                    log(`ApexShot: click handler error: ${e.message}`);
                }
                return Clutter.EVENT_PROPAGATE;
            });
        } catch (e) {
            log(`ApexShot: click tracking setup failed: ${e.message}`);
        }
    }

    _readPointer() {
        try {
            const result = global.get_pointer();
            if (result && result.length >= 2) {
                this._x = Math.floor(result[0]);
                this._y = Math.floor(result[1]);
                this._modifiers = result.length >= 3 ? result[2] : 0;
            }
        } catch (e) {}
    }

    _maskForButton(button) {
        if (button === 1)
            return Clutter.ModifierType.BUTTON1_MASK;
        if (button === 2)
            return Clutter.ModifierType.BUTTON2_MASK;
        if (button === 3)
            return Clutter.ModifierType.BUTTON3_MASK;
        return 0;
    }

    _pressedButtonMask() {
        return [1, 2, 3].reduce((mask, button) => {
            const buttonMask = this._maskForButton(button);
            return (this._modifiers & buttonMask) !== 0 ? mask | buttonMask : mask;
        }, 0);
    }

    _recordClick(t, x, y, button) {
        const last = this._clicks.length > 0 ? this._clicks[this._clicks.length - 1] : null;
        if (last && last[3] === button && Math.abs(t - last[0]) < 0.03 &&
            Math.abs(x - last[1]) <= 2 && Math.abs(y - last[2]) <= 2)
            return;
        this._clicks.push([t, x, y, button]);
        if (this._clicks.length > 500)
            this._clicks.shift();
    }

    _sampleButtons(t) {
        const current = this._pressedButtonMask();
        const pressed = current & ~this._buttonMask;
        for (const button of [1, 2, 3]) {
            if ((pressed & this._maskForButton(button)) !== 0)
                this._recordClick(t, this._x, this._y, button);
        }
        this._buttonMask = current;
    }

    _samplePointer(force) {
        this._readPointer();
        const t = (GLib.get_monotonic_time() - this._t0) / 1_000_000;
        // Shell stage events do not include application windows on Wayland,
        // but the global pointer state includes button modifier masks.
        this._sampleButtons(t);
        const last = this._samples.length > 0 ? this._samples[this._samples.length - 1] : null;
        const still = last && last[1] === this._x && last[2] === this._y && last[3] === this._cursorKind;
        // Keep a still sample every 100ms so the editor can detect dwells.
        if (!force && still && (t - last[0]) < 0.1)
            return;
        this._samples.push([t, this._x, this._y, this._cursorKind]);
        if (this._samples.length >= MAX_POINTER_SAMPLES)
            this._compactPointerSamples();
    }

    _compactPointerSamples() {
        const lastIndex = this._samples.length - 1;
        const compacted = [this._samples[0]];
        for (let i = 2; i < lastIndex; i += 2)
            compacted.push(this._samples[i]);
        if (lastIndex > 0)
            compacted.push(this._samples[lastIndex]);
        this._samples = compacted;
    }

    _stopPointerTrackInternal(returnData) {
        if (this._tracking)
            this._samplePointer(true);
        this._tracking = false;
        if (this._pollId) {
            GLib.source_remove(this._pollId);
            this._pollId = 0;
        }
        if (this._cursorChangedId && this._tracker) {
            try {
                this._tracker.disconnect(this._cursorChangedId);
            } catch (e) {}
            this._cursorChangedId = 0;
        }
        this._tracker = null;
        if (this._buttonPressId) {
            try {
                global.stage.disconnect(this._buttonPressId);
            } catch (e) {}
            this._buttonPressId = 0;
        }
        const t0 = this._t0;
        const samples = this._samples.slice();
        const clicks = this._clicks.slice();
        this._samples = [];
        this._clicks = [];
        this._t0 = 0;
        this._modifiers = 0;
        this._buttonMask = 0;
        if (returnData)
            return [t0, samples, clicks];
        return [0, [], []];
    }

    _redraw() {
        if (!this._rect)
            return;

        const {x, y, width, height} = this._rect;
        const stageWidth = global.stage.width;
        const stageHeight = global.stage.height;

        const left = Math.max(0, Math.min(x, stageWidth));
        const top = Math.max(0, Math.min(y, stageHeight));
        const right = Math.max(left, Math.min(x + width, stageWidth));
        const bottom = Math.max(top, Math.min(y + height, stageHeight));

        if (!this._maskGroup) {
            this._maskGroup = new St.Widget({reactive: false});
            global.window_group.add_child(this._maskGroup);
        }

        this._maskGroup.remove_all_children();
        this._maskGroup.set_position(0, 0);
        this._maskGroup.set_size(stageWidth, stageHeight);

        const bands = [
            [0, 0, stageWidth, top],
            [0, top, left, bottom - top],
            [right, top, stageWidth - right, bottom - top],
            [0, bottom, stageWidth, stageHeight - bottom],
        ];

        for (const [bandX, bandY, bandWidth, bandHeight] of bands) {
            if (bandWidth <= 0 || bandHeight <= 0)
                continue;

            this._maskGroup.add_child(new St.Widget({
                reactive: false,
                x: bandX,
                y: bandY,
                width: bandWidth,
                height: bandHeight,
                style: MASK_STYLE,
            }));
        }
    }

    _destroyMask() {
        if (!this._maskGroup)
            return;

        this._maskGroup.destroy();
        this._maskGroup = null;
    }

    _destroyCountdown() {
        if (this._countdownTimerId) {
            GLib.source_remove(this._countdownTimerId);
            this._countdownTimerId = 0;
        }
        if (this._countdown) {
            this._countdown.destroy();
            this._countdown = null;
        }
    }
}

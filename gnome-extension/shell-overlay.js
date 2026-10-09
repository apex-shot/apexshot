// SPDX-License-Identifier: AGPL-3.0-or-later

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import Meta from 'gi://Meta';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {classifyCursorTracker} from './cursor-classifier.js';
import {
    daemonCanChangeResource,
    resourceBelongsToDaemon,
    SHELL_OVERLAY_BUS_NAMES,
} from './daemon-ownership.js';
import {PressTracker} from './press-tracker.js';

const DBUS_NAMES = SHELL_OVERLAY_BUS_NAMES;
const DBUS_PATH = '/org/apexshot/ShellOverlay';
const NATIVE_DAEMON_BUS_NAME = 'org.apexshot.Daemon';
const FLATPAK_DAEMON_BUS_NAME = 'org.apexshot.ApexShot.Daemon';
const DAEMON_BUS_NAMES = [NATIVE_DAEMON_BUS_NAME, FLATPAK_DAEMON_BUS_NAME];

const DBUS_INTERFACE = `
<node>
  <interface name="org.apexshot.ShellOverlay">
    <method name="ShowMaskV2">
      <arg type="s" name="daemon_name" direction="in"/>
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
    </method>
    <method name="ShowMask">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
    </method>
    <method name="HideMask"/>
    <method name="HideMaskV2">
      <arg type="s" name="daemon_name" direction="in"/>
    </method>
    <method name="ShowCountdownV2">
      <arg type="s" name="daemon_name" direction="in"/>
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
      <arg type="u" name="seconds" direction="in"/>
    </method>
    <method name="HideCountdownV2">
      <arg type="s" name="daemon_name" direction="in"/>
    </method>
    <method name="ShowCountdown">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
      <arg type="u" name="seconds" direction="in"/>
    </method>
    <method name="HideCountdown"/>
    <method name="ShowCaptureCountdown">
      <arg type="i" name="monitor_x" direction="in"/>
      <arg type="i" name="monitor_y" direction="in"/>
      <arg type="i" name="monitor_width" direction="in"/>
      <arg type="u" name="seconds" direction="in"/>
      <arg type="i" name="fade_x" direction="in"/>
      <arg type="i" name="fade_y" direction="in"/>
      <arg type="i" name="fade_width" direction="in"/>
      <arg type="i" name="fade_height" direction="in"/>
    </method>
    <method name="ShowCaptureCountdownV2">
      <arg type="s" name="daemon_name" direction="in"/>
      <arg type="i" name="monitor_x" direction="in"/>
      <arg type="i" name="monitor_y" direction="in"/>
      <arg type="i" name="monitor_width" direction="in"/>
      <arg type="u" name="seconds" direction="in"/>
      <arg type="i" name="fade_x" direction="in"/>
      <arg type="i" name="fade_y" direction="in"/>
      <arg type="i" name="fade_width" direction="in"/>
      <arg type="i" name="fade_height" direction="in"/>
    </method>
    <method name="FocusCaptureMenu">
      <arg type="x" name="pid" direction="in"/>
      <arg type="b" name="focused" direction="out"/>
    </method>
    <method name="FocusCaptureMenuV2">
      <arg type="s" name="app_id" direction="in"/>
      <arg type="s" name="window_title" direction="in"/>
      <arg type="x" name="pid" direction="in"/>
      <arg type="b" name="focused" direction="out"/>
    </method>
    <method name="PositionQuickAccess">
      <arg type="x" name="pid" direction="in"/>
      <arg type="i" name="monitor_x" direction="in"/>
      <arg type="i" name="monitor_y" direction="in"/>
    </method>
    <method name="PositionQuickAccessV2">
      <arg type="s" name="app_id" direction="in"/>
      <arg type="s" name="window_title" direction="in"/>
      <arg type="x" name="pid" direction="in"/>
      <arg type="i" name="monitor_x" direction="in"/>
      <arg type="i" name="monitor_y" direction="in"/>
    </method>
    <method name="RegisterQuickAccessV2">
      <arg type="s" name="app_id" direction="in"/>
      <arg type="s" name="window_title" direction="in"/>
      <arg type="x" name="pid" direction="in"/>
    </method>
    <method name="RegisterCaptureOverlayV2">
      <arg type="s" name="app_id" direction="in"/>
      <arg type="s" name="window_title" direction="in"/>
    </method>
    <method name="StartPointerTrack"/>
    <method name="StartPointerTrackV3">
      <arg type="s" name="daemon_name" direction="in"/>
    </method>
    <method name="StopPointerTrack">
      <arg type="x" name="t0" direction="out"/>
      <arg type="a(diis)" name="samples" direction="out"/>
      <arg type="a(diii)" name="clicks" direction="out"/>
    </method>
    <method name="StopPointerTrackV2">
      <arg type="x" name="t0" direction="out"/>
      <arg type="a(diis)" name="samples" direction="out"/>
      <arg type="a(diii)" name="clicks" direction="out"/>
      <arg type="a(ddib)" name="presses" direction="out"/>
    </method>
    <method name="StopPointerTrackV3">
      <arg type="s" name="daemon_name" direction="in"/>
      <arg type="x" name="t0" direction="out"/>
      <arg type="a(diis)" name="samples" direction="out"/>
      <arg type="a(diii)" name="clicks" direction="out"/>
      <arg type="a(ddib)" name="presses" direction="out"/>
    </method>
    <method name="GetPointerSnapshot">
      <arg type="i" name="x" direction="out"/>
      <arg type="i" name="y" direction="out"/>
      <arg type="s" name="kind" direction="out"/>
      <arg type="b" name="valid" direction="out"/>
    </method>
    <method name="GetMonitorGeometryAt">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="monitor_x" direction="out"/>
      <arg type="i" name="monitor_y" direction="out"/>
      <arg type="i" name="width" direction="out"/>
      <arg type="i" name="height" direction="out"/>
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
const CAPTURE_COUNTDOWN_STYLE = 'background-color: rgba(255, 102, 0, 0.95); border: 1px solid rgba(255, 224, 196, 0.5); border-radius: 22px;';
const CAPTURE_COUNTDOWN_LABEL_STYLE = 'color: white; font-size: 22px; font-weight: bold; font-family: Inter, Cantarell, sans-serif;';

/// Dims everything outside the area ApexShot is recording.
///
/// The mask is four plain widgets (above, left, right, below the capture
/// rect) parented to `global.window_group`, so it dims windows without
/// covering the shell chrome.
export class ShellOverlayService {
    constructor(previewStacker) {
        this._previewStacker = previewStacker;
        this._dbus = null;
        this._connection = null;
        this._nameIds = [];
        this._daemonWatchIds = new Map();
        this._monitorsChangedId = 0;
        this._maskGroup = null;
        this._rect = null;
        this._maskOwner = null;
        this._countdown = null;
        this._countdownOwner = null;
        this._captureFade = null;
        this._countdownTimerId = 0;
        this._tracking = false;
        this._pointerTrackOwner = null;
        this._t0 = 0;
        this._samples = [];
        this._clicks = [];
        this._pressTracker = new PressTracker();
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

    enable(connection) {
        this._connection = connection;
        this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS_INTERFACE, this);
        this._dbus.export(connection, DBUS_PATH);

        this._nameIds = DBUS_NAMES.map(name => connection.own_name(
            name,
            Gio.BusNameOwnerFlags.REPLACE,
            null,
            null));

        this._monitorsChangedId = Main.layoutManager.connect('monitors-changed',
            () => this._redraw());

        for (const daemonName of DAEMON_BUS_NAMES)
            this._watchDaemon(daemonName);
    }

    disable() {
        for (const watchId of this._daemonWatchIds.values())
            Gio.bus_unwatch_name(watchId);
        this._daemonWatchIds.clear();
        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }

        this._stopPointerTrackInternal(false);
        this._destroyMask();
        this._destroyCountdown();

        for (const nameId of this._nameIds)
            this._connection.unown_name(nameId);
        this._nameIds = [];

        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
        this._connection = null;
    }

    _watchDaemon(daemonName) {
        if (this._daemonWatchIds.has(daemonName))
            return;

        const watchId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            daemonName,
            Gio.BusNameWatcherFlags.NONE,
            () => {},
            () => this._cleanupDaemon(daemonName));
        this._daemonWatchIds.set(daemonName, watchId);
    }

    _cleanupDaemon(daemonName) {
        if (resourceBelongsToDaemon(this._maskOwner, daemonName)) {
            this._rect = null;
            this._maskOwner = null;
            this._destroyMask();
        }
        if (resourceBelongsToDaemon(this._countdownOwner, daemonName)) {
            this._countdownOwner = null;
            this._destroyCountdown();
        }
        if (resourceBelongsToDaemon(this._pointerTrackOwner, daemonName)) {
            this._pointerTrackOwner = null;
            this._stopPointerTrackInternal(false);
        }
    }

    _canControl(owner, daemonName) {
        return DAEMON_BUS_NAMES.includes(daemonName) && daemonCanChangeResource(owner, daemonName);
    }

    ShowMask(x, y, width, height) {
        this._showMask(NATIVE_DAEMON_BUS_NAME, x, y, width, height);
    }

    ShowMaskV2(daemonName, x, y, width, height) {
        this._showMask(daemonName, x, y, width, height);
    }

    _showMask(daemonName, x, y, width, height) {
        if (!this._canControl(this._maskOwner, daemonName))
            return;
        if (width <= 0 || height <= 0) {
            this._hideMask(daemonName);
            return;
        }

        this._rect = {x, y, width, height};
        this._maskOwner = daemonName;
        this._redraw();
    }

    HideMask() {
        this._hideMask(NATIVE_DAEMON_BUS_NAME);
    }

    HideMaskV2(daemonName) {
        this._hideMask(daemonName);
    }

    _hideMask(daemonName) {
        if (!this._canControl(this._maskOwner, daemonName))
            return;
        this._rect = null;
        this._maskOwner = null;
        this._destroyMask();
    }

    ShowCountdown(x, y, width, height, seconds) {
        this._showCountdown(NATIVE_DAEMON_BUS_NAME, x, y, width, height, seconds);
    }

    ShowCountdownV2(daemonName, x, y, width, height, seconds) {
        this._showCountdown(daemonName, x, y, width, height, seconds);
    }

    _showCountdown(daemonName, x, y, width, height, seconds) {
        if (!this._canControl(this._countdownOwner, daemonName))
            return;
        this._destroyCountdown();
        this._countdownOwner = daemonName;
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
        this._hideCountdown(NATIVE_DAEMON_BUS_NAME);
    }

    HideCountdownV2(daemonName) {
        this._hideCountdown(daemonName);
    }

    _hideCountdown(daemonName) {
        if (!this._canControl(this._countdownOwner, daemonName))
            return;
        this._countdownOwner = null;
        this._destroyCountdown();
    }

    FocusCaptureMenu(pid) {
        return this._focusCaptureMenu('', '', pid);
    }

    FocusCaptureMenuV2(appId, windowTitle, pid) {
        return this._focusCaptureMenu(appId, windowTitle, pid);
    }

    _focusCaptureMenu(appId, windowTitle, pid) {
        const allowedTitles = ['ApexShot Capture', 'ApexShot Display Picker'];
        if (windowTitle && !allowedTitles.includes(windowTitle))
            return false;
        const titles = windowTitle ? [windowTitle] : allowedTitles;
        const actor = this._findActor(titles, appId, pid);
        if (!actor)
            return false;

        const window = actor.meta_window;
        const workspace = global.workspace_manager.get_active_workspace();
        if (window.get_workspace() !== workspace)
            window.change_workspace(workspace);
        window.unminimize();
        window.make_above();
        Main.activateWindow(window, global.get_current_time());
        return true;
    }

    PositionQuickAccess(pid, monitorX, monitorY) {
        this._positionQuickAccess('', 'ApexShot Preview', pid, monitorX, monitorY);
    }

    PositionQuickAccessV2(appId, windowTitle, pid, monitorX, monitorY) {
        if (!windowTitle)
            return;
        this._positionQuickAccess(appId, windowTitle, pid, monitorX, monitorY);
    }

    RegisterQuickAccessV2(appId, windowTitle, pid) {
        if (!appId || !windowTitle)
            return;
        this._positionQuickAccess(appId, windowTitle, pid, null, null);
    }

    RegisterCaptureOverlayV2(appId, windowTitle) {
        if (!this._isFlatpakAppId(appId) || windowTitle !== 'ApexShot Capture Overlay')
            return;

        let attempts = 0;
        const register = () => {
            const windows = global.get_window_actors()
                .map(actor => actor.meta_window)
                .filter(window => window && window.get_title() === windowTitle &&
                    this._windowHasAppId(window, appId));
            if (windows.length === 0) {
                attempts++;
                return attempts < 20 ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
            }

            for (const window of windows)
                this._previewStacker.registerCaptureUiWindow(window);
            return GLib.SOURCE_REMOVE;
        };

        if (register() === GLib.SOURCE_CONTINUE)
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, register);
    }

    _findActor(titles, appId, pid) {
        const candidates = global.get_window_actors().filter(candidate => {
            const window = candidate.meta_window;
            return window && titles.includes(window.get_title());
        });
        if (appId) {
            const byAppId = candidates.filter(candidate => {
                const window = candidate.meta_window;
                return this._windowHasAppId(window, appId);
            });
            if (byAppId.length === 1)
                return byAppId[0];
            if (byAppId.length > 1 && !this._isFlatpakAppId(appId)) {
                const byPid = byAppId.filter(candidate => candidate.meta_window.get_pid() === pid);
                return byPid.length === 1 ? byPid[0] : null;
            }
            return null;
        }

        return candidates.find(candidate => candidate.meta_window.get_pid() === pid) ?? null;
    }

    _windowHasAppId(window, appId) {
        return this._windowApplicationIds(window).includes(appId);
    }

    _windowApplicationIds(window) {
        return [
            typeof window.get_gtk_application_id === 'function'
                ? window.get_gtk_application_id()
                : null,
            typeof window.get_wm_class === 'function' ? window.get_wm_class() : null,
        ].filter(Boolean);
    }

    _isFlatpakAppId(appId) {
        return appId === 'org.apexshot.ApexShot' || appId === 'org.apexshot.ApexShot.Dev';
    }

    _positionQuickAccess(appId, windowTitle, pid, monitorX, monitorY) {
        let attempts = 0;
        const position = () => {
            const actor = this._findActor([windowTitle], appId, pid);
            if (!actor) {
                attempts++;
                return attempts < 20 ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
            }

            const window = actor.meta_window;
            this._previewStacker.registerCaptureUiWindow(window);
            if (monitorX === null || monitorY === null)
                return GLib.SOURCE_REMOVE;
            window.move_frame(true, monitorX, monitorY);
            window.make_above();
            if (typeof window.raise === 'function')
                window.raise();
            return GLib.SOURCE_REMOVE;
        };

        if (position() === GLib.SOURCE_CONTINUE)
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, position);
    }

    ShowCaptureCountdown(monitorX, monitorY, monitorWidth, seconds,
        fadeX, fadeY, fadeWidth, fadeHeight) {
        this._showCaptureCountdown(NATIVE_DAEMON_BUS_NAME,
            monitorX, monitorY, monitorWidth, seconds, fadeX, fadeY, fadeWidth, fadeHeight);
    }

    ShowCaptureCountdownV2(daemonName, monitorX, monitorY, monitorWidth, seconds,
        fadeX, fadeY, fadeWidth, fadeHeight) {
        this._showCaptureCountdown(daemonName,
            monitorX, monitorY, monitorWidth, seconds, fadeX, fadeY, fadeWidth, fadeHeight);
    }

    _showCaptureCountdown(daemonName, monitorX, monitorY, monitorWidth, seconds,
        fadeX, fadeY, fadeWidth, fadeHeight) {
        if (!this._canControl(this._countdownOwner, daemonName))
            return;
        this._destroyCountdown();
        this._countdownOwner = daemonName;
        if (monitorWidth <= 0 || seconds <= 0)
            return;

        let remaining = seconds;
        const label = new St.Label({
            text: `${remaining}`,
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
            style: CAPTURE_COUNTDOWN_LABEL_STYLE,
        });
        this._countdown = new St.Bin({
            reactive: false,
            can_focus: false,
            x: Math.round(monitorX + monitorWidth / 2 - 59),
            y: Math.round(monitorY + 28),
            width: 118,
            height: 45,
            style: CAPTURE_COUNTDOWN_STYLE,
        });
        this._countdown.set_child(label);

        if (fadeWidth > 0 && fadeHeight > 0) {
            this._captureFade = new St.Widget({
                reactive: false,
                can_focus: false,
                x: fadeX,
                y: fadeY,
                width: fadeWidth,
                height: fadeHeight,
                style: 'background-color: rgba(12, 12, 14, 0.30);',
            });
            Main.layoutManager.addTopChrome(this._captureFade, {
                trackFullscreen: true,
            });
        }
        Main.layoutManager.addTopChrome(this._countdown, {
            trackFullscreen: true,
        });

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

    StartPointerTrack() {
        this._startPointerTrack(NATIVE_DAEMON_BUS_NAME);
    }

    StartPointerTrackV3(daemonName) {
        this._startPointerTrack(daemonName);
    }

    _startPointerTrack(daemonName) {
        if (!DAEMON_BUS_NAMES.includes(daemonName) ||
            !this._canControl(this._pointerTrackOwner, daemonName))
            return;
        this._stopPointerTrackInternal(false);
        this._pointerTrackOwner = daemonName;
        this._samples = [];
        this._clicks = [];
        this._pressTracker = new PressTracker();
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
        return this._stopPointerTrack(NATIVE_DAEMON_BUS_NAME, false);
    }

    StopPointerTrackV2() {
        return this._stopPointerTrack(NATIVE_DAEMON_BUS_NAME, true);
    }

    StopPointerTrackV3(daemonName) {
        return this._stopPointerTrack(daemonName, true);
    }

    _stopPointerTrack(daemonName, withPresses) {
        if (!DAEMON_BUS_NAMES.includes(daemonName) ||
            !this._canControl(this._pointerTrackOwner, daemonName))
            return withPresses ? [0, [], [], []] : [0, [], []];
        const result = this._stopPointerTrackInternal(true);
        this._pointerTrackOwner = null;
        return withPresses ? result : result.slice(0, 3);
    }

    GetPointerSnapshot() {
        this._readPointer();
        return [this._x, this._y, this._cursorKind, true];
    }

    GetMonitorGeometryAt(x, y) {
        const monitors = Main.layoutManager.monitors ?? [];
        const monitor = monitors.find(item =>
            x >= item.x && x < item.x + item.width &&
            y >= item.y && y < item.y + item.height);
        if (!monitor)
            return [0, 0, 0, 0, false];
        return [monitor.x, monitor.y, monitor.width, monitor.height, true];
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
                    const type = event.type();
                    if (type !== Clutter.EventType.BUTTON_PRESS &&
                        type !== Clutter.EventType.BUTTON_RELEASE)
                        return Clutter.EVENT_PROPAGATE;
                    const button = event.get_button();
                    if (button < 1 || button > 3)
                        return Clutter.EVENT_PROPAGATE;
                    const t = (GLib.get_monotonic_time() - this._t0) / 1_000_000;
                    if (type === Clutter.EventType.BUTTON_PRESS) {
                        const [x, y] = event.get_coords();
                        this._notePress(t, Math.floor(x), Math.floor(y), button);
                        this._buttonMask |= this._maskForButton(button);
                    } else {
                        this._noteRelease(t, button);
                        this._buttonMask &= ~this._maskForButton(button);
                    }
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

    /// Record a button going down: the click point and the press interval.
    _notePress(t, x, y, button) {
        this._recordClick(t, x, y, button);
        this._pressTracker.press(button, t, x, y);
    }

    /// Record a button coming up, closing its press interval.
    _noteRelease(t, button) {
        this._pressTracker.release(button, t);
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
        const released = this._buttonMask & ~current;
        for (const button of [1, 2, 3]) {
            const mask = this._maskForButton(button);
            if ((pressed & mask) !== 0)
                this._notePress(t, this._x, this._y, button);
            if ((released & mask) !== 0)
                this._noteRelease(t, button);
        }
        this._buttonMask = current;
    }

    _samplePointer(force) {
        this._readPointer();
        this._pressTracker.move(this._x, this._y);
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
        if (this._tracking) {
            this._samplePointer(true);
            const t = (GLib.get_monotonic_time() - this._t0) / 1_000_000;
            this._pressTracker.closeAll(t);
        }
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
        const presses = this._pressTracker.take();
        this._samples = [];
        this._clicks = [];
        this._t0 = 0;
        this._modifiers = 0;
        this._buttonMask = 0;
        if (returnData)
            return [t0, samples, clicks, presses];
        return [0, [], [], []];
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
        this._countdownOwner = null;
        if (this._countdownTimerId) {
            GLib.source_remove(this._countdownTimerId);
            this._countdownTimerId = 0;
        }
        if (this._countdown) {
            this._countdown.destroy();
            this._countdown = null;
        }
        if (this._captureFade) {
            this._captureFade.destroy();
            this._captureFade = null;
        }
    }
}

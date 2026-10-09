// SPDX-License-Identifier: AGPL-3.0-or-later

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import {WINDOW_LIST_BUS_NAMES} from './daemon-ownership.js';

const DBUS_NAMES = WINDOW_LIST_BUS_NAMES;
const DBUS_PATH = '/org/apexshot/WindowList';

const DBUS_INTERFACE = `
<node>
  <interface name="org.apexshot.WindowList">
    <method name="GetWindows">
      <arg type="s" name="windows_json" direction="out"/>
    </method>
    <method name="ActivateWindowById">
      <arg type="u" name="window_id" direction="in"/>
      <arg type="b" name="success" direction="out"/>
    </method>
  </interface>
</node>`;

const RECORDING_OVERLAY_CLASSES = ['com.apexshot.recording'];

function isRecordingOverlayWindow(wmClass) {
    return RECORDING_OVERLAY_CLASSES.includes(wmClass.toLowerCase());
}

/// Serializes the given window records for ApexShot's window picker.
///
/// Tracked capture UI and windows outside the normal window list (docks,
/// panels) are dropped, and sizes are clamped so cards are always layout-safe.
export function buildWindowListPayload(windows) {
    return windows
        .filter(window =>
            Number.isFinite(window.id) && !window.skipTaskbar &&
            !isRecordingOverlayWindow(window.wmClass) &&
            !window.captureUi)
        .map(window => ({
            id: Math.trunc(window.id),
            title: window.title || 'Window',
            app: window.app || window.title || 'Window',
            x: Math.trunc(window.x),
            y: Math.trunc(window.y),
            width: Math.max(1, Math.trunc(window.width)),
            height: Math.max(1, Math.trunc(window.height)),
            minimized: window.minimized,
        }));
}

/// Restores and focuses a window the user picked in ApexShot.
export function activateWindowRecord(metaWindow, timestamp) {
    if (!metaWindow)
        return false;

    if (metaWindow.minimized)
        metaWindow.unminimize();

    metaWindow.activate(timestamp);
    return true;
}

/// Lets ApexShot enumerate and focus windows, which a Wayland client cannot do
/// for itself. Metadata only — no window contents are read or sent.
export class WindowListService {
    constructor(previewStacker) {
        this._previewStacker = previewStacker;
        this._dbus = null;
        this._connection = null;
        this._nameIds = [];
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
    }

    disable() {
        for (const nameId of this._nameIds)
            this._connection.unown_name(nameId);
        this._nameIds = [];

        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
        this._connection = null;
    }

    GetWindows() {
        return JSON.stringify(buildWindowListPayload(this._listWindows()));
    }

    ActivateWindowById(windowId) {
        const record = this._listWindows()
            .find(window => window.id === Math.trunc(windowId));

        return activateWindowRecord(record?.metaWindow ?? null,
            global.get_current_time());
    }

    /// Windows from every workspace, so the picker does not hide windows that
    /// merely sit on another workspace.
    _listWindows() {
        const workspaceManager = global.workspace_manager;
        const tracker = Shell.WindowTracker.get_default();
        const records = new Map();

        for (let index = 0; index < workspaceManager.get_n_workspaces(); index++) {
            const workspace = workspaceManager.get_workspace_by_index(index);
            const windows = global.display.get_tab_list(Meta.TabList.NORMAL_ALL, workspace);

            for (const metaWindow of windows) {
                const id = metaWindow.get_id();
                if (records.has(id))
                    continue;

                const frame = metaWindow.get_frame_rect();
                const wmClass = metaWindow.get_wm_class() ?? '';
                const app = tracker.get_window_app(metaWindow);
                const appName = app ? app.get_name() : wmClass;

                records.set(id, {
                    id,
                    title: metaWindow.get_title() ?? '',
                    app: appName,
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height,
                    minimized: metaWindow.minimized,
                    skipTaskbar: metaWindow.is_skip_taskbar(),
                    captureUi: this._previewStacker.isCaptureUiWindow(metaWindow),
                    wmClass,
                    metaWindow,
                });
            }
        }

        return [...records.values()];
    }
}

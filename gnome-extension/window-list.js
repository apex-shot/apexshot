// SPDX-License-Identifier: AGPL-3.0-or-later

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

const DBUS_NAME = 'org.apexshot.WindowList';
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
/// ApexShot's recording overlays and windows that are not in the window list
/// (docks, panels) are dropped, and sizes are clamped so the picker never has
/// to lay out a zero-sized card.
export function buildWindowListPayload(windows) {
    return windows
        .filter(window =>
            Number.isFinite(window.id) && !window.skipTaskbar &&
            !isRecordingOverlayWindow(window.wmClass))
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
    constructor() {
        this._dbus = null;
        this._nameId = 0;
    }

    enable() {
        this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS_INTERFACE, this);
        this._dbus.export(Gio.DBus.session, DBUS_PATH);

        this._nameId = Gio.DBus.session.own_name(
            DBUS_NAME,
            Gio.BusNameOwnerFlags.REPLACE,
            null,
            null);
    }

    disable() {
        if (this._nameId) {
            Gio.DBus.session.unown_name(this._nameId);
            this._nameId = 0;
        }

        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
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
                    wmClass,
                    metaWindow,
                });
            }
        }

        return [...records.values()];
    }
}

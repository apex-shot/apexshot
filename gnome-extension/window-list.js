// SPDX-License-Identifier: AGPL-3.0-or-later
// Fast metadata-only window list for ApexShot (no surface previews).

const WINDOW_LIST_DBUS_NAME = "org.apexshot.WindowList";
const WINDOW_LIST_DBUS_PATH = "/org/apexshot/WindowList";
const WINDOW_LIST_DBUS_IFACE_XML = `
<node>
  <interface name="org.apexshot.WindowList">
    <method name="GetWindows">
      <arg type="s" name="windows_json" direction="out"/>
    </method>
    <method name="CaptureWindowById">
      <arg type="u" name="window_id" direction="in"/>
      <arg type="s" name="filename" direction="in"/>
      <arg type="b" name="success" direction="out"/>
    </method>
    <method name="ActivateWindowById">
      <arg type="u" name="window_id" direction="in"/>
      <arg type="b" name="success" direction="out"/>
    </method>
  </interface>
</node>`;

function clampDimension(value) {
    return Math.max(1, Number.isFinite(value) ? Math.trunc(value) : 1);
}

function sanitizeText(value, fallback = "") {
    const text = typeof value === "string" ? value.trim() : "";
    return text || fallback;
}

function isApexShotWindowIdentity(appName, wmClass) {
    const app = sanitizeText(appName).toLowerCase();
    const klass = sanitizeText(wmClass).toLowerCase();
    return app === "apexshot"
        || klass === "io.github.codegoddy.apexshot"
        || klass === "apexshot"
        || klass === "com.apexshot.recording";
}

function normalizeWindowRecord(window) {
    if (!window)
        return null;

    const app = sanitizeText(window.app, sanitizeText(window.title, "Window"));
    const wmClass = sanitizeText(window.wmClass);
    return {
        ...window,
        app,
        wmClass,
        x: Number.isFinite(window.x) ? Math.trunc(window.x) : 0,
        y: Number.isFinite(window.y) ? Math.trunc(window.y) : 0,
        width: clampDimension(window.width),
        height: clampDimension(window.height),
        apexshot: isApexShotWindowIdentity(app, wmClass),
    };
}

function isEligibleWindowRecord(window) {
    if (!window)
        return false;
    if (window.skipTaskbar || window.apexshot)
        return false;
    if (!Number.isFinite(window.id))
        return false;
    return true;
}

export function buildWindowListPayload(windows) {
    return windows
        .map(normalizeWindowRecord)
        .filter(Boolean)
        .filter(isEligibleWindowRecord)
        .map(window => ({
            id: Math.trunc(window.id),
            title: sanitizeText(window.title, "Window"),
            app: window.app,
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
            minimized: Boolean(window.minimized),
            // Always empty — ApexShot uses a text list picker, not previews.
            thumbnail_b64: "",
        }));
}

function readFrameRect(metaWindow) {
    const rect = typeof metaWindow.get_frame_rect === "function"
        ? metaWindow.get_frame_rect()
        : null;
    return {
        x: rect?.x ?? 0,
        y: rect?.y ?? 0,
        width: rect?.width ?? 1,
        height: rect?.height ?? 1,
    };
}

export function activateWindowRecord(metaWindow, timestamp = 0) {
    if (!metaWindow)
        return false;

    try {
        const isMinimized = typeof metaWindow.minimized === "boolean"
            ? metaWindow.minimized
            : typeof metaWindow.get_minimized === "function"
                ? metaWindow.get_minimized()
                : false;
        if (isMinimized && typeof metaWindow.unminimize === "function")
            metaWindow.unminimize();

        if (typeof metaWindow.activate === "function") {
            metaWindow.activate(timestamp);
            return true;
        }
    } catch (e) {
        logError(e, "[apexshot] Failed to activate selected window");
    }

    return false;
}

function extractWindowRecord(metaWindow, windowTracker) {
    if (!metaWindow)
        return null;

    const app = windowTracker?.get_window_app?.(metaWindow) ?? null;
    const title = typeof metaWindow.get_title === "function" ? metaWindow.get_title() : "";
    const wmClass = typeof metaWindow.get_wm_class === "function" ? metaWindow.get_wm_class() : "";
    const appName = app?.get_name?.() ?? wmClass;
    const rect = readFrameRect(metaWindow);
    const minimized = typeof metaWindow.minimized === "boolean"
        ? metaWindow.minimized
        : typeof metaWindow.get_minimized === "function"
            ? metaWindow.get_minimized()
            : false;
    const skipTaskbar = typeof metaWindow.is_skip_taskbar === "function"
        ? metaWindow.is_skip_taskbar()
        : false;

    return {
        id: typeof metaWindow.get_id === "function" ? metaWindow.get_id() : NaN,
        title,
        app: appName,
        wmClass,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        minimized,
        skipTaskbar,
        apexshot: isApexShotWindowIdentity(appName, wmClass),
        _metaWindow: metaWindow,
    };
}

export class WindowListService {
    constructor({Gio, GLib, Meta, Shell, shellGlobal, windowTracker}) {
        this._Gio = Gio;
        this._GLib = GLib;
        this._Meta = Meta;
        this._Shell = Shell; // optional; kept for constructor compat
        this._global = shellGlobal;
        this._windowTracker = windowTracker;
        this._dbusObject = null;
        this._ownName = null;
    }

    enable() {
        this._dbusObject = this._Gio.DBusExportedObject.wrapJSObject(WINDOW_LIST_DBUS_IFACE_XML, this);
        try {
            this._dbusObject.export(this._Gio.DBus.session, WINDOW_LIST_DBUS_PATH);
        } catch (e) {
            logError(e, `Failed to export ${WINDOW_LIST_DBUS_PATH}`);
        }

        this._ownName = this._Gio.DBus.session.own_name(
            WINDOW_LIST_DBUS_NAME,
            this._Gio.BusNameOwnerFlags.NONE,
            null,
            () => {
                log(`[apexshot] Lost D-Bus name ${WINDOW_LIST_DBUS_NAME}`);
                this._ownName = null;
            }
        );
    }

    disable() {
        if (this._ownName !== null) {
            this._Gio.DBus.session.unown_name(this._ownName);
            this._ownName = null;
        }

        if (this._dbusObject) {
            try {
                this._dbusObject.unexport();
            } catch (e) {
                logError(e, `Failed to unexport ${WINDOW_LIST_DBUS_PATH}`);
            }
            this._dbusObject = null;
        }
    }

    GetWindowsAsync(_params, invocation) {
        try {
            // Sync, metadata-only — no per-window surface screenshots.
            const payload = buildWindowListPayload(this._listCurrentWindows());
            invocation.return_value(this._GLib.Variant.new("(s)", [JSON.stringify(payload)]));
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.WindowList.Error", e.message);
        }
    }

    CaptureWindowByIdAsync(params, invocation) {
        // Not used by the list picker; kept for D-Bus compatibility.
        try {
            const [windowId, filename] = params;
            void windowId;
            void filename;
            invocation.return_value(this._GLib.Variant.new("(b)", [false]));
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.WindowList.Error", e.message);
        }
    }

    ActivateWindowByIdAsync(params, invocation) {
        try {
            const [windowId] = params;
            const metaWindow = this._findMetaWindowById(windowId);
            const success = activateWindowRecord(metaWindow, this._global.get_current_time());
            invocation.return_value(this._GLib.Variant.new("(b)", [success]));
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.WindowList.Error", e.message);
        }
    }

    _listCurrentWindows() {
        // All workspaces so open windows are not dropped when they sit on
        // a non-active workspace.
        const manager = this._global.workspace_manager;
        const n = typeof manager.get_n_workspaces === "function"
            ? manager.get_n_workspaces()
            : 1;

        const seen = new Set();
        const result = [];

        for (let i = 0; i < n; i++) {
            const workspace = manager.get_workspace_by_index(i);
            if (!workspace)
                continue;
            const windows = this._global.display.get_tab_list(
                this._Meta.TabList.NORMAL_ALL,
                workspace
            );
            for (const metaWindow of windows) {
                const record = extractWindowRecord(metaWindow, this._windowTracker);
                if (!record || !Number.isFinite(record.id))
                    continue;
                if (seen.has(record.id))
                    continue;
                seen.add(record.id);
                result.push(record);
            }
        }

        return result;
    }

    _findMetaWindowById(windowId) {
        const id = Number(windowId);
        for (const record of this._listCurrentWindows()) {
            if (Math.trunc(record.id) === Math.trunc(id))
                return record._metaWindow ?? null;
        }
        return null;
    }
}

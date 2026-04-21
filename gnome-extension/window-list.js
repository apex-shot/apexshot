// SPDX-License-Identifier: AGPL-3.0-or-later

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
  </interface>
</node>`;

function clampDimension(value) {
    return Math.max(1, Number.isFinite(value) ? Math.trunc(value) : 1);
}

function sanitizeText(value, fallback = "") {
    const text = typeof value === "string" ? value.trim() : "";
    return text || fallback;
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
        apexshot: isApexShotWindowIdentity(app, wmClass),
    };
}

function isEligibleWindowRecord(window) {
    if (!window || !window.visible)
        return false;
    if (window.minimized || window.skipTaskbar || window.apexshot)
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
            x: Number.isFinite(window.x) ? Math.trunc(window.x) : 0,
            y: Number.isFinite(window.y) ? Math.trunc(window.y) : 0,
            width: clampDimension(window.width),
            height: clampDimension(window.height),
            thumbnail_b64: "",
        }));
}

function isApexShotWindowIdentity(appName, wmClass) {
    const app = sanitizeText(appName).toLowerCase();
    const klass = sanitizeText(wmClass).toLowerCase();
    return app === "apexshot"
        || klass === "io.github.codegoddy.apexshot"
        || klass === "apexshot"
        || klass === "com.apexshot.recording";
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

function extractWindowRecord(metaWindow, windowTracker) {
    if (!metaWindow)
        return null;

    const app = windowTracker?.get_window_app?.(metaWindow) ?? null;
    const title = typeof metaWindow.get_title === "function" ? metaWindow.get_title() : "";
    const wmClass = typeof metaWindow.get_wm_class === "function" ? metaWindow.get_wm_class() : "";
    const appName = app?.get_name?.() ?? wmClass;
    const rect = readFrameRect(metaWindow);
    const visible = typeof metaWindow.showing_on_its_workspace === "function"
        ? metaWindow.showing_on_its_workspace()
        : true;
    const minimized = typeof metaWindow.minimized === "boolean"
        ? metaWindow.minimized
        : typeof metaWindow.get_minimized === "function"
            ? metaWindow.get_minimized()
            : false;
    const skipTaskbar = typeof metaWindow.is_skip_taskbar === "function"
        ? metaWindow.is_skip_taskbar()
        : false;
    const hasActor = Boolean(metaWindow.get_compositor_private?.());

    return {
        id: typeof metaWindow.get_id === "function" ? metaWindow.get_id() : NaN,
        title,
        app: appName,
        wmClass,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        visible: visible && hasActor,
        minimized,
        skipTaskbar,
        hasActor,
        apexshot: isApexShotWindowIdentity(appName, wmClass),
    };
}

export class WindowListService {
    constructor({Gio, GLib, Meta, shellGlobal, windowTracker}) {
        this._Gio = Gio;
        this._GLib = GLib;
        this._Meta = Meta;
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
            const payload = buildWindowListPayload(this._listCurrentWindows());
            invocation.return_value(this._GLib.Variant.new("(s)", [JSON.stringify(payload)]));
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.WindowList.Error", e.message);
        }
    }

    CaptureWindowByIdAsync(params, invocation) {
        try {
            const [windowId, filename] = params;
            const target = this._listCurrentWindows().find(window => window.id === windowId);
            if (!target) {
                invocation.return_value(this._GLib.Variant.new("(b)", [false]));
                return;
            }

            this._Gio.DBus.session.call(
                "org.gnome.Shell.Screenshot",
                "/org/gnome/Shell/Screenshot",
                "org.gnome.Shell.Screenshot",
                "ScreenshotArea",
                this._GLib.Variant.new("(iiiibs)", [
                    target.x,
                    target.y,
                    target.width,
                    target.height,
                    false,
                    filename,
                ]),
                new this._GLib.VariantType("(bs)"),
                this._Gio.DBusCallFlags.NONE,
                -1,
                null,
                (_conn, result) => {
                    try {
                        const reply = this._Gio.DBus.session.call_finish(result);
                        const [success] = reply.deepUnpack();
                        invocation.return_value(this._GLib.Variant.new("(b)", [Boolean(success)]));
                    } catch (e) {
                        logError(e, "[apexshot] CaptureWindowById screenshot failed");
                        invocation.return_value(this._GLib.Variant.new("(b)", [false]));
                    }
                }
            );
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.WindowList.Error", e.message);
        }
    }

    _listCurrentWindows() {
        const workspace = this._global.workspace_manager.get_active_workspace();
        const windows = this._global.display.get_tab_list(this._Meta.TabList.NORMAL_ALL, workspace);
        return windows
            .map(window => extractWindowRecord(window, this._windowTracker))
            .filter(Boolean);
    }
}

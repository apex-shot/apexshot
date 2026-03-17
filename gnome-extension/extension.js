// SPDX-License-Identifier: AGPL-3.0-or-later
// ApexShot Preview Helper — D-Bus lifecycle tracking, per-preview watchdog

import Meta from "gi://Meta";
import Gio from "gi://Gio";
import GLib from "gi://GLib";

// D-Bus interface exposed by the apexshot native process.
// The extension listens for signals on this name/path/iface.
const DBUS_NAME      = "org.apexshot.Preview";
const DBUS_PATH      = "/org/apexshot/Preview";
const DBUS_IFACE     = "org.apexshot.Preview";

// Introspection XML so Gio can proxy the signal types.
const DBUS_IFACE_XML = `
<node>
  <interface name="org.apexshot.Preview">
    <signal name="PreviewOpened">
      <arg type="s" name="preview_id"/>
      <arg type="u" name="pid"/>
      <arg type="s" name="title"/>
      <arg type="s" name="namespace"/>
      <arg type="t" name="opened_at_ms"/>
    </signal>
    <signal name="PreviewClosed">
      <arg type="s" name="preview_id"/>
    </signal>
  </interface>
</node>`;

export default class ApexShotPreview {
    constructor() {
        // Map<preview_id, { pid, title, openedAtMs, window, signalIds }>
        this._trackedPreviews = new Map();

        // Map<preview_id, { pid, title, openedAtMs }>  — waiting for MetaWindow
        this._pendingPreviews = new Map();

        // GLib source id for the 50 ms watchdog (null when no tracked previews)
        this._watchdogSource = null;

        // GLib source id for the 50 ms pending-resolve loop
        this._resolveSource = null;

        // GNOME Shell signal IDs
        this._windowCreatedId = null;
        this._focusChangedId  = null;

        // Gio.DBusConnection subscription id
        this._dbusSubId = null;
        this._dbusConn  = null;
    }

    enable() {
        // Subscribe to D-Bus signals emitted by the Rust process.
        this._dbusConn = Gio.DBus.session;
        this._dbusSubId = this._dbusConn.signal_subscribe(
            null,          // sender — any (apexshot may not own the name)
            DBUS_IFACE,
            null,          // member — subscribe to all signals on the iface
            DBUS_PATH,
            null,
            Gio.DBusSignalFlags.NONE,
            (conn, sender, path, iface, signal, params) => {
                if (signal === "PreviewOpened") {
                    const [previewId, pid, title, namespace, openedAtMs] = params.recursiveUnpack();
                    this._onPreviewOpened(previewId, pid, title, openedAtMs);
                } else if (signal === "PreviewClosed") {
                    const [previewId] = params.recursiveUnpack();
                    this._onPreviewClosed(previewId);
                }
            }
        );

        // Also watch for new windows so we can match pending previews quickly.
        this._windowCreatedId = global.display.connect(
            "window-created",
            (_display, window) => this._onWindowCreated(window)
        );

        // Re-raise all tracked previews on focus changes.
        this._focusChangedId = global.display.connect(
            "notify::focus-window",
            () => this._onFocusChange()
        );
    }

    disable() {
        if (this._dbusConn && this._dbusSubId !== null) {
            this._dbusConn.signal_unsubscribe(this._dbusSubId);
        }
        this._dbusSubId = null;
        this._dbusConn  = null;

        if (this._windowCreatedId) {
            global.display.disconnect(this._windowCreatedId);
            this._windowCreatedId = null;
        }
        if (this._focusChangedId) {
            global.display.disconnect(this._focusChangedId);
            this._focusChangedId = null;
        }

        this._stopWatchdog();
        this._stopResolveLoop();

        this._trackedPreviews.clear();
        this._pendingPreviews.clear();
    }

    // -------------------------------------------------------------------------
    // D-Bus event handlers
    // -------------------------------------------------------------------------

    _onPreviewOpened(previewId, pid, title, openedAtMs) {
        if (this._trackedPreviews.has(previewId) || this._pendingPreviews.has(previewId)) {
            return; // already known
        }

        // Try to resolve the MetaWindow immediately.
        const win = this._findWindowByPid(pid, title);
        if (win) {
            this._bindPreview(previewId, pid, title, openedAtMs, win);
        } else {
            // Keep pending — resolve loop will pick it up.
            this._pendingPreviews.set(previewId, { pid, title, openedAtMs });
            this._startResolveLoop();
        }
    }

    _onPreviewClosed(previewId) {
        if (this._trackedPreviews.has(previewId)) {
            this._unbindPreview(previewId);
        } else {
            this._pendingPreviews.delete(previewId);
            if (this._pendingPreviews.size === 0) {
                this._stopResolveLoop();
            }
        }
    }

    // -------------------------------------------------------------------------
    // Window lifecycle
    // -------------------------------------------------------------------------

    _onWindowCreated(window) {
        if (!window) return;

        // Try to resolve pending previews against the new window immediately,
        // then again once the title is available.
        this._tryResolvePending();

        const sid = window.connect("notify::title", () => {
            window.disconnect(sid);
            this._tryResolvePending();
        });
    }

    _onFocusChange() {
        for (const [, data] of this._trackedPreviews) {
            if (data.window) this._applyAbove(data.window);
        }
    }

    // -------------------------------------------------------------------------
    // Bind / unbind a tracked preview
    // -------------------------------------------------------------------------

    _bindPreview(previewId, pid, title, openedAtMs, win) {
        const signalIds = [];

        signalIds.push(win.connect("notify::minimized", () => {
            if (!win.minimized) this._applyAbove(win);
        }));
        signalIds.push(win.connect("notify::hidden", () => {
            if (!win.is_hidden()) this._applyAbove(win);
        }));
        signalIds.push(win.connect("notify::layer", () => {
            this._applyAbove(win);
        }));
        // Detect when the compositor destroys the window (e.g. process crash).
        signalIds.push(win.connect("unmanaged", () => {
            this._onWindowUnmanaged(previewId);
        }));

        this._trackedPreviews.set(previewId, { pid, title, openedAtMs, window: win, signalIds });
        this._pendingPreviews.delete(previewId);

        this._applyAbove(win);
        this._startWatchdog();

        if (this._pendingPreviews.size === 0) {
            this._stopResolveLoop();
        }
    }

    _unbindPreview(previewId) {
        const data = this._trackedPreviews.get(previewId);
        if (!data) return;

        if (data.window) {
            for (const sid of data.signalIds) {
                try { data.window.disconnect(sid); } catch (_) {}
            }
        }
        this._trackedPreviews.delete(previewId);

        if (this._trackedPreviews.size === 0) {
            this._stopWatchdog();
        }
    }

    _onWindowUnmanaged(previewId) {
        // The compositor removed the window without a D-Bus PreviewClosed —
        // clean up stale tracking entry.
        this._unbindPreview(previewId);
    }

    // -------------------------------------------------------------------------
    // Pending resolve loop — only runs while pendingPreviews is non-empty
    // -------------------------------------------------------------------------

    _startResolveLoop() {
        if (this._resolveSource !== null) return;
        this._resolveSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
            this._tryResolvePending();
            if (this._pendingPreviews.size === 0) {
                this._resolveSource = null;
                return false; // stop
            }
            return true; // continue
        });
    }

    _stopResolveLoop() {
        if (this._resolveSource !== null) {
            GLib.source_remove(this._resolveSource);
            this._resolveSource = null;
        }
    }

    _tryResolvePending() {
        for (const [previewId, info] of this._pendingPreviews) {
            const win = this._findWindowByPid(info.pid, info.title);
            if (win) {
                this._bindPreview(previewId, info.pid, info.title, info.openedAtMs, win);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Watchdog — 50 ms, only while tracked previews exist
    // -------------------------------------------------------------------------

    _startWatchdog() {
        if (this._watchdogSource !== null) return;
        this._watchdogSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
            for (const [previewId, data] of this._trackedPreviews) {
                if (!data.window) continue;
                // Stale-window check: if the window is no longer managed,
                // clean it up rather than touching a dead object.
                try {
                    if (data.window.is_hidden && data.window.is_hidden() &&
                            !data.window.get_compositor_private()) {
                        this._onWindowUnmanaged(previewId);
                        continue;
                    }
                    this._applyAbove(data.window);
                } catch (_) {
                    this._onWindowUnmanaged(previewId);
                }
            }
            if (this._trackedPreviews.size === 0) {
                this._watchdogSource = null;
                return false; // stop
            }
            return true; // continue
        });
    }

    _stopWatchdog() {
        if (this._watchdogSource !== null) {
            GLib.source_remove(this._watchdogSource);
            this._watchdogSource = null;
        }
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /** Find a MetaWindow whose PID matches, falling back to title match. */
    _findWindowByPid(pid, title) {
        const actors = global.get_window_actors();
        const pidCandidates = [];
        const titleCandidates = [];

        for (const actor of actors) {
            const win = actor.get_meta_window();
            if (!win) continue;

            if (win.get_pid && win.get_pid() === pid) {
                pidCandidates.push(win);
            } else if (win.get_title && win.get_title() === title) {
                titleCandidates.push(win);
            }
        }

        // Prefer exact PID match; if exactly one, bind it.
        if (pidCandidates.length === 1) return pidCandidates[0];

        // Multiple PID matches: also require title match.
        if (pidCandidates.length > 1) {
            const refined = pidCandidates.filter(w => w.get_title() === title);
            if (refined.length >= 1) return refined[0];
            return pidCandidates[0]; // best effort
        }

        // No PID match — fall back to title (advisory only).
        if (titleCandidates.length === 1) return titleCandidates[0];

        return null;
    }

    _applyAbove(window) {
        try {
            window.make_above();
            window.stick();
            window.unminimize();
        } catch (_) {
            // Compositor may have already destroyed the window.
        }
    }
}
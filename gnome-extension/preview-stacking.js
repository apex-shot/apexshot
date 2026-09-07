// SPDX-License-Identifier: AGPL-3.0-or-later

import Gio from 'gi://Gio';

const DBUS_PATH = '/org/apexshot/TrackedWindow';
const DBUS_INTERFACE = 'org.apexshot.TrackedWindow';

/// Keeps ApexShot's own preview and editor windows above other windows.
///
/// ApexShot announces each window it opens over the session bus, because a
/// Wayland client cannot raise itself. Windows are matched by PID and title,
/// then pinned with `make_above()` for as long as ApexShot tracks them.
/// Keep a tracked MetaWindow in the always-on-top layer.
/// Exported for unit tests.
export function applyAlwaysOnTop(window) {
    if (!window)
        return false;
    if (typeof window.get_compositor_private === 'function' && !window.get_compositor_private())
        return false;

    if (window.minimized && typeof window.unminimize === 'function')
        window.unminimize();
    if (!window.above && typeof window.make_above === 'function')
        window.make_above();
    // Restack inside the above layer so a newly focused window cannot cover us
    // without Mutter unsetting the above flag.
    if (typeof window.raise === 'function')
        window.raise();
    return true;
}

export class PreviewStacker {
    constructor() {
        // trackedId -> {pid, title, window, signalIds}
        this._tracked = new Map();
        // trackedId -> {pid, title}, waiting for their MetaWindow to appear
        this._pending = new Map();
        this._subscriptionId = 0;
        this._windowCreatedId = 0;
        this._focusWindowId = 0;
        // MetaWindow -> handler id for windows we watch for a late title
        this._titleWatchers = new Map();
    }

    enable() {
        this._subscriptionId = Gio.DBus.session.signal_subscribe(
            null,
            DBUS_INTERFACE,
            null,
            DBUS_PATH,
            null,
            Gio.DBusSignalFlags.NONE,
            (connection, sender, path, iface, signal, params) => {
                if (signal === 'TrackedWindowOpened') {
                    const [trackedId, pid, title] = params.recursiveUnpack();
                    this._onOpened(trackedId, pid, title);
                } else if (signal === 'TrackedWindowClosed') {
                    const [trackedId] = params.recursiveUnpack();
                    this._onClosed(trackedId);
                }
            });

        this._windowCreatedId = global.display.connect('window-created',
            (display, window) => this._onWindowCreated(window));
        this._focusWindowId = global.display.connect('notify::focus-window',
            () => this._raiseTracked());
    }

    disable() {
        if (this._subscriptionId) {
            Gio.DBus.session.signal_unsubscribe(this._subscriptionId);
            this._subscriptionId = 0;
        }

        if (this._windowCreatedId) {
            global.display.disconnect(this._windowCreatedId);
            this._windowCreatedId = 0;
        }

        if (this._focusWindowId) {
            global.display.disconnect(this._focusWindowId);
            this._focusWindowId = 0;
        }

        for (const [window, handlerId] of this._titleWatchers)
            window.disconnect(handlerId);
        this._titleWatchers.clear();

        for (const trackedId of [...this._tracked.keys()])
            this._release(trackedId);

        this._pending.clear();
    }

    _onOpened(trackedId, pid, title) {
        if (this._tracked.has(trackedId) || this._pending.has(trackedId))
            return;

        const window = this._findWindow(pid, title);
        if (window)
            this._pin(trackedId, pid, title, window);
        else
            this._pending.set(trackedId, {pid, title});
    }

    _onClosed(trackedId) {
        this._pending.delete(trackedId);
        this._release(trackedId);
    }

    _onWindowCreated(window) {
        // A newly mapped window can cover an already-tracked preview even when
        // Mutter leaves the `above` flag set. Re-raise first, then try to match
        // any preview that has not found its MetaWindow yet.
        this._raiseTracked();

        if (!window || this._pending.size === 0)
            return;

        if (this._resolvePending())
            return;

        // The title is often set a moment after the window appears, so give
        // this window one more chance to match once it has one.
        const handlerId = window.connect('notify::title', () => {
            this._unwatchTitle(window);
            this._resolvePending();
        });
        this._titleWatchers.set(window, handlerId);
    }

    _unwatchTitle(window) {
        const handlerId = this._titleWatchers.get(window);
        if (!handlerId)
            return;

        window.disconnect(handlerId);
        this._titleWatchers.delete(window);
    }

    _resolvePending() {
        let resolved = false;

        for (const [trackedId, {pid, title}] of [...this._pending]) {
            const window = this._findWindow(pid, title);
            if (!window)
                continue;

            this._pending.delete(trackedId);
            this._pin(trackedId, pid, title, window);
            resolved = true;
        }

        return resolved;
    }

    _pin(trackedId, pid, title, window) {
        const signalIds = [
            window.connect('notify::minimized', () => {
                if (!window.minimized)
                    this._raise(window);
            }),
            window.connect('notify::above', () => this._raise(window)),
            window.connect('unmanaged', () => this._release(trackedId)),
        ];

        this._tracked.set(trackedId, {pid, title, window, signalIds});
        this._unwatchTitle(window);
        this._raise(window);
    }

    _release(trackedId) {
        const tracked = this._tracked.get(trackedId);
        if (!tracked)
            return;

        this._tracked.delete(trackedId);

        const {window, signalIds} = tracked;
        for (const signalId of signalIds)
            window.disconnect(signalId);

        if (window.get_compositor_private() && window.above)
            window.unmake_above();
    }

    _raiseTracked() {
        for (const tracked of this._tracked.values())
            this._raise(tracked.window);
    }

    _raise(window) {
        applyAlwaysOnTop(window);
    }

    /// Match on PID first, since titles change; fall back to an exact title
    /// match for windows whose PID the compositor does not report.
    _findWindow(pid, title) {
        const candidates = [];

        for (const actor of global.get_window_actors()) {
            const window = actor.get_meta_window();
            if (window)
                candidates.push(window);
        }

        const byPid = candidates.filter(window => window.get_pid() === pid);
        if (byPid.length > 1) {
            const exact = byPid.find(window => window.get_title() === title);
            if (exact)
                return exact;
        }
        if (byPid.length > 0)
            return byPid[0];

        const byTitle = candidates.filter(window => window.get_title() === title);
        return byTitle.length === 1 ? byTitle[0] : null;
    }
}

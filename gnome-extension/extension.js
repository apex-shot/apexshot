// SPDX-License-Identifier: AGPL-3.0-or-later
// ApexShot Preview Helper — D-Bus lifecycle tracking, per-preview watchdog

import Meta from "gi://Meta";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import St from "gi://St";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";

// D-Bus interface exposed by the apexshot native process.
// The extension listens for signals on this name/path/iface.
const PREVIEW_DBUS_NAME      = "org.apexshot.Preview";
const PREVIEW_DBUS_PATH      = "/org/apexshot/Preview";
const PREVIEW_DBUS_IFACE     = "org.apexshot.Preview";

// Introspection XML so Gio can proxy the signal types.
const PREVIEW_DBUS_IFACE_XML = `
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

const MASK_DBUS_NAME = "org.apexshot.ShellOverlay";
const MASK_DBUS_PATH = "/org/apexshot/ShellOverlay";
const MASK_DBUS_IFACE = "org.apexshot.ShellOverlay";
const MASK_DBUS_IFACE_XML = `
<node>
  <interface name="org.apexshot.ShellOverlay">
    <method name="ShowMask">
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
    </method>
    <method name="HideMask"/>
    <method name="ShowControls">
      <arg type="s" name="dbus_dest" direction="in"/>
      <arg type="s" name="session_id" direction="in"/>
      <arg type="i" name="x" direction="in"/>
      <arg type="i" name="y" direction="in"/>
      <arg type="i" name="width" direction="in"/>
      <arg type="i" name="height" direction="in"/>
      <arg type="b" name="is_fullscreen" direction="in"/>
      <arg type="b" name="show_timer" direction="in"/>
      <arg type="s" name="runtime_overlay_snapshot" direction="in"/>
    </method>
    <method name="HideControls"/>
  </interface>
</node>`;

const CONTROLS_BAR_WIDTH = 346;
const CONTROLS_BAR_HEIGHT = 56;
const CONTROLS_MARGIN = 32;
const CONTROLS_DOCK_SAFE = 72;
const CONTROLS_GAP = 8;

class PreviewStackingHelper {
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
            PREVIEW_DBUS_IFACE,
            null,          // member — subscribe to all signals on the iface
            PREVIEW_DBUS_PATH,
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

class RecordingMaskService {
    constructor() {
        this._dbusObject = null;
        this._ownName = null;
        this._maskGroup = null;
        this._currentRect = null;
        this._controlsChrome = null;
        this._controlsState = null;
        this._controlsTimerSource = null;
        this._timerLabel = null;
        this._pauseIcon = null;
        this._monitorsChangedId = null;
    }

    enable() {
        this._dbusObject = Gio.DBusExportedObject.wrapJSObject(MASK_DBUS_IFACE_XML, this);
        try {
            this._dbusObject.export(Gio.DBus.session, MASK_DBUS_PATH);
        } catch (e) {
            logError(e, `Failed to export ${MASK_DBUS_PATH}`);
        }

        this._ownName = Gio.DBus.session.own_name(
            MASK_DBUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            null,
            () => {
                log(`[apexshot] Lost D-Bus name ${MASK_DBUS_NAME}`);
                this._ownName = null;
            }
        );

        this._monitorsChangedId = Main.layoutManager.connect("monitors-changed", () => {
            if (this._currentRect) {
                this._showMask(
                    this._currentRect.x,
                    this._currentRect.y,
                    this._currentRect.width,
                    this._currentRect.height
                );
            }
            this._repositionControls();
        });
    }

    disable() {
        this._hideControls();
        this._hideMask();

        if (this._monitorsChangedId !== null) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = null;
        }

        if (this._ownName !== null) {
            Gio.DBus.session.unown_name(this._ownName);
            this._ownName = null;
        }

        if (this._dbusObject) {
            try {
                this._dbusObject.unexport();
            } catch (e) {
                logError(e, `Failed to unexport ${MASK_DBUS_PATH}`);
            }
            this._dbusObject.run_dispose();
            this._dbusObject = null;
        }
    }

    ShowMaskAsync(params, invocation) {
        try {
            const [x, y, width, height] = params;
            this._showMask(x, y, width, height);
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    HideMaskAsync(_params, invocation) {
        try {
            this._hideMask();
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    ShowControlsAsync(params, invocation) {
        try {
            const [dbusDest, sessionId, x, y, width, height, isFullscreen, showTimer, runtimeOverlaySnapshot] = params;
            this._showControls({
                dbusDest,
                sessionId,
                rect: {x, y, width, height},
                isFullscreen,
                showTimer,
                runtimeOverlaySnapshot: runtimeOverlaySnapshot || null,
            });
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    HideControlsAsync(_params, invocation) {
        try {
            this._hideControls();
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    _showMask(x, y, width, height) {
        this._currentRect = {x, y, width, height};

        this._ensureMaskGroup();
        this._maskGroup.remove_all_children();

        const stage = global.stage;
        const stageWidth = stage.width;
        const stageHeight = stage.height;
        this._maskGroup.set_position(0, 0);
        this._maskGroup.set_size(stageWidth, stageHeight);

        const left = Math.max(0, x);
        const top = Math.max(0, y);
        const right = Math.min(stageWidth, x + width);
        const bottom = Math.min(stageHeight, y + height);

        const rects = [
            [0, 0, stageWidth, top],
            [0, top, left, Math.max(0, bottom - top)],
            [right, top, Math.max(0, stageWidth - right), Math.max(0, bottom - top)],
            [0, bottom, stageWidth, Math.max(0, stageHeight - bottom)],
        ];

        for (const [rectX, rectY, rectWidth, rectHeight] of rects) {
            if (rectWidth <= 0 || rectHeight <= 0)
                continue;

            const region = new St.Widget({
                reactive: false,
                x: rectX,
                y: rectY,
                width: rectWidth,
                height: rectHeight,
                style: "background-color: rgba(0, 0, 0, 0.55);",
            });
            this._maskGroup.add_child(region);
        }

        if (!this._maskGroup.get_parent())
            global.window_group.add_child(this._maskGroup);

        this._maskGroup.show();
    }

    _hideMask() {
        this._currentRect = null;

        if (!this._maskGroup)
            return;

        this._maskGroup.remove_all_children();
        this._maskGroup.hide();
        if (this._maskGroup.get_parent()) {
            this._maskGroup.get_parent().remove_child(this._maskGroup);
        }
    }

    _ensureMaskGroup() {
        if (this._maskGroup)
            return;

        this._maskGroup = new St.Widget({
            reactive: false,
            x: 0,
            y: 0,
            width: global.stage.width,
            height: global.stage.height,
        });
    }

    _showControls(spec) {
        this._hideControls();

        this._controlsState = {
            dbusDest: spec.dbusDest,
            sessionId: spec.sessionId,
            rect: {...spec.rect},
            isFullscreen: spec.isFullscreen,
            showTimer: spec.showTimer,
            runtimeOverlaySnapshot: spec.runtimeOverlaySnapshot,
            paused: false,
            elapsedBeforePauseMs: 0,
            runningStartMs: GLib.get_monotonic_time() / 1000,
        };

        this._controlsChrome = this._buildControlsChrome();
        Main.layoutManager.addChrome(this._controlsChrome, {
            affectsInputRegion: true,
            trackFullscreen: false,
        });
        this._controlsChrome.show();
        this._repositionControls();
        GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._repositionControls();
            return GLib.SOURCE_REMOVE;
        });
        this._startControlsTimer();
        this._updateTimerText();
    }

    _hideControls() {
        this._stopControlsTimer();
        this._controlsState = null;
        this._timerLabel = null;
        this._pauseIcon = null;

        if (!this._controlsChrome)
            return;

        if (this._controlsChrome.get_parent()) {
            Main.layoutManager.removeChrome(this._controlsChrome);
        }
        this._controlsChrome.destroy();
        this._controlsChrome = null;
    }

    _buildControlsChrome() {
        const chrome = new St.BoxLayout({
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: [
                "background-color: rgba(30, 30, 34, 0.85);",
                "border: 1px solid rgba(255, 255, 255, 0.15);",
                "border-radius: 28px;",
                "padding: 8px 6px 8px 12px;",
                "box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);",
                `width: ${CONTROLS_BAR_WIDTH}px;`,
                `height: ${CONTROLS_BAR_HEIGHT}px;`,
            ].join(" "),
        });

        const stopSegment = new St.BoxLayout({
            reactive: true,
            y_align: Clutter.ActorAlign.CENTER,
            style: [
                "background-color: rgba(255, 69, 58, 0.18);",
                "border: 1px solid rgba(255, 69, 58, 0.3);",
                "border-radius: 20px;",
                "padding: 0 16px 0 6px;",
                "margin-right: 8px;",
                "height: 40px;",
                "spacing: 8px;",
            ].join(" "),
        });

        const stopBtn = this._createIconButton("media-playback-stop-symbolic", () => {
            if (this._sendRecordingCommand("Stop"))
                this._hideControls();
        }, {
            accent: "color: rgb(255, 85, 75);",
            width: 32,
            height: 32,
            iconSize: 16,
            borderRadius: 16,
        });
        stopSegment.add_child(stopBtn);

        this._timerLabel = new St.Label({
            text: "0:00",
            visible: this._controlsState.showTimer,
            style: "color: rgb(255, 85, 75); font-weight: 800; font-size: 15px; font-family: monospace;",
            y_align: Clutter.ActorAlign.CENTER,
        });
        stopSegment.add_child(this._timerLabel);
        chrome.add_child(stopSegment);

        const buttonLayout = new St.BoxLayout({
            style: "spacing: 4px;",
            y_align: Clutter.ActorAlign.CENTER,
            x_align: Clutter.ActorAlign.CENTER,
            y_expand: true,
        });

        buttonLayout.add_child(this._createSeparator());

        this._pauseIcon = new St.Icon({
            icon_name: "media-playback-pause-symbolic",
            style: "icon-size: 18px; color: rgb(240, 240, 240);",
        });
        buttonLayout.add_child(this._createIconButton(this._pauseIcon, () => {
            const method = this._controlsState?.paused ? "Resume" : "Pause";
            if (!this._sendRecordingCommand(method))
                return;
            this._setControlsPaused(!this._controlsState.paused);
        }, { width: 40, height: 40, borderRadius: 20 }));

        buttonLayout.add_child(this._createIconButton("system-reboot-symbolic", () => {
            if (!this._sendRecordingCommand("Restart"))
                return;
            this._resetControlsTimer();
        }, { width: 40, height: 40, borderRadius: 20, iconSize: 18 }));

        buttonLayout.add_child(this._createSeparator());

        buttonLayout.add_child(this._createIconButton("user-trash-symbolic", () => {
            if (this._sendRecordingCommand("Discard"))
                this._hideControls();
        }, { width: 40, height: 40, borderRadius: 20, iconSize: 18 }));

        buttonLayout.add_child(this._createIconButton("view-list-symbolic", () => {}, { width: 40, height: 40, borderRadius: 20, iconSize: 18 }));

        chrome.add_child(buttonLayout);

        return chrome;
    }

    _createSeparator() {
        return new St.Widget({
            reactive: false,
            style: "width: 1px; height: 24px; margin: 0 6px; background-color: rgba(255, 255, 255, 0.12); border-radius: 1px;",
            y_align: Clutter.ActorAlign.CENTER,
        });
    }

    _createIconButton(icon, onClick, options = {}) {
        const w = options.width ?? 40;
        const h = options.height ?? 40;
        const r = options.borderRadius ?? 20;
        
        const button = new St.Button({
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: `background-color: transparent; width: ${w}px; height: ${h}px; border-radius: ${r}px; padding: 0;`,
        });

        const iconContainer = new St.BoxLayout({
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
            x_expand: true,
            y_expand: true,
        });

        const iconActor = typeof icon === "string"
            ? new St.Icon({
                icon_name: icon,
                style: `icon-size: ${options.iconSize ?? 18}px; ${options.accent ?? "color: rgb(240, 240, 240);" }`,
            })
            : icon;
            
        iconContainer.add_child(iconActor);
        button.set_child(iconContainer);

        const baseStyle = `width: ${w}px; height: ${h}px; border-radius: ${r}px; padding: 0; transition-duration: 200ms;`;
        
        button.connect("notify::hover", () => {
            if (button.hover) {
                button.set_style(`${baseStyle} background-color: rgba(255, 255, 255, 0.12);`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
            }
        });

        button.connect("button-press-event", () => {
            button.set_style(`${baseStyle} background-color: rgba(255, 255, 255, 0.18);`);
            return Clutter.EVENT_PROPAGATE;
        });

        button.connect("button-release-event", () => {
            if (button.hover) {
                button.set_style(`${baseStyle} background-color: rgba(255, 255, 255, 0.12);`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
            }
            return Clutter.EVENT_PROPAGATE;
        });

        button.connect("clicked", () => onClick());
        return button;
    }

    _repositionControls() {
        if (!this._controlsChrome || !this._controlsState)
            return;

        const monitor = this._monitorForRect(this._controlsState.rect);
        const [x, y] = this._computeControlsPosition(this._controlsState.rect, this._controlsState.isFullscreen, monitor);
        this._controlsChrome.set_position(x, y);
    }

    _computeControlsPosition(rect, isFullscreen, monitor) {
        const minX = monitor.x + CONTROLS_MARGIN;
        const maxX = Math.max(minX, monitor.x + monitor.width - CONTROLS_BAR_WIDTH - CONTROLS_MARGIN);
        const topY = monitor.y + CONTROLS_MARGIN;

        if (isFullscreen || rect.width <= 0 || rect.height <= 0) {
            return [
                monitor.x + Math.floor((monitor.width - CONTROLS_BAR_WIDTH) / 2),
                topY,
            ];
        }

        const x = Math.max(minX, Math.min(
            rect.x + Math.floor((rect.width - CONTROLS_BAR_WIDTH) / 2),
            maxX
        ));
        const belowY = rect.y + rect.height + CONTROLS_GAP;
        if (belowY + CONTROLS_BAR_HEIGHT + CONTROLS_DOCK_SAFE <= monitor.y + monitor.height) {
            return [x, belowY];
        }

        const aboveY = rect.y - CONTROLS_BAR_HEIGHT - CONTROLS_GAP;
        if (aboveY >= topY) {
            return [x, aboveY];
        }

        const maxY = monitor.y + monitor.height - CONTROLS_BAR_HEIGHT - CONTROLS_MARGIN;
        return [x, Math.max(topY, Math.min(aboveY, maxY))];
    }

    _monitorForRect(rect) {
        const monitors = Main.layoutManager.monitors ?? [];
        if (monitors.length === 0) {
            return {x: 0, y: 0, width: global.stage.width, height: global.stage.height};
        }

        if (rect.width > 0 && rect.height > 0) {
            const centerX = rect.x + rect.width / 2;
            const centerY = rect.y + rect.height / 2;
            const direct = monitors.find(m =>
                centerX >= m.x &&
                centerX < m.x + m.width &&
                centerY >= m.y &&
                centerY < m.y + m.height
            );
            if (direct)
                return direct;

            let bestMonitor = monitors[0];
            let bestArea = -1;
            for (const monitor of monitors) {
                const overlapLeft = Math.max(rect.x, monitor.x);
                const overlapTop = Math.max(rect.y, monitor.y);
                const overlapRight = Math.min(rect.x + rect.width, monitor.x + monitor.width);
                const overlapBottom = Math.min(rect.y + rect.height, monitor.y + monitor.height);
                const overlapWidth = Math.max(0, overlapRight - overlapLeft);
                const overlapHeight = Math.max(0, overlapBottom - overlapTop);
                const area = overlapWidth * overlapHeight;
                if (area > bestArea) {
                    bestArea = area;
                    bestMonitor = monitor;
                }
            }
            return bestMonitor;
        }

        return Main.layoutManager.primaryMonitor ?? monitors[0];
    }

    _startControlsTimer() {
        this._stopControlsTimer();
        this._controlsTimerSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 250, () => {
            this._updateTimerText();
            return this._controlsState ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
        });
    }

    _stopControlsTimer() {
        if (this._controlsTimerSource !== null) {
            GLib.source_remove(this._controlsTimerSource);
            this._controlsTimerSource = null;
        }
    }

    _setControlsPaused(paused) {
        if (!this._controlsState || this._controlsState.paused === paused)
            return;

        if (paused) {
            this._controlsState.elapsedBeforePauseMs = this._elapsedControlsMs();
        } else {
            this._controlsState.runningStartMs = GLib.get_monotonic_time() / 1000;
        }
        this._controlsState.paused = paused;
        if (this._pauseIcon) {
            this._pauseIcon.icon_name = paused
                ? "media-playback-start-symbolic"
                : "media-playback-pause-symbolic";
        }
        this._updateTimerText();
    }

    _resetControlsTimer() {
        if (!this._controlsState)
            return;

        this._controlsState.paused = false;
        this._controlsState.elapsedBeforePauseMs = 0;
        this._controlsState.runningStartMs = GLib.get_monotonic_time() / 1000;
        if (this._pauseIcon)
            this._pauseIcon.icon_name = "media-playback-pause-symbolic";
        this._updateTimerText();
    }

    _elapsedControlsMs() {
        if (!this._controlsState)
            return 0;
        if (this._controlsState.paused)
            return this._controlsState.elapsedBeforePauseMs;
        return this._controlsState.elapsedBeforePauseMs +
            Math.max(0, Math.floor(GLib.get_monotonic_time() / 1000 - this._controlsState.runningStartMs));
    }

    _updateTimerText() {
        if (!this._timerLabel || !this._controlsState || !this._controlsState.showTimer)
            return;
        this._timerLabel.text = this._formatElapsed(this._elapsedControlsMs());
    }

    _formatElapsed(elapsedMs) {
        const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
        const minutes = Math.floor(totalSeconds / 60);
        const seconds = totalSeconds % 60;
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }

    _sendRecordingCommand(method) {
        if (!this._controlsState)
            return false;

        try {
            const reply = Gio.DBus.session.call_sync(
                this._controlsState.dbusDest,
                "/org/apexshot/RecordingControl",
                "org.apexshot.RecordingControl",
                method,
                new GLib.Variant("(s)", [this._controlsState.sessionId]),
                new GLib.VariantType("(b)"),
                Gio.DBusCallFlags.NONE,
                -1,
                null
            );
            const [accepted] = reply.deepUnpack();
            return accepted;
        } catch (e) {
            logError(e, `[apexshot] Failed to send recording command ${method}`);
            return false;
        }
    }
}

export default class ApexShotShellSupport {
    constructor() {
        this._previewHelper = new PreviewStackingHelper();
        this._maskService = new RecordingMaskService();
    }

    enable() {
        this._previewHelper.enable();
        this._maskService.enable();
    }

    disable() {
        this._maskService.disable();
        this._previewHelper.disable();
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
// ApexShot Preview Helper — D-Bus lifecycle tracking, per-preview watchdog

import Meta from "gi://Meta";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import {createSessionState} from "./session-state.js";
import {pushRuntimeOverlayKeystrokeText} from "./session-state.js";
import {recordRuntimeOverlayPointerSample} from "./session-state.js";
import {recordRuntimeOverlayKeystroke} from "./session-state.js";
import {setRuntimeOverlayVisibility} from "./session-state.js";
import {shouldExcludeOverlayEvent, updateRuntimeOverlaySnapshot} from "./runtime-overlays.js";
import {MaskUi} from "./mask-ui.js";
import {ControlsUi} from "./controls-ui.js";

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
    <method name="ToggleOverlay">
      <arg type="s" name="key" direction="in"/>
      <arg type="b" name="visible" direction="in"/>
    </method>
    <method name="PushKeystroke">
      <arg type="s" name="session_id" direction="in"/>
      <arg type="s" name="text" direction="in"/>
    </method>
  </interface>
</node>`;

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
        this._monitorsChangedId = null;
        this._pointerPollSource = null;
        this._stageKeyPressEventId = null;
        this._sessionState = createSessionState();
        this._maskUi = new MaskUi(this._sessionState);
        this._controlsUi = new ControlsUi(this._sessionState, {
            sendRecordingCommand: method => this._sendRecordingCommand(method),
        });
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
            this._maskUi.refresh();
            this._controlsUi.reposition();
        });
        this._stageKeyPressEventId = global.stage.connect("key-press-event", (_actor, event) =>
            this._onStageKeyPress(event)
        );
        log("[apexshot] runtime keystroke listener enabled");
    }

    disable() {
        this._hideControls();
        this._hideMask();

        if (this._monitorsChangedId !== null) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = null;
        }
        this._stopPointerPolling();
        if (this._stageKeyPressEventId !== null) {
            global.stage.disconnect(this._stageKeyPressEventId);
            this._stageKeyPressEventId = null;
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

    ToggleOverlayAsync(params, invocation) {
        try {
            const [key, visible] = params;
            this._toggleOverlay(key, visible);
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    PushKeystrokeAsync(params, invocation) {
        try {
            const [sessionId, text] = params;
            this._pushKeystroke(sessionId, text);
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error("org.apexshot.ShellOverlay.Error", e.message);
        }
    }

    _showMask(x, y, width, height) {
        this._maskUi.showMask(x, y, width, height);
    }

    _hideMask() {
        this._maskUi.hideMask();
    }

    _ensureMaskGroup() {
        this._maskUi.ensureMaskGroup();
    }

    _showControls(spec) {
        this._controlsUi.showControls(spec);
        this._startPointerPolling();
    }

    _hideControls() {
        this._stopPointerPolling();
        this._controlsUi.hideControls();
    }

    _repositionControls() {
        this._controlsUi.reposition();
    }

    _startPointerPolling() {
        if (this._pointerPollSource !== null)
            return;

        this._pointerPollSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 16, () => {
            this._pollPointerState();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _stopPointerPolling() {
        if (this._pointerPollSource !== null) {
            GLib.source_remove(this._pointerPollSource);
            this._pointerPollSource = null;
        }
    }

    _toggleOverlay(key, visible) {
        if (!setRuntimeOverlayVisibility(this._sessionState, key, visible))
            return;
        updateRuntimeOverlaySnapshot(this._sessionState);
    }

    _pushKeystroke(sessionId, text) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState || controlsState.sessionId !== sessionId)
            return;

        const changed = pushRuntimeOverlayKeystrokeText(
            this._sessionState,
            text,
            Math.floor(GLib.get_monotonic_time() / 1000)
        );
        if (changed) {
            log(`[apexshot] pushed keystroke ${text}`);
            updateRuntimeOverlaySnapshot(this._sessionState);
        }
    }

    _pollPointerState() {
        if (!this._sessionState.controlsState)
            return;

        const [x, y, modifiers] = global.get_pointer();
        let target = null;
        if (global.stage && typeof global.stage.get_actor_at_pos === "function") {
            target = global.stage.get_actor_at_pos(Clutter.PickMode.REACTIVE, x, y);
        }
        const exclude = shouldExcludeOverlayEvent(this._sessionState, target);

        const clicks = recordRuntimeOverlayPointerSample(this._sessionState, {
            x,
            y,
            left: Boolean(modifiers & Clutter.ModifierType.BUTTON1_MASK),
            middle: Boolean(modifiers & Clutter.ModifierType.BUTTON2_MASK),
            right: Boolean(modifiers & Clutter.ModifierType.BUTTON3_MASK),
            capture: !exclude,
            timestampMs: Math.floor(GLib.get_monotonic_time() / 1000),
        });
        if (!clicks.length)
            return;

        updateRuntimeOverlaySnapshot(this._sessionState);
    }

    _sendRecordingCommand(method) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return false;

        try {
            const reply = Gio.DBus.session.call_sync(
                controlsState.dbusDest,
                "/org/apexshot/RecordingControl",
                "org.apexshot.RecordingControl",
                method,
                new GLib.Variant("(s)", [controlsState.sessionId]),
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

    _onStageKeyPress(event) {
        if (!event)
            return Clutter.EVENT_PROPAGATE;

        const target = typeof event.get_source === "function"
            ? event.get_source()
            : null;
        if (shouldExcludeOverlayEvent(this._sessionState, target))
            return Clutter.EVENT_PROPAGATE;

        const state = typeof event.get_state === "function" ? event.get_state() : 0;
        const keySymbol = typeof event.get_key_symbol === "function"
            ? event.get_key_symbol()
            : 0;
        const unicodeValue = typeof event.get_key_unicode === "function"
            ? event.get_key_unicode()
            : 0;
        const keyName = Clutter.keysym_to_name(keySymbol) ?? "";
        const changed = recordRuntimeOverlayKeystroke(this._sessionState, {
            keySymbol: keyName,
            unicodeChar: unicodeValue > 0 ? String.fromCodePoint(unicodeValue) : "",
            ctrl: Boolean(state & Clutter.ModifierType.CONTROL_MASK),
            alt: Boolean(state & Clutter.ModifierType.MOD1_MASK),
            shift: Boolean(state & Clutter.ModifierType.SHIFT_MASK),
            meta: Boolean(state & (Clutter.ModifierType.SUPER_MASK | Clutter.ModifierType.META_MASK)),
            timestampMs: Math.floor(GLib.get_monotonic_time() / 1000),
        });
        log(`[apexshot] stage key press key=${keyName || unicodeValue || "unknown"} changed=${changed}`);
        if (changed)
            updateRuntimeOverlaySnapshot(this._sessionState);
        return Clutter.EVENT_PROPAGATE;
    }
}

export default class ApexShotShellSupport {
    constructor() {
        this._previewHelper = new PreviewStackingHelper();
        this._maskService = new RecordingMaskService();
    }

    enable() {
        log("[apexshot] extension enable marker 2026-03-29T02:45Z");
        this._previewHelper.enable();
        this._maskService.enable();
    }

    disable() {
        this._maskService.disable();
        this._previewHelper.disable();
    }
}

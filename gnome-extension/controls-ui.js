// SPDX-License-Identifier: AGPL-3.0-or-later

import GLib from "gi://GLib";
import St from "gi://St";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import {
    clearControlsState,
    getRuntimeOverlaySupportMessage,
    getRuntimeOverlayVisibility,
    registerSelfOwnedActor,
    setRuntimeOverlayVisibility,
    setControlsState,
} from "./session-state.js";
import {
    attachRuntimeOverlays,
    destroyRuntimeOverlays,
    updateRuntimeOverlaySnapshot,
} from "./runtime-overlays.js";

const CONTROLS_BAR_WIDTH = 280;
const CONTROLS_BAR_HEIGHT = 48;
const CONTROLS_MARGIN = 32;
const CONTROLS_DOCK_SAFE = 72;
const CONTROLS_GAP = 8;
const RUNTIME_OVERLAY_MENU_WIDTH = 200;
const RUNTIME_OVERLAY_MENU_GAP = 10;
const RUNTIME_OVERLAY_MENU_MARGIN = 24;
const RUNTIME_OVERLAY_TOGGLE_SPECS = Object.freeze([
    {key: "webcam", icon: "camera-web-symbolic", label: "Webcam"},
    {key: "clicks", icon: "input-mouse-symbolic", label: "Clicks"},
    {key: "keystrokes", icon: "input-keyboard-symbolic", label: "Keystrokes"},
    {key: "mic", icon: "audio-input-microphone-symbolic", label: "Mic"},
    {key: "speaker", icon: "audio-volume-high-symbolic", label: "Speaker"},
]);

export class ControlsUi {
    constructor(sessionState, {sendRecordingCommand}) {
        this._sessionState = sessionState;
        this._sendRecordingCommand = sendRecordingCommand;
        this._controlsChrome = null;
        this._controlsTimerSource = null;
        this._timerLabel = null;
        this._pauseIcon = null;
        this._runtimeOverlayMenu = null;
        this._runtimeOverlayMenuButton = null;
        this._runtimeOverlayToggleRows = new Map();
        this._runtimeOverlayInfoExpanded = new Set();
    }

    showControls(spec) {
        this.hideControls();

        setControlsState(this._sessionState, spec, GLib.get_monotonic_time() / 1000);
        attachRuntimeOverlays(this._sessionState);

        this._controlsChrome = this._buildControlsChrome();
        Main.layoutManager.addChrome(this._controlsChrome, {
            affectsInputRegion: true,
            trackFullscreen: false,
        });
        this._controlsChrome.show();
        this.reposition();
        GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this.reposition();
            return GLib.SOURCE_REMOVE;
        });
        this._startControlsTimer();
        this._updateTimerText();
    }

    hideControls() {
        this._stopControlsTimer();
        this._hideRuntimeOverlayMenu();
        destroyRuntimeOverlays(this._sessionState);
        clearControlsState(this._sessionState);
        this._timerLabel = null;
        this._pauseIcon = null;
        this._runtimeOverlayMenuButton = null;
        this._runtimeOverlayToggleRows.clear();
        this._runtimeOverlayInfoExpanded.clear();

        if (!this._controlsChrome)
            return;

        if (this._controlsChrome.get_parent())
            Main.layoutManager.removeChrome(this._controlsChrome);
        this._controlsChrome.destroy();
        this._controlsChrome = null;
    }

    reposition() {
        const controlsState = this._sessionState.controlsState;
        if (!this._controlsChrome || !controlsState)
            return;

        const monitor = this._monitorForRect(controlsState.rect);
        const [x, y] = this._computeControlsPosition(
            controlsState.rect,
            controlsState.isFullscreen,
            monitor
        );
        this._controlsChrome.set_position(x, y);
        this._positionRuntimeOverlayMenu();
        updateRuntimeOverlaySnapshot(this._sessionState);
    }

    _buildControlsChrome() {
        const controlsState = this._sessionState.controlsState;
        const chrome = registerSelfOwnedActor(this._sessionState, new St.BoxLayout({
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: [
                "background-color: #141414;",
                "border: 1px solid rgba(255, 255, 255, 0.10);",
                "border-radius: 10px;",
                "padding: 8px 12px;",
                "spacing: 12px;",
                "box-shadow: 0 4px 12px rgba(0, 0, 0, 0.24);",
            ].join(" "),
        }), "controls.chrome");

        const stopSegment = new St.BoxLayout({
            reactive: true,
            y_align: Clutter.ActorAlign.CENTER,
            style: [
                "background-color: #000000;",
                "border: 1px solid rgba(255, 255, 255, 0.11);",
                "border-radius: 6px;",
                "padding: 3px 8px 3px 3px;",
                "height: 36px;",
                "spacing: 8px;",
            ].join(" "),
        });

        const stopBtn = this._createIconButton("media-playback-stop-symbolic", () => {
            if (this._sendRecordingCommand("Stop"))
                this.hideControls();
        }, {
            accent: "color: #ed6a5e;",
            width: 30,
            height: 30,
            iconSize: 15,
            borderRadius: 6,
        });
        stopSegment.add_child(stopBtn);

        this._timerLabel = new St.Label({
            text: "0:00",
            visible: controlsState.showTimer,
            style: "color: #f1f1f3; font-weight: 700; font-size: 14px; font-family: monospace; letter-spacing: 0.2px;",
            y_align: Clutter.ActorAlign.CENTER,
        });
        stopSegment.add_child(this._timerLabel);
        chrome.add_child(stopSegment);

        const buttonShell = new St.BoxLayout({
            style: [
                "background-color: #000000;",
                "border: 1px solid rgba(255, 255, 255, 0.11);",
                "border-radius: 6px;",
                "padding: 3px;",
            ].join(" "),
            y_align: Clutter.ActorAlign.CENTER,
            x_align: Clutter.ActorAlign.CENTER,
            y_expand: true,
        });

        const buttonLayout = new St.BoxLayout({
            style: "spacing: 2px;",
            y_align: Clutter.ActorAlign.CENTER,
            x_align: Clutter.ActorAlign.CENTER,
            y_expand: true,
        });

        this._pauseIcon = new St.Icon({
            icon_name: "media-playback-pause-symbolic",
            style: "icon-size: 16px; color: rgb(236, 239, 244);",
        });
        buttonLayout.add_child(this._createIconButton(this._pauseIcon, () => {
            const state = this._sessionState.controlsState;
            const method = state?.paused ? "Resume" : "Pause";
            if (!this._sendRecordingCommand(method))
                return;
            this._setControlsPaused(!state.paused);
        }));

        buttonLayout.add_child(this._createIconButton("system-reboot-symbolic", () => {
            if (!this._sendRecordingCommand("Restart"))
                return;
            this._resetControlsTimer();
        }, {
            iconSize: 16,
        }));

        buttonLayout.add_child(this._createSeparator());

        buttonLayout.add_child(this._createIconButton("user-trash-symbolic", () => {
            if (this._sendRecordingCommand("Discard"))
                this.hideControls();
        }, {
            iconSize: 16,
            accent: "color: rgba(236, 222, 187, 0.96);",
        }));

        if (controlsState.runtimeOverlaySnapshot)
            buttonLayout.add_child(this._createRuntimeOverlayMenuButton());

        buttonShell.add_child(buttonLayout);
        chrome.add_child(buttonShell);

        return chrome;
    }

    _createSeparator() {
        return new St.Widget({
            reactive: false,
            style: "width: 1px; height: 20px; margin: 0 4px; background-color: rgba(255, 255, 255, 0.11); border-radius: 1px;",
            y_align: Clutter.ActorAlign.CENTER,
        });
    }

    _createIconButton(icon, onClick, options = {}) {
        const w = options.width ?? 30;
        const h = options.height ?? 30;
        const r = options.borderRadius ?? 6;

        const button = registerSelfOwnedActor(this._sessionState, new St.Button({
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: `background-color: transparent; width: ${w}px; height: ${h}px; border-radius: ${r}px; padding: 0;`,
        }), options.owner ?? "controls.button");

        const iconContainer = new St.BoxLayout({
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
            x_expand: true,
            y_expand: true,
        });

        const iconActor = typeof icon === "string"
            ? new St.Icon({
                icon_name: icon,
                style: `icon-size: ${options.iconSize ?? 16}px; ${options.accent ?? "color: #f1f1f3;"}`,
            })
            : icon;

        iconContainer.add_child(iconActor);
        button.set_child(iconContainer);

        const hoverBackground = options.hoverBackground ?? "#1a1a1d";
        const pressBackground = options.pressBackground ?? "#151517";
        const hoverBorder = options.hoverBorder ?? "rgba(255, 255, 255, 0.09)";
        const pressBorder = options.pressBorder ?? "rgba(255, 255, 255, 0.15)";
        const baseStyle = [
            `width: ${w}px;`,
            `height: ${h}px;`,
            `border-radius: ${r}px;`,
            "padding: 0;",
            "transition-duration: 140ms;",
            "border: 1px solid transparent;",
        ].join(" ");

        button.connect("notify::hover", () => {
            if (button.hover) {
                button.set_style(`${baseStyle} background-color: ${hoverBackground}; border-color: ${hoverBorder};`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
            }
        });

        button.connect("button-press-event", () => {
            button.set_style(`${baseStyle} background-color: ${pressBackground}; border-color: ${pressBorder};`);
            return Clutter.EVENT_PROPAGATE;
        });

        button.connect("button-release-event", () => {
            if (button.hover) {
                button.set_style(`${baseStyle} background-color: ${hoverBackground}; border-color: ${hoverBorder};`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
            }
            return Clutter.EVENT_PROPAGATE;
        });

        button.connect("clicked", () => onClick());
        return button;
    }

    _createRuntimeOverlayMenuButton() {
        this._runtimeOverlayMenuButton = this._createIconButton("view-list-symbolic", () => {
            if (this._runtimeOverlayMenu)
                this._hideRuntimeOverlayMenu();
            else
                this._showRuntimeOverlayMenu();
        }, {
            iconSize: 16,
            owner: "controls.overlay-menu-button",
        });
        return this._runtimeOverlayMenuButton;
    }

    _showRuntimeOverlayMenu() {
        if (this._runtimeOverlayMenu || !this._sessionState.runtimeOverlaySnapshot)
            return;

        const menu = registerSelfOwnedActor(this._sessionState, new St.BoxLayout({
            vertical: true,
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: [
                `width: ${RUNTIME_OVERLAY_MENU_WIDTH}px;`,
                "background-color: rgba(30, 30, 30, 0.92);",
                "border: 1px solid rgba(255, 255, 255, 0.16);",
                "border-radius: 12px;",
                "padding: 8px 4px;",
                "spacing: 0;",
                "box-shadow: 0 14px 34px rgba(0, 0, 0, 0.4);",
            ].join(" "),
        }), "controls.overlay-menu");

        menu.add_child(new St.Label({
            text: "OVERLAYS",
            style: "padding: 8px 14px 4px 14px; font-size: 11px; font-weight: 700; color: rgba(255, 255, 255, 0.43); letter-spacing: 0.2px;",
        }));

        this._runtimeOverlayToggleRows.clear();
        for (const spec of RUNTIME_OVERLAY_TOGGLE_SPECS)
            menu.add_child(this._createRuntimeOverlayToggleRow(spec));

        this._runtimeOverlayMenu = menu;
        Main.layoutManager.addChrome(this._runtimeOverlayMenu, {
            affectsInputRegion: true,
            trackFullscreen: false,
        });
        this._runtimeOverlayMenu.show();
        this._refreshRuntimeOverlayToggleRows();
        this._positionRuntimeOverlayMenu();
        GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._positionRuntimeOverlayMenu();
            return GLib.SOURCE_REMOVE;
        });
    }

    _hideRuntimeOverlayMenu() {
        if (!this._runtimeOverlayMenu)
            return;

        if (this._runtimeOverlayMenu.get_parent())
            Main.layoutManager.removeChrome(this._runtimeOverlayMenu);
        this._runtimeOverlayMenu.destroy();
        this._runtimeOverlayMenu = null;
        this._runtimeOverlayToggleRows.clear();
        this._runtimeOverlayInfoExpanded.clear();
    }

    _createRuntimeOverlayToggleRow(spec) {
        const visible = getRuntimeOverlayVisibility(this._sessionState, spec.key);
        const supportMessage = getRuntimeOverlaySupportMessage(this._sessionState, spec.key);
        const infoExpanded = this._runtimeOverlayInfoExpanded.has(spec.key);

        const button = registerSelfOwnedActor(this._sessionState, new St.Button({
            reactive: !supportMessage,
            can_focus: !supportMessage,
            track_hover: !supportMessage,
            style: [
                "padding: 0;",
                "border-radius: 6px;",
                "margin: 1px 4px;",
                "background-color: transparent;",
                "text-align: left;",
            ].join(" "),
        }), `controls.overlay-toggle.${spec.key}`);

        const layout = new St.BoxLayout({
            vertical: true,
            reactive: false,
            style: "padding: 6px 4px; spacing: 2px; border-radius: 6px;",
        });

        const mainRow = new St.BoxLayout({
            reactive: false,
            style: "spacing: 6px;",
            x_align: Clutter.ActorAlign.FILL,
            x_expand: true,
        });
        layout.add_child(mainRow);

        const checkIcon = new St.Icon({
            icon_name: "object-select-symbolic",
            style: `icon-size: 14px; color: ${!supportMessage && visible ? "#007AFF" : "transparent"};`,
            y_align: Clutter.ActorAlign.CENTER,
        });
        mainRow.add_child(checkIcon);

        const label = new St.Label({
            text: spec.label,
            y_align: Clutter.ActorAlign.CENTER,
            x_expand: true,
            style: "font-size: 13px; color: #f1f1f3;",
        });
        mainRow.add_child(label);

        let infoButton = null;
        if (supportMessage) {
            infoButton = registerSelfOwnedActor(this._sessionState, new St.Button({
                reactive: true,
                can_focus: true,
                track_hover: true,
                style: [
                    "width: 18px;",
                    "height: 18px;",
                    "padding: 0;",
                    "border-radius: 9px;",
                    "background-color: rgba(255, 255, 255, 0.06);",
                    "border: 1px solid rgba(255, 255, 255, 0.1);",
                ].join(" "),
            }), `controls.overlay-toggle-info.${spec.key}`);
            infoButton.set_child(new St.Label({
                text: "?",
                y_align: Clutter.ActorAlign.CENTER,
                x_align: Clutter.ActorAlign.CENTER,
                style: "font-size: 11px; font-weight: 700; color: rgba(255, 255, 255, 0.82);",
            }));
            infoButton.connect("clicked", () => {
                if (this._runtimeOverlayInfoExpanded.has(spec.key))
                    this._runtimeOverlayInfoExpanded.delete(spec.key);
                else
                    this._runtimeOverlayInfoExpanded.add(spec.key);
                this._refreshRuntimeOverlayToggleRows();
            });
            mainRow.add_child(infoButton);
        }

        const messageLabel = new St.Label({
            text: supportMessage,
            visible: Boolean(supportMessage) && infoExpanded,
            style: "font-size: 11px; color: rgba(255, 255, 255, 0.52); padding-left: 20px;",
            x_expand: true,
        });
        layout.add_child(messageLabel);

        button.set_child(layout);
        button.connect("clicked", () => this._toggleRuntimeOverlay(spec.key));

        const baseStyle = "padding: 0; border-radius: 6px; margin: 1px 4px; text-align: left;";
        button.connect("notify::hover", () => {
            if (button.hover && !supportMessage) {
                button.set_style(`${baseStyle} background-color: #007AFF;`);
                layout.set_style("padding: 6px 4px; spacing: 2px; border-radius: 6px;");
                label.set_style("font-size: 13px; color: white;");
                checkIcon.set_style(`icon-size: 14px; color: ${visible ? "white" : "transparent"};`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
                layout.set_style("padding: 6px 4px; spacing: 2px; border-radius: 6px;");
                label.set_style(`font-size: 13px; color: ${supportMessage ? "rgba(255,255,255,0.72)" : "#f1f1f3"};`);
                checkIcon.set_style(`icon-size: 14px; color: ${!supportMessage && visible ? "#007AFF" : "transparent"};`);
            }
        });

        this._runtimeOverlayToggleRows.set(spec.key, {button, layout, label, messageLabel, checkIcon, infoButton});
        return button;
    }

    _toggleRuntimeOverlay(key) {
        if (getRuntimeOverlaySupportMessage(this._sessionState, key))
            return;

        const nextVisible = !getRuntimeOverlayVisibility(this._sessionState, key);
        if (!setRuntimeOverlayVisibility(this._sessionState, key, nextVisible))
            return;

        this._refreshRuntimeOverlayToggleRows();
        updateRuntimeOverlaySnapshot(this._sessionState);
    }

    _refreshRuntimeOverlayToggleRows() {
        for (const [key, row] of this._runtimeOverlayToggleRows.entries()) {
            const supportMessage = getRuntimeOverlaySupportMessage(this._sessionState, key);
            const visible = getRuntimeOverlayVisibility(this._sessionState, key);
            const isHovered = row.button.hover;

            if (isHovered && !supportMessage) {
                row.button.set_style("padding: 0; border-radius: 6px; margin: 1px 4px; text-align: left; background-color: #007AFF;");
                row.label.set_style("font-size: 13px; color: white;");
                row.checkIcon.set_style(`icon-size: 14px; color: ${visible ? "white" : "transparent"};`);
            } else {
                row.button.set_style("padding: 0; border-radius: 6px; margin: 1px 4px; text-align: left; background-color: transparent;");
                row.label.set_style(`font-size: 13px; color: ${supportMessage ? "rgba(255,255,255,0.72)" : "#f1f1f3"};`);
                row.checkIcon.set_style(`icon-size: 14px; color: ${!supportMessage && visible ? "#007AFF" : "transparent"};`);
            }

            row.messageLabel.text = supportMessage;
            row.messageLabel.visible = Boolean(supportMessage) && this._runtimeOverlayInfoExpanded.has(key);
            row.button.reactive = !supportMessage;
            row.button.can_focus = !supportMessage;

            if (supportMessage) {
                row.layout.set_style("padding: 6px 4px; spacing: 2px; border-radius: 6px; background-color: rgba(255,180,80,0.04); border: 1px solid rgba(255,210,120,0.08);");
                if (row.infoButton) {
                    row.infoButton.set_style([
                        "width: 18px;",
                        "height: 18px;",
                        "padding: 0;",
                        "border-radius: 9px;",
                        `background-color: ${this._runtimeOverlayInfoExpanded.has(key) ? "rgba(255,255,255,0.12)" : "rgba(255,255,255,0.06)"};`,
                        "border: 1px solid rgba(255, 255, 255, 0.1);",
                    ].join(" "));
                }
            } else {
                row.layout.set_style("padding: 6px 4px; spacing: 2px; border-radius: 6px;");
            }
        }
    }

    _positionRuntimeOverlayMenu() {
        if (!this._runtimeOverlayMenu || !this._controlsChrome)
            return;

        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return;

        const monitor = this._monitorForRect(controlsState.rect);
        const [controlsX, controlsY] = this._controlsChrome.get_position();
        const [, menuWidth] = this._runtimeOverlayMenu.get_preferred_width(-1);
        const [, menuHeight] = this._runtimeOverlayMenu.get_preferred_height(menuWidth);
        const minX = monitor.x + RUNTIME_OVERLAY_MENU_MARGIN;
        const maxX = Math.max(minX, monitor.x + monitor.width - menuWidth - RUNTIME_OVERLAY_MENU_MARGIN);
        const x = Math.max(minX, Math.min(controlsX + CONTROLS_BAR_WIDTH - menuWidth, maxX));

        const topY = controlsY - menuHeight - RUNTIME_OVERLAY_MENU_GAP;
        const bottomY = controlsY + CONTROLS_BAR_HEIGHT + RUNTIME_OVERLAY_MENU_GAP;
        const unclampedY = topY >= monitor.y + RUNTIME_OVERLAY_MENU_MARGIN
            ? topY
            : Math.min(
                bottomY,
                monitor.y + monitor.height - menuHeight - RUNTIME_OVERLAY_MENU_MARGIN
            );
        const minY = monitor.y + RUNTIME_OVERLAY_MENU_MARGIN;
        const maxY = Math.max(minY, monitor.y + monitor.height - menuHeight - RUNTIME_OVERLAY_MENU_MARGIN);
        const y = Math.max(minY, Math.min(unclampedY, maxY));

        this._runtimeOverlayMenu.set_position(x, y);
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
        if (belowY + CONTROLS_BAR_HEIGHT + CONTROLS_DOCK_SAFE <= monitor.y + monitor.height)
            return [x, belowY];

        const aboveY = rect.y - CONTROLS_BAR_HEIGHT - CONTROLS_GAP;
        if (aboveY >= topY)
            return [x, aboveY];

        const maxY = monitor.y + monitor.height - CONTROLS_BAR_HEIGHT - CONTROLS_MARGIN;
        return [x, Math.max(topY, Math.min(aboveY, maxY))];
    }

    _monitorForRect(rect) {
        const monitors = Main.layoutManager.monitors ?? [];
        if (monitors.length === 0)
            return {x: 0, y: 0, width: global.stage.width, height: global.stage.height};

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
            updateRuntimeOverlaySnapshot(this._sessionState);
            return this._sessionState.controlsState ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
        });
    }

    _stopControlsTimer() {
        if (this._controlsTimerSource !== null) {
            GLib.source_remove(this._controlsTimerSource);
            this._controlsTimerSource = null;
        }
    }

    _setControlsPaused(paused) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState || controlsState.paused === paused)
            return;

        if (paused) {
            controlsState.elapsedBeforePauseMs = this._elapsedControlsMs();
        } else {
            controlsState.runningStartMs = GLib.get_monotonic_time() / 1000;
        }
        controlsState.paused = paused;
        if (this._pauseIcon) {
            this._pauseIcon.icon_name = paused
                ? "media-playback-start-symbolic"
                : "media-playback-pause-symbolic";
        }
        this._updateTimerText();
    }

    _resetControlsTimer() {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return;

        controlsState.paused = false;
        controlsState.elapsedBeforePauseMs = 0;
        controlsState.runningStartMs = GLib.get_monotonic_time() / 1000;
        if (this._pauseIcon)
            this._pauseIcon.icon_name = "media-playback-pause-symbolic";
        this._updateTimerText();
    }

    _elapsedControlsMs() {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return 0;
        if (controlsState.paused)
            return controlsState.elapsedBeforePauseMs;
        return controlsState.elapsedBeforePauseMs +
            Math.max(0, Math.floor(GLib.get_monotonic_time() / 1000 - controlsState.runningStartMs));
    }

    _updateTimerText() {
        const controlsState = this._sessionState.controlsState;
        if (!this._timerLabel || !controlsState || !controlsState.showTimer)
            return;
        this._timerLabel.text = this._formatElapsed(this._elapsedControlsMs());
    }

    _formatElapsed(elapsedMs) {
        const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
        const minutes = Math.floor(totalSeconds / 60);
        const seconds = totalSeconds % 60;
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }
}

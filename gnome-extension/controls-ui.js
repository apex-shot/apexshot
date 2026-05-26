// SPDX-License-Identifier: AGPL-3.0-or-later

import GLib from "gi://GLib";
import St from "gi://St";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import {shellVersionAtLeast50} from "./gnome-version.js";
import {
    clearControlsState,
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
import {
    computeControlsDockPosition,
    createRuntimeOverlayHeaderStyle,
    computeAdjacentPopupPosition,
    createRuntimeOverlayMenuStyle,
    createRuntimeOverlayRowStyles,
    createWarningPopupStyle,
} from "./controls-ui-layout.js";

const CONTROLS_BAR_WIDTH_MIN = 320;
const CONTROLS_BAR_HEIGHT = 48;
const RUNTIME_OVERLAY_MENU_WIDTH = 168;
const RUNTIME_OVERLAY_MENU_GAP = 10;
const RUNTIME_OVERLAY_MENU_MARGIN = 24;
const RUNTIME_OVERLAY_TOGGLE_SPECS = Object.freeze([
    {key: "webcam", icon: "camera-web-symbolic", label: "Webcam"},
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
        this._panelTimerLabel = null;
        this._panelIndicator = null;
        this._pauseIcon = null;
        this._runtimeOverlayMenu = null;
        this._runtimeOverlayMenuButton = null;
        this._runtimeOverlayToggleRows = new Map();
        this._warningPopup = null;
        this._warningPopupAnchor = null;
        this._warningPopupTimeout = null;
    }

    showControls(spec) {
        this.hideControls();

        setControlsState(this._sessionState, spec, GLib.get_monotonic_time() / 1000);
        attachRuntimeOverlays(this._sessionState);
        this._panelIndicator = this._buildPanelIndicator();
        Main.panel._rightBox.insert_child_at_index(this._panelIndicator, 0);
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
        this._hideWarningPopup();
        destroyRuntimeOverlays(this._sessionState);
        clearControlsState(this._sessionState);
        this._timerLabel = null;
        this._panelTimerLabel = null;
        this._pauseIcon = null;
        this._runtimeOverlayMenuButton = null;
        this._runtimeOverlayToggleRows.clear();

        if (this._controlsChrome) {
            if (this._controlsChrome.get_parent())
                Main.layoutManager.removeChrome(this._controlsChrome);
            this._controlsChrome.destroy();
            this._controlsChrome = null;
        }

        if (this._panelIndicator) {
            this._panelIndicator.destroy();
            this._panelIndicator = null;
        }
    }

    reposition() {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return;

        if (!this._controlsChrome) {
            updateRuntimeOverlaySnapshot(this._sessionState);
            return;
        }

        const monitor = this._monitorForRect(controlsState.rect);
        const [x, y] = this._computeControlsPosition(controlsState, monitor);
        this._controlsChrome.set_position(x, y);
        this._positionRuntimeOverlayMenu();
        this._positionWarningPopup();
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
                "border: 1px solid rgba(255, 255, 255, 0.08);",
                "border-radius: 12px;",
                "padding: 4px 6px;",
                "spacing: 0;",
            ].join(" "),
        }), "controls.chrome");

        const stopSegment = new St.BoxLayout({
            reactive: true,
            y_align: Clutter.ActorAlign.CENTER,
            style: [
                "background-color: #1e1f22;",
                "border-radius: 8px;",
                "padding: 4px 12px 4px 8px;",
                "height: 38px;",
                "spacing: 8px;",
            ].join(" "),
        });

        const stopBtn = this._createIconButton("media-playback-stop-symbolic", () => {
            this._sendRecordingCommand("Stop", accepted => {
                if (accepted)
                    this.hideControls();
            });
        }, {
            accent: "color: #f46357;",
            width: 26,
            height: 26,
            iconSize: 13,
            borderRadius: 0,
            hoverBackground: "transparent",
            pressBackground: "transparent",
            hoverBorder: "transparent",
            pressBorder: "transparent",
        });
        stopSegment.add_child(stopBtn);

        this._timerLabel = new St.Label({
            text: "0:00",
            visible: controlsState.showTimer,
            style: "color: #f46357; font-weight: 800; font-size: 14px; letter-spacing: -0.1px;",
            y_align: Clutter.ActorAlign.CENTER,
        });
        stopSegment.add_child(this._timerLabel);
        chrome.add_child(stopSegment);
        chrome.add_child(this._createSeparator());

        const buttonLayout = new St.BoxLayout({
            style: "spacing: 0;",
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
            this._sendRecordingCommand(method, accepted => {
                if (accepted)
                    this._setControlsPaused(!state.paused);
            });
        }, {
            width: 52,
            height: 38,
            borderRadius: 8,
        }));

        buttonLayout.add_child(this._createSeparator());

        buttonLayout.add_child(this._createIconButton("system-reboot-symbolic", () => {
            this._sendRecordingCommand("Restart", accepted => {
                if (accepted)
                    this._resetControlsTimer();
            });
        }, {
            iconSize: 16,
            width: 52,
            height: 38,
            borderRadius: 8,
        }));

        buttonLayout.add_child(this._createSeparator());

        buttonLayout.add_child(this._createIconButton("user-trash-symbolic", () => {
            this._sendRecordingCommand("Discard", accepted => {
                if (accepted)
                    this.hideControls();
            });
        }, {
            iconSize: 16,
            width: 52,
            height: 38,
            borderRadius: 8,
        }));

        buttonLayout.add_child(this._createSeparator());

        if (controlsState.runtimeOverlaySnapshot)
            buttonLayout.add_child(this._createRuntimeOverlayMenuButton());

        chrome.add_child(buttonLayout);

        return chrome;
    }

    _buildPanelIndicator() {
        const indicator = new St.BoxLayout({
            style_class: "panel-status-menu-box",
            y_align: Clutter.ActorAlign.CENTER,
            reactive: false,
        });

        const pill = new St.BoxLayout({
            reactive: false,
            y_align: Clutter.ActorAlign.CENTER,
            style: [
                "background-color: rgba(180, 62, 62, 0.92);",
                "border-radius: 999px;",
                "padding: 0 6px 0 10px;",
                "spacing: 6px;",
                "height: 26px;",
            ].join(" "),
        });

        this._panelTimerLabel = new St.Label({
            text: "0:00",
            visible: this._sessionState.controlsState?.showTimer ?? true,
            y_align: Clutter.ActorAlign.CENTER,
            style: "font-weight: 800; color: white; padding: 0 2px 0 0;",
        });
        pill.add_child(this._panelTimerLabel);

        const stopButton = new St.Button({
            reactive: true,
            can_focus: true,
            track_hover: true,
            style: [
                "background-color: rgba(0, 0, 0, 0.18);",
                "border-radius: 999px;",
                "padding: 0;",
                "width: 18px;",
                "height: 18px;",
            ].join(" "),
            child: new St.Icon({
                icon_name: "media-playback-stop-symbolic",
                style: "icon-size: 11px; color: white;",
                y_align: Clutter.ActorAlign.CENTER,
            }),
        });
        stopButton.connect("button-press-event", () => {
            this._sendRecordingCommand("Stop", accepted => {
                if (accepted)
                    this.hideControls();
            });
            return Clutter.EVENT_STOP;
        });
        pill.add_child(stopButton);
        indicator.add_child(pill);

        return indicator;
    }

    _createSeparator() {
        return new St.Widget({
            reactive: false,
            style: "width: 1px; height: 100%; margin: 15px 0; background-color: rgba(255, 255, 255, 0.08);",
            y_align: Clutter.ActorAlign.FILL,
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

        const hoverBackground = options.hoverBackground ?? "rgba(255, 255, 255, 0.086)";
        const pressBackground = options.pressBackground ?? "rgba(255, 255, 255, 0.133)";
        const hoverBorder = options.hoverBorder ?? "transparent";
        const pressBorder = options.pressBorder ?? "transparent";
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
            width: 52,
            height: 38,
            borderRadius: 8,
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
            style: createRuntimeOverlayMenuStyle(RUNTIME_OVERLAY_MENU_WIDTH),
        }), "controls.overlay-menu");

        menu.add_child(new St.Label({
            text: "OVERLAYS",
            style: createRuntimeOverlayHeaderStyle(),
        }));

        this._runtimeOverlayToggleRows.clear();
        for (const spec of RUNTIME_OVERLAY_TOGGLE_SPECS)
            menu.add_child(this._createRuntimeOverlayToggleRow(spec));

        this._runtimeOverlayMenu = menu;
        const menuChromeParams = {trackFullscreen: false};
        if (!shellVersionAtLeast50())
            menuChromeParams.affectsInputRegion = true;
        Main.layoutManager.addChrome(this._runtimeOverlayMenu, menuChromeParams);
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
    }

    _showWarningPopup(message, nearButton) {
        this._hideWarningPopup();

        const popup = registerSelfOwnedActor(this._sessionState, new St.BoxLayout({
            vertical: true,
            reactive: false,
            style: createWarningPopupStyle(),
        }), "controls.warning-popup");

        popup.add_child(new St.Label({
            text: message,
            style: "font-size: 12px; color: #F1F1F3; text-align: left;",
        }));

        this._warningPopup = popup;
        this._warningPopupAnchor = nearButton;
        const warningChromeParams = {trackFullscreen: false};
        if (!shellVersionAtLeast50())
            warningChromeParams.affectsInputRegion = false;
        Main.layoutManager.addChrome(this._warningPopup, warningChromeParams);
        this._warningPopup.show();
        this._positionWarningPopup();

        // Auto-hide after 4 seconds
        this._warningPopupTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 4000, () => {
            this._hideWarningPopup();
            return GLib.SOURCE_REMOVE;
        });
    }

    _hideWarningPopup() {
        if (this._warningPopupTimeout) {
            GLib.source_remove(this._warningPopupTimeout);
            this._warningPopupTimeout = null;
        }
        if (this._warningPopup) {
            if (this._warningPopup.get_parent())
                Main.layoutManager.removeChrome(this._warningPopup);
            this._warningPopup.destroy();
            this._warningPopup = null;
        }
        this._warningPopupAnchor = null;
    }

    _positionWarningPopup() {
        if (!this._warningPopup || !this._warningPopupAnchor)
            return;

        const controlsState = this._sessionState.controlsState;
        if (!controlsState)
            return;

        const monitor = this._monitorForRect(controlsState.rect);
        const anchorRect = this._actorRectOnStage(this._warningPopupAnchor);
        if (!anchorRect)
            return;
        const [, popupWidth] = this._warningPopup.get_preferred_width(-1);
        const [, popupHeight] = this._warningPopup.get_preferred_height(popupWidth);
        const {x, y} = computeAdjacentPopupPosition({
            anchorRect,
            popupSize: {width: popupWidth, height: popupHeight},
            monitor,
            gap: 10,
            margin: 10,
        });

        this._warningPopup.set_position(x, y);
    }

    _createRuntimeOverlayToggleRow(spec) {
        const visible = getRuntimeOverlayVisibility(this._sessionState, spec.key);
        const supportMessage = getRuntimeOverlaySupportMessage(this._sessionState, spec.key);
        const rowStyles = createRuntimeOverlayRowStyles(Boolean(supportMessage));

        const button = registerSelfOwnedActor(this._sessionState, new St.Button({
            reactive: true,
            can_focus: true,
            track_hover: true,
            x_expand: true,
            style: rowStyles.button,
        }), `controls.overlay-toggle.${spec.key}`);

        const layout = new St.BoxLayout({
            vertical: false,
            reactive: false,
            x_expand: rowStyles.layout.expandHorizontally,
            style: rowStyles.layout.style,
        });

        const checkSlot = new St.BoxLayout({
            reactive: false,
            width: rowStyles.checkSlot.width,
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
        });

        const checkIcon = new St.Icon({
            icon_name: "object-select-symbolic",
            style: `icon-size: 14px; color: ${visible ? "#F1F1F3" : "transparent"};`,
            y_align: Clutter.ActorAlign.CENTER,
        });
        checkSlot.add_child(checkIcon);
        layout.add_child(checkSlot);

        const label = new St.Label({
            text: spec.label,
            x_expand: rowStyles.label.expandHorizontally,
            x_align: Clutter.ActorAlign.START,
            y_align: Clutter.ActorAlign.CENTER,
            style: rowStyles.label.style,
        });
        layout.add_child(label);

        let infoButton = null;
        if (supportMessage) {
            infoButton = registerSelfOwnedActor(this._sessionState, new St.Button({
                reactive: true,
                can_focus: true,
                track_hover: true,
                style: [
                    "margin-left: 4px;",
                    rowStyles.infoButton,
                ].join(" "),
            }), `controls.overlay-toggle-info.${spec.key}`);
            infoButton.set_child(new St.Label({
                text: "?",
                y_align: Clutter.ActorAlign.CENTER,
                x_align: Clutter.ActorAlign.CENTER,
                style: "font-size: 11px; font-weight: 700; color: rgba(255, 255, 255, 0.82);",
            }));
            infoButton.connect("clicked", () => {
                this._showWarningPopup(supportMessage, infoButton);
            });
            layout.add_child(infoButton);
        }

        button.set_child(layout);

        // Handle click - toggle if no support message, show popup if there is one
        button.connect("clicked", () => {
            if (supportMessage) {
                this._showWarningPopup(supportMessage, infoButton ?? button);
            } else {
                this._toggleRuntimeOverlay(spec.key);
            }
        });

        const baseStyle = rowStyles.baseButton;
        button.connect("notify::hover", () => {
            const currentVisible = getRuntimeOverlayVisibility(this._sessionState, spec.key);
            if (button.hover) {
                button.set_style(`${baseStyle} background-color: rgba(255, 255, 255, 0.06);`);
                label.set_style(rowStyles.label.hoverStyle || rowStyles.label.style);
                checkIcon.set_style(`icon-size: 14px; color: ${currentVisible ? "white" : "transparent"};`);
            } else {
                button.set_style(`${baseStyle} background-color: transparent;`);
                label.set_style(rowStyles.label.style);
                checkIcon.set_style(`icon-size: 14px; color: ${currentVisible ? "#F1F1F3" : "transparent"};`);
            }
        });

        this._runtimeOverlayToggleRows.set(spec.key, {button, layout, checkSlot, label, checkIcon, infoButton});
        return button;
    }

    _toggleRuntimeOverlay(key) {
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
            const rowStyles = createRuntimeOverlayRowStyles(Boolean(supportMessage));

            if (isHovered) {
                row.button.set_style(`${rowStyles.baseButton} background-color: rgba(255, 255, 255, 0.06);`);
                row.label.set_style(rowStyles.label.hoverStyle || rowStyles.label.style);
                row.checkIcon.set_style(`icon-size: 14px; color: ${visible ? "white" : "transparent"};`);
            } else {
                row.button.set_style(`${rowStyles.baseButton} background-color: transparent;`);
                row.label.set_style(rowStyles.label.style);
                row.checkIcon.set_style(`icon-size: 14px; color: ${visible ? "#F1F1F3" : "transparent"};`);
            }
        }
    }

    _actorRectOnStage(actor) {
        if (!actor)
            return null;

        const [x, y] = actor.get_transformed_position();
        return {
            x,
            y,
            width: actor.width,
            height: actor.height,
        };
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
        const x = Math.max(minX, Math.min(controlsX + this._controlsChrome.width - menuWidth, maxX));

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

    _computeControlsPosition(controlsState, monitor) {
        const barWidth = this._controlsChrome ? this._controlsChrome.width : CONTROLS_BAR_WIDTH_MIN;
        const position = computeControlsDockPosition({
            rect: controlsState.rect,
            isFullscreen: controlsState.isFullscreen,
            visibilityPolicy: controlsState.visibilityPolicy,
            monitor,
            controlsSize: {
                width: barWidth,
                height: CONTROLS_BAR_HEIGHT,
            },
        });
        return position ? [position.x, position.y] : [monitor.x, monitor.y];
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

    setPaused(sessionId, paused) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState || controlsState.sessionId !== sessionId)
            return;
        this._setControlsPaused(paused);
    }

    resetTimer(sessionId) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState || controlsState.sessionId !== sessionId)
            return;
        this._resetControlsTimer();
    }

    endSession(sessionId) {
        const controlsState = this._sessionState.controlsState;
        if (!controlsState || controlsState.sessionId !== sessionId)
            return;
        this.hideControls();
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
        if (!controlsState || !controlsState.showTimer)
            return;
        const formatted = this._formatElapsed(this._elapsedControlsMs());
        if (this._timerLabel)
            this._timerLabel.text = formatted;
        if (this._panelTimerLabel)
            this._panelTimerLabel.text = formatted;
    }

    _formatElapsed(elapsedMs) {
        const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
        const minutes = Math.floor(totalSeconds / 60);
        const seconds = totalSeconds % 60;
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }
}

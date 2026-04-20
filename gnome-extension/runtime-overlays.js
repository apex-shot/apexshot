// SPDX-License-Identifier: AGPL-3.0-or-later

import GLib from "gi://GLib";
import Gio from "gi://Gio";
import St from "gi://St";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import {createKeystrokeOverlayModel} from "./keystroke-display.js";
import {
    createRenderableRuntimeOverlayVisibility,
    hasRenderableRuntimeOverlays,
} from "./runtime-overlays-visibility.js";
import {
    createRuntimeOverlayVisibility,
    getRuntimeOverlayClickIndicator,
    getRuntimeOverlayKeystrokeText,
    getRuntimeOverlaySupportMessage,
    isSelfOwnedActor,
    registerSelfOwnedActor,
    setRuntimeOverlayWebcamPosition,
} from "./session-state.js";

const OVERLAY_MARGIN = 10;
const CLICK_INDICATOR_MARGIN = 18;
const KEYSTROKE_INDICATOR_MARGIN = 18;
const WEBCAM_SIZE_MAP = Object.freeze({
    0: {width: 120, height: 160},  // Small
    1: {width: 200, height: 260},  // Medium
    2: {width: 280, height: 370},  // Large
    3: {width: 360, height: 480},  // Huge
    // 4 = Fullscreen: handled specially in webcamPreviewSize()
});
const CLICK_COLOR_MAP = Object.freeze({
    0: "rgb(180, 180, 180)",
    1: "rgb(122, 100, 255)",
    2: "rgb(255, 60, 60)",
    3: "rgb(60, 120, 255)",
    4: "rgb(60, 200, 80)",
    5: "rgb(255, 210, 50)",
    6: "rgb(255, 150, 30)",
    7: "rgb(180, 60, 220)",
    8: "rgb(255, 255, 255)",
});

function ensureRuntimeOverlayState(sessionState) {
    if (!sessionState.runtimeOverlayState) {
        sessionState.runtimeOverlayState = {
            chrome: null,
            webcamActor: null,
            webcamFrameActor: null,
            webcamFrameLoadSerial: 0,
            webcamFrameImageUri: "",
            webcamLastFramePath: "",
            webcamLastSequence: -1,
            webcamPollSource: null,
            webcamAsyncInProgress: false,
            webcamDragging: false,
            webcamDragOffsetX: 0,
            webcamDragOffsetY: 0,
            webcamImageContent: null,
            webcamFrameWidth: 0,
            webcamFrameHeight: 0,
            clicksActor: null,
            clickPulseStackActor: null,
            clickPulseActor: null,
            clickHaloActor: null,
            clickLabelActor: null,
            lastAnimatedClickTimestampMs: -1,
            keystrokesActor: null,
            keystrokeLabelActor: null,
            visibility: createRuntimeOverlayVisibility(),
            keystrokeOverlay: createKeystrokeOverlayModel(),
            selfOwnedActors: new WeakSet(),
            selfOwnedActorOwners: new WeakMap(),
        };
    }

    const state = sessionState.runtimeOverlayState;
    state.visibility ??= createRuntimeOverlayVisibility();
    state.keystrokeOverlay ??= createKeystrokeOverlayModel();
    state.lastAnimatedClickTimestampMs ??= -1;
    state.selfOwnedActors ??= new WeakSet();
    state.selfOwnedActorOwners ??= new WeakMap();
    return state;
}

function createWebcamActor(sessionState, overlayState) {
    const actor = new St.Widget({
        reactive: true,
        can_focus: false,
        track_hover: true,
        clip_to_allocation: true,
        layout_manager: new Clutter.BinLayout(),
    });

    overlayState.webcamFrameActor = new St.Widget({
        reactive: false,
        x_expand: true,
        y_expand: true,
        clip_to_allocation: true,
    });
    actor.add_child(overlayState.webcamFrameActor);

    actor.connect("button-press-event", (_actor, event) => {
        const controlsState = sessionState.controlsState;
        const snapshot = sessionState.runtimeOverlaySnapshot;
        if (!controlsState || !snapshot)
            return Clutter.EVENT_PROPAGATE;

        const [stageX, stageY] = event.get_coords();
        const [actorX, actorY] = actor.get_position();
        overlayState.webcamDragging = true;
        overlayState.webcamDragOffsetX = stageX - actorX;
        overlayState.webcamDragOffsetY = stageY - actorY;
        return Clutter.EVENT_STOP;
    });

    actor.connect("motion-event", (_actor, event) => {
        if (!overlayState.webcamDragging)
            return Clutter.EVENT_PROPAGATE;

        const controlsState = sessionState.controlsState;
        const snapshot = sessionState.runtimeOverlaySnapshot;
        if (!controlsState || !snapshot)
            return Clutter.EVENT_STOP;

        const size = webcamPreviewSize(snapshot, controlsState.rect);
        const [stageX, stageY] = event.get_coords();
        const bounds = clampPlacement(
            controlsState.rect,
            stageX - overlayState.webcamDragOffsetX,
            stageY - overlayState.webcamDragOffsetY,
            size.width,
            size.height,
            OVERLAY_MARGIN
        );
        const minX = controlsState.rect.x + OVERLAY_MARGIN;
        const maxX = Math.max(minX, controlsState.rect.x + controlsState.rect.width - size.width - OVERLAY_MARGIN);
        const minY = controlsState.rect.y + OVERLAY_MARGIN;
        const maxY = Math.max(minY, controlsState.rect.y + controlsState.rect.height - size.height - OVERLAY_MARGIN);
        const relX = maxX > minX ? (bounds.x - minX) / (maxX - minX) : 0;
        const relY = maxY > minY ? 1 - ((bounds.y - minY) / (maxY - minY)) : 0;
        setRuntimeOverlayWebcamPosition(sessionState, relX, relY);
        updateRuntimeOverlaySnapshot(sessionState);
        return Clutter.EVENT_STOP;
    });

    const stopDragging = () => {
        overlayState.webcamDragging = false;
        return Clutter.EVENT_STOP;
    };
    actor.connect("button-release-event", stopDragging);
    actor.connect("leave-event", () => Clutter.EVENT_PROPAGATE);

    return actor;
}

function createClicksActor(overlayState) {
    const actor = new St.Widget({
        reactive: false,
        layout_manager: new Clutter.BinLayout(),
    });
    actor.opacity = 0;
    actor.visible = false;

    overlayState.clickPulseStackActor = new St.Widget({
        reactive: false,
        layout_manager: new Clutter.BinLayout(),
    });
    actor.add_child(overlayState.clickPulseStackActor);

    overlayState.clickHaloActor = new St.Widget({reactive: false});
    overlayState.clickPulseStackActor.add_child(overlayState.clickHaloActor);

    overlayState.clickPulseActor = new St.Widget({reactive: false});
    overlayState.clickPulseStackActor.add_child(overlayState.clickPulseActor);

    return actor;
}

function createKeystrokesActor(overlayState) {
    const actor = new St.BoxLayout({
        reactive: false,
        style: [
            "padding: 10px 16px;",
            "spacing: 10px;",
            "border-radius: 16px;",
        ].join(" "),
    });

    actor.add_child(new St.Icon({
        icon_name: "input-keyboard-symbolic",
        style: "icon-size: 17px;",
        y_align: Clutter.ActorAlign.CENTER,
    }));

    overlayState.keystrokeLabelActor = new St.Label({
        text: "Shift + A",
        y_align: Clutter.ActorAlign.CENTER,
        style: "font-size: 15px; font-weight: 700;",
    });
    actor.add_child(overlayState.keystrokeLabelActor);

    return actor;
}

function webcamPreviewSize(snapshot, rect) {
    // Fullscreen (size 4) uses the recording rect dimensions
    const isFullscreen = snapshot.webcam_size === 4;
    let width, height;

    if (isFullscreen) {
        width = Math.max(1, rect.width - (2 * OVERLAY_MARGIN));
        height = Math.max(1, rect.height - (2 * OVERLAY_MARGIN));
    } else {
        const base = WEBCAM_SIZE_MAP[snapshot.webcam_size] ?? WEBCAM_SIZE_MAP[1];
        width = base.width;
        height = base.height;
    }

    switch (snapshot.webcam_shape) {
    case 0:
    case 1:
        height = width;
        break;
    case 2:
        height = Math.round(width * 0.75);
        break;
    case 3:
        break;
    default:
        break;
    }

    const maxWidth = Math.max(1, rect.width - (2 * OVERLAY_MARGIN));
    const maxHeight = Math.max(1, rect.height - (2 * OVERLAY_MARGIN));
    return {
        width: Math.min(width, maxWidth),
        height: Math.min(height, maxHeight),
    };
}

function webcamBorderRadius(snapshot, width, height) {
    switch (snapshot.webcam_shape) {
    case 0:
        return Math.floor(Math.min(width, height) / 2);
    case 1:
        return 8;
    case 2:
        return 12;
    case 3:
        return 12;
    default:
        return 12;
    }
}

function clampPlacement(rect, desiredX, desiredY, width, height, margin) {
    const clampedWidth = Math.min(width, Math.max(1, rect.width - (2 * margin)));
    const clampedHeight = Math.min(height, Math.max(1, rect.height - (2 * margin)));
    const minX = rect.x + margin;
    const maxX = Math.max(minX, rect.x + rect.width - clampedWidth - margin);
    const minY = rect.y + margin;
    const maxY = Math.max(minY, rect.y + rect.height - clampedHeight - margin);

    return {
        x: Math.round(Math.min(maxX, Math.max(minX, desiredX))),
        y: Math.round(Math.min(maxY, Math.max(minY, desiredY))),
        width: Math.round(clampedWidth),
        height: Math.round(clampedHeight),
    };
}

function keyPositionCoords(snapshot, rect, width, height) {
    const margin = KEYSTROKE_INDICATOR_MARGIN;
    switch (snapshot.key_position) {
    case 0:
        return [rect.x + Math.floor((rect.width - width) / 2), rect.y + rect.height - height - margin];
    case 1:
        return [rect.x + margin, rect.y + rect.height - height - margin];
    case 2:
        return [rect.x + rect.width - width - margin, rect.y + rect.height - height - margin];
    case 3:
        return [rect.x + Math.floor((rect.width - width) / 2), rect.y + margin];
    case 4:
        return [rect.x + margin, rect.y + margin];
    case 5:
        return [rect.x + rect.width - width - margin, rect.y + margin];
    default:
        return [rect.x + Math.floor((rect.width - width) / 2), rect.y + rect.height - height - margin];
    }
}

function setActorVisible(actor, visible) {
    if (!actor)
        return;
    actor.visible = visible;
}

function updateChromeSize(overlayState) {
    overlayState.chrome?.set_position(0, 0);
    overlayState.chrome?.set_size(global.stage.width, global.stage.height);
}

function loadWebcamPreviewManifestAsync(path, callback) {
    if (!path) {
        callback(null);
        return;
    }

    const file = Gio.File.new_for_path(path);
    file.load_contents_async(null, (source, result) => {
        try {
            const [, bytes] = source.load_contents_finish(result);
            const text = new TextDecoder().decode(bytes);
            const parsed = JSON.parse(text);
            if (!parsed || typeof parsed !== "object") {
                callback(null);
                return;
            }
            if (typeof parsed.sequence !== "number" || typeof parsed.frame_path !== "string") {
                callback(null);
                return;
            }
            callback(parsed);
        } catch (_) {
            callback(null);
        }
    });
}

function applyWebcamPreviewFrame(overlayState, framePath) {
    const file = Gio.File.new_for_path(framePath);
    const width = Math.max(1, overlayState.webcamActor.width);
    const height = Math.max(1, overlayState.webcamActor.height);
    
    const texture = St.TextureCache.get_default().load_file_async(file, -1, height, 1, 1);
    texture.reactive = false;

    const applyContent = () => {
        if (!texture.content || !overlayState.webcamFrameActor) {
            return;
        }

        // Transfer content to frame actor
        const content = texture.content;
        overlayState.webcamFrameActor.set({
            content,
            width,
            height,
            contentGravity: Clutter.ContentGravity.RESIZE_ASPECT_FILL,
        });
        overlayState.webcamLastFramePath = framePath;
        
        // Clear the temp texture's content so it doesn't destroy our content
        texture.content = null;
        texture.destroy();
    };

    if (texture.content) {
        applyContent();
    } else {
        texture.connect("notify::content", applyContent);
    }
}

function ensureWebcamPreviewPolling(sessionState, overlayState) {
    if (overlayState.webcamPollSource != null)
        return;

    overlayState.webcamPollSource = GLib.timeout_add(GLib.PRIORITY_HIGH_IDLE, 33, () => {
        const snapshot = sessionState.runtimeOverlaySnapshot;
        if (!snapshot?.webcam_preview_manifest_path || !overlayState.webcamActor) {
            overlayState.webcamPollSource = null;
            return GLib.SOURCE_REMOVE;
        }

        if (!overlayState.webcamAsyncInProgress) {
            overlayState.webcamAsyncInProgress = true;
            loadWebcamPreviewManifestAsync(snapshot.webcam_preview_manifest_path, (manifest) => {
                overlayState.webcamAsyncInProgress = false;
                if (manifest && manifest.sequence !== overlayState.webcamLastSequence) {
                    overlayState.webcamLastSequence = manifest.sequence;
                    try {
                        applyWebcamPreviewFrame(overlayState, manifest.frame_path);
                    } catch (error) {
                        logError(error, `[apexshot] webcam preview apply failed path=${manifest.frame_path}`);
                    }
                }
            });
        }
        return GLib.SOURCE_CONTINUE;
    });
}

function stopWebcamPreviewPolling(overlayState) {
    if (typeof overlayState.webcamPollSource === "number" && overlayState.webcamPollSource > 0)
        GLib.source_remove(overlayState.webcamPollSource);
    overlayState.webcamPollSource = null;
}

function updateWebcamActor(overlayState, snapshot, rect) {
    const visible = createRenderableRuntimeOverlayVisibility(overlayState.visibility).webcam;
    setActorVisible(overlayState.webcamActor, visible);
    if (!visible) {
        stopWebcamPreviewPolling(overlayState);
        return;
    }

    const size = webcamPreviewSize(snapshot, rect);
    const radius = webcamBorderRadius(snapshot, size.width, size.height);
    const minX = rect.x + OVERLAY_MARGIN;
    const maxX = Math.max(minX, rect.x + rect.width - size.width - OVERLAY_MARGIN);
    const minY = rect.y + OVERLAY_MARGIN;
    const maxY = Math.max(minY, rect.y + rect.height - size.height - OVERLAY_MARGIN);
    const x = Math.round(minX + ((maxX - minX) * snapshot.webcam_rel_x));
    const y = Math.round(minY + ((maxY - minY) * (1 - snapshot.webcam_rel_y)));

    overlayState.webcamActor.set_size(size.width, size.height);
    overlayState.webcamActor.set_position(x, y);

    // Apply border radius with overflow:hidden to clip the frame content
    if (overlayState.webcamFrameActor) {
        overlayState.webcamFrameActor.set_style([
            `border-radius: ${radius}px;`,
            "overflow: hidden;",
        ].join(" "));
    }

    // Apply border outline to main actor (matches C++: 1.5px white at 40/255 alpha ~16%)
    overlayState.webcamActor.set_style([
        "background-color: transparent;",
        `border-radius: ${radius}px;`,
        "border: 1.5px solid rgba(255, 255, 255, 0.16);",
    ].join(" "));

    ensureWebcamPreviewPolling({runtimeOverlaySnapshot: snapshot}, overlayState);
}

function updateClicksActor(overlayState, snapshot, rect) {
    const visible = createRenderableRuntimeOverlayVisibility(overlayState.visibility).clicks;
    const click = getRuntimeOverlayClickIndicator(
        {runtimeOverlayState: overlayState},
        Math.floor(GLib.get_monotonic_time() / 1000)
    );
    if (!visible || !click) {
        if (!visible) {
            overlayState.clicksActor.remove_all_transitions();
            overlayState.clickHaloActor.remove_all_transitions();
            overlayState.clickPulseActor.remove_all_transitions();
            overlayState.clicksActor.opacity = 0;
            overlayState.clicksActor.hide();
        }
        return;
    }

    if (overlayState.lastAnimatedClickTimestampMs === click.timestampMs)
        return;

    overlayState.lastAnimatedClickTimestampMs = click.timestampMs;

    const clickSize = 12 + Math.round(snapshot.click_size * 56);
    const haloSize = clickSize + 18 + Math.round(snapshot.click_size * 10);
    const clickColor = CLICK_COLOR_MAP[snapshot.click_color] ?? CLICK_COLOR_MAP[0];
    let borderWidth = 2;
    let fillColor = "rgba(255, 255, 255, 0.06)";
    let innerOpacity = 230;
    let haloOpacity = 120;

    if (snapshot.click_style === 1) {
        borderWidth = 0;
        fillColor = clickColor;
        innerOpacity = 210;
        haloOpacity = 92;
    } else if (snapshot.click_style >= 2) {
        borderWidth = 3;
        fillColor = "rgba(255, 255, 255, 0.12)";
        innerOpacity = 240;
        haloOpacity = 138;
    }

    overlayState.clickHaloActor.set_size(haloSize, haloSize);
    overlayState.clickHaloActor.set_style([
        `width: ${haloSize}px;`,
        `height: ${haloSize}px;`,
        `border-radius: ${Math.floor(haloSize / 2)}px;`,
        `border: 2px solid ${clickColor};`,
        "background-color: transparent;",
    ].join(" "));

    overlayState.clickPulseActor.set_size(clickSize, clickSize);
    overlayState.clickPulseActor.set_style([
        `width: ${clickSize}px;`,
        `height: ${clickSize}px;`,
        `border-radius: ${Math.floor(clickSize / 2)}px;`,
        borderWidth > 0 ? `border: ${borderWidth}px solid ${clickColor};` : "border: none;",
        `background-color: ${fillColor};`,
    ].join(" "));
    overlayState.clickPulseStackActor.set_size(haloSize, haloSize);
    const bounds = clampPlacement(
        rect,
        click.x - Math.floor(haloSize / 2),
        click.y - Math.floor(haloSize / 2),
        haloSize,
        haloSize,
        CLICK_INDICATOR_MARGIN
    );
    overlayState.clicksActor.set_size(bounds.width, bounds.height);
    overlayState.clicksActor.set_position(bounds.x, bounds.y);
    overlayState.clickPulseStackActor.set_position(0, 0);

    overlayState.clicksActor.remove_all_transitions();
    overlayState.clickHaloActor.remove_all_transitions();
    overlayState.clickPulseActor.remove_all_transitions();
    overlayState.clicksActor.opacity = 255;
    overlayState.clicksActor.show();

    overlayState.clickPulseStackActor.set_scale(snapshot.click_animate ? 0.82 : 1.0, snapshot.click_animate ? 0.82 : 1.0);
    overlayState.clickPulseStackActor.opacity = 255;
    overlayState.clickHaloActor.opacity = snapshot.click_animate ? haloOpacity : 0;
    overlayState.clickPulseActor.opacity = innerOpacity;

    const durationMs = snapshot.click_animate ? 170 : 110;
    if (snapshot.click_animate) {
        overlayState.clickPulseStackActor.ease({
            scale_x: 1.18,
            scale_y: 1.18,
            duration: durationMs,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
        overlayState.clickHaloActor.ease({
            opacity: 0,
            duration: durationMs,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
    } else {
        overlayState.clickHaloActor.opacity = 0;
    }
    overlayState.clickPulseActor.ease({
        opacity: 0,
        duration: durationMs,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
    });
    overlayState.clicksActor.ease({
        opacity: 0,
        duration: durationMs,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        onComplete: () => {
            overlayState.clicksActor.hide();
        },
    });
}

function updateKeystrokesActor(sessionState, overlayState, snapshot, rect) {
    const visible = createRenderableRuntimeOverlayVisibility(overlayState.visibility).keystrokes;
    setActorVisible(overlayState.keystrokesActor, visible);
    if (!visible)
        return;

    const liveText = getRuntimeOverlayKeystrokeText(
        sessionState,
        Math.floor(GLib.get_monotonic_time() / 1000)
    );
    const supportMessage = getRuntimeOverlaySupportMessage(sessionState, "keystrokes");
    const displayText = liveText || supportMessage;
    const darkAppearance = snapshot.key_appearance === 0;
    const backgroundColor = darkAppearance
        ? (snapshot.key_blur_bg ? "rgba(20, 20, 24, 0.48)" : "rgba(20, 20, 24, 0.9)")
        : (snapshot.key_blur_bg ? "rgba(245, 245, 250, 0.48)" : "rgba(245, 245, 250, 0.9)");
    const textColor = darkAppearance ? "rgb(255, 255, 255)" : "rgb(20, 20, 24)";
    const scale = 0.85 + (snapshot.key_size * 0.75);
    const textWidth = displayText
        ? Math.round((Math.min(displayText.length, 42) * 8.5 + 60) * scale)
        : 0;
    const width = Math.max(Math.round(124 * scale), textWidth);
    const height = Math.round(46 * scale);
    const [rawX, rawY] = keyPositionCoords(snapshot, rect, width, height);
    const bounds = clampPlacement(
        rect,
        rawX,
        rawY,
        width,
        height,
        KEYSTROKE_INDICATOR_MARGIN
    );

    overlayState.keystrokesActor.set_size(bounds.width, bounds.height);
    overlayState.keystrokesActor.set_position(bounds.x, bounds.y);
    overlayState.keystrokesActor.set_style([
        `background-color: ${backgroundColor};`,
        `color: ${textColor};`,
        `border: 1px solid ${darkAppearance ? "rgba(255, 255, 255, 0.16)" : "rgba(20, 20, 24, 0.12)"};`,
        `border-radius: ${Math.round(12 * scale)}px;`,
        `padding: ${Math.round(10 * scale)}px ${Math.round(16 * scale)}px;`,
        `spacing: ${Math.round(10 * scale)}px;`,
        snapshot.key_blur_bg ? "box-shadow: 0 12px 24px rgba(0, 0, 0, 0.18);" : "",
    ].join(" "));
    overlayState.keystrokeLabelActor.text = displayText;
    overlayState.keystrokeLabelActor.set_style([
        `font-size: ${Math.round(15 * scale)}px;`,
        "font-weight: 700;",
        `color: ${textColor};`,
    ].join(" "));
}

export function attachRuntimeOverlays(sessionState) {
    if (!sessionState?.runtimeOverlaySnapshot)
        return null;

    const overlayState = ensureRuntimeOverlayState(sessionState);
    if (!hasRenderableRuntimeOverlays(overlayState.visibility)) {
        destroyRuntimeOverlays(sessionState);
        return null;
    }

    if (overlayState.chrome)
        return overlayState;

    overlayState.chrome = new St.Widget({
        reactive: false,
        clip_to_allocation: false,
    });
    updateChromeSize(overlayState);

    overlayState.webcamActor = createWebcamActor(sessionState, overlayState);
    overlayState.clicksActor = createClicksActor(overlayState);
    overlayState.keystrokesActor = createKeystrokesActor(overlayState);

    overlayState.chrome.add_child(overlayState.webcamActor);
    overlayState.chrome.add_child(overlayState.clicksActor);
    overlayState.chrome.add_child(overlayState.keystrokesActor);

    registerSelfOwnedActor(sessionState, overlayState.chrome, "runtime-overlay.chrome");
    registerSelfOwnedActor(sessionState, overlayState.webcamActor, "runtime-overlay.webcam");
    registerSelfOwnedActor(sessionState, overlayState.clicksActor, "runtime-overlay.clicks");
    registerSelfOwnedActor(sessionState, overlayState.keystrokesActor, "runtime-overlay.keystrokes");

    Main.layoutManager.addChrome(overlayState.chrome, {
        affectsInputRegion: false,
        trackFullscreen: false,
    });
    overlayState.chrome.show();
    return overlayState;
}

export function updateRuntimeOverlaySnapshot(sessionState) {
    const snapshot = sessionState?.runtimeOverlaySnapshot;
    if (!snapshot) {
        destroyRuntimeOverlays(sessionState);
        return;
    }

    const controlsState = sessionState.controlsState;
    const rect = controlsState?.rect;
    if (!rect) {
        destroyRuntimeOverlays(sessionState);
        return;
    }

    const overlayState = attachRuntimeOverlays(sessionState);
    if (!overlayState)
        return;

    updateChromeSize(overlayState);
    updateWebcamActor(overlayState, snapshot, rect);
    updateClicksActor(overlayState, snapshot, rect);
    updateKeystrokesActor(sessionState, overlayState, snapshot, rect);
}

export function destroyRuntimeOverlays(sessionState) {
    if (!sessionState)
        return;

    const overlayState = ensureRuntimeOverlayState(sessionState);
    stopWebcamPreviewPolling(overlayState);
    if (overlayState.chrome) {
        if (overlayState.chrome.get_parent())
            Main.layoutManager.removeChrome(overlayState.chrome);
        overlayState.chrome.destroy();
    }

    overlayState.chrome = null;
    overlayState.webcamActor = null;
    overlayState.webcamFrameActor = null;
    overlayState.webcamFrameLoadSerial = 0;
    overlayState.webcamFrameImageUri = "";
    overlayState.webcamLastFramePath = "";
    overlayState.webcamLastSequence = -1;
    overlayState.webcamAsyncInProgress = false;
    overlayState.clicksActor = null;
    overlayState.clickPulseStackActor = null;
    overlayState.clickPulseActor = null;
    overlayState.clickHaloActor = null;
    overlayState.clickLabelActor = null;
    overlayState.keystrokesActor = null;
    overlayState.keystrokeLabelActor = null;
}

export function shouldExcludeOverlayEvent(sessionState, target) {
    return isSelfOwnedActor(sessionState, target);
}

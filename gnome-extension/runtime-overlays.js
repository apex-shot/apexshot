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
// Click colours mirror the C++ recording overlay palette so the live
// click marker on screen is visually identical to the preview the user
// configured (see capture-overlay/src/CaptureOverlay_Drawing.cpp).
const CLICK_COLOR_MAP = Object.freeze({
    0: {r: 180, g: 180, b: 180}, // Gray
    1: {r: 122, g: 100, b: 255}, // Indigo
    2: {r: 255, g: 60,  b: 60},  // Red
    3: {r: 60,  g: 120, b: 255}, // Blue
    4: {r: 60,  g: 200, b: 80},  // Green
    5: {r: 255, g: 210, b: 50},  // Yellow
    6: {r: 255, g: 150, b: 30},  // Orange
    7: {r: 180, g: 60,  b: 220}, // Purple
    8: {r: 255, g: 255, b: 255}, // White
});

function clickRgb(color) {
    return `rgb(${color.r}, ${color.g}, ${color.b})`;
}

function clickRgba(color, alpha) {
    return `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha})`;
}

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
            // Stage-level signal IDs used while a webcam drag is in
            // progress. We listen on `global.stage` for motion / release
            // (instead of only on the actor) so a fast pointer movement or a
            // release that lands on another widget never leaves the webcam
            // stuck to the cursor.
            webcamStageMotionId: 0,
            webcamStageReleaseId: 0,
            webcamImageContent: null,
            webcamFrameWidth: 0,
            webcamFrameHeight: 0,
            clicksActor: null,
            clickPulseStackActor: null,
            clickPulseActor: null,
            clickHaloActor: null,
            clickMarkerActor: null,
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

    // Apply the current pointer position to the webcam actor. Shared by
    // the actor's own motion handler (slow drags inside the widget) and
    // the stage-level capture installed during an active drag (fast drags
    // that move the pointer outside the widget bounds).
    const applyDragPosition = (stageX, stageY) => {
        const controlsState = sessionState.controlsState;
        const snapshot = sessionState.runtimeOverlaySnapshot;
        if (!controlsState || !snapshot)
            return false;

        const size = webcamPreviewSize(snapshot, controlsState.rect);
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
        return true;
    };

    const stopDragging = () => {
        overlayState.webcamDragging = false;
        if (overlayState.webcamStageMotionId) {
            global.stage.disconnect(overlayState.webcamStageMotionId);
            overlayState.webcamStageMotionId = 0;
        }
        if (overlayState.webcamStageReleaseId) {
            global.stage.disconnect(overlayState.webcamStageReleaseId);
            overlayState.webcamStageReleaseId = 0;
        }
        return Clutter.EVENT_STOP;
    };

    actor.connect("button-press-event", (_actor, event) => {
        const controlsState = sessionState.controlsState;
        const snapshot = sessionState.runtimeOverlaySnapshot;
        if (!controlsState || !snapshot)
            return Clutter.EVENT_PROPAGATE;

        // Defensively clear any leftover stage-level handlers before we
        // start a new drag, in case a previous drag's release was somehow
        // missed (e.g. the pointer was grabbed by another window).
        stopDragging();

        const [stageX, stageY] = event.get_coords();
        const [actorX, actorY] = actor.get_position();
        overlayState.webcamDragging = true;
        overlayState.webcamDragOffsetX = stageX - actorX;
        overlayState.webcamDragOffsetY = stageY - actorY;

        // Track motion + release on the stage so the drag follows the
        // pointer reliably even when it leaves the actor's bounds, and so
        // the release is always observed regardless of which widget is
        // under the cursor when the user lets go.
        overlayState.webcamStageMotionId = global.stage.connect(
            "motion-event",
            (_stage, ev) => {
                if (!overlayState.webcamDragging)
                    return Clutter.EVENT_PROPAGATE;
                const [sx, sy] = ev.get_coords();
                applyDragPosition(sx, sy);
                return Clutter.EVENT_STOP;
            }
        );
        overlayState.webcamStageReleaseId = global.stage.connect(
            "button-release-event",
            () => stopDragging()
        );

        return Clutter.EVENT_STOP;
    });

    actor.connect("motion-event", (_actor, event) => {
        if (!overlayState.webcamDragging)
            return Clutter.EVENT_PROPAGATE;
        const [stageX, stageY] = event.get_coords();
        applyDragPosition(stageX, stageY);
        return Clutter.EVENT_STOP;
    });

    actor.connect("button-release-event", stopDragging);
    actor.connect("leave-event", () => Clutter.EVENT_PROPAGATE);
    // If the actor itself is destroyed mid-drag (e.g. the recording
    // session ends), make sure we don't leave the stage handlers dangling
    // — they would otherwise keep the actor stuck to the pointer for any
    // future overlay state object that re-reads `webcamDragging`.
    actor.connect("destroy", () => stopDragging());

    return actor;
}

function createClicksActor(overlayState) {
    // Layered structure (back to front), centered via BinLayout so each
    // child can have its own size and pivot:
    //   clickHaloActor   → soft radial glow behind the marker
    //   clickPulseActor  → animated expanding ring (when click_animate)
    //   clickMarkerActor → the filled / outlined click circle itself
    //
    // The container (`clicksActor`) and its inner stack
    // (`clickPulseStackActor`) are sized to the largest element (the
    // expanded pulse ring) so the ring can grow without being clipped.
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

    overlayState.clickMarkerActor = new St.Widget({reactive: false});
    overlayState.clickPulseStackActor.add_child(overlayState.clickMarkerActor);

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

        // Remove previous frame child if any
        const oldChild = overlayState.webcamFrameActor.get_first_child();
        if (oldChild)
            oldChild.destroy();

        // Add texture as child so parent's border-radius + overflow:hidden clips it
        texture.set_size(width, height);
        texture.set({
            x_expand: true,
            y_expand: true,
            contentGravity: Clutter.ContentGravity.RESIZE_ASPECT_FILL,
        });
        overlayState.webcamFrameActor.add_child(texture);
        overlayState.webcamLastFramePath = framePath;
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
            overlayState.clickMarkerActor?.remove_all_transitions();
            overlayState.clicksActor.opacity = 0;
            overlayState.clicksActor.hide();
        }
        return;
    }

    if (overlayState.lastAnimatedClickTimestampMs === click.timestampMs)
        return;

    overlayState.lastAnimatedClickTimestampMs = click.timestampMs;

    // ── Geometry ─────────────────────────────────────────────────────────
    // markerSize is the diameter of the visible click marker. Mirrors the
    // C++ preview's `baseRadius = 6 + click_size * 28` doubled (the preview
    // is rendered at ~half scale inside the configuration panel), so a
    // user-set size of 0.5 → 40 px on screen.
    const markerSize = 12 + Math.round(snapshot.click_size * 56);
    const markerRadius = Math.floor(markerSize / 2);
    // Halo is the soft radial glow behind the marker. The C++ preview uses
    // `baseRadius * 2.4` for the gradient extent; we reproduce that with a
    // box-shadow whose blur radius scales the same way.
    const haloSize = markerSize;
    const haloRadius = Math.floor(haloSize / 2);
    // Maximum extent the animated pulse ring grows to. Keep the container
    // big enough for it to expand without clipping.
    const pulseMaxScale = 1.85;
    const containerSize = Math.ceil(markerSize * pulseMaxScale) + 24;
    const containerHalf = Math.floor(containerSize / 2);

    const colorRgb = CLICK_COLOR_MAP[snapshot.click_color] ?? CLICK_COLOR_MAP[0];
    const colorString = clickRgb(colorRgb);
    const isFilled = snapshot.click_style === 1;

    // ── Halo: soft radial glow, identical to the gradient-halo in the C++
    // preview. Implemented as a transparent circle with a coloured
    // box-shadow so it works on any GNOME Shell version (no reliance on
    // CSS radial-gradient support in St). ────────────────────────────────
    const glowSpread = Math.max(2, Math.round(markerSize * 0.18));
    const glowBlur = Math.max(8, Math.round(markerSize * 0.85));
    overlayState.clickHaloActor.set_size(haloSize, haloSize);
    overlayState.clickHaloActor.set_style([
        `width: ${haloSize}px;`,
        `height: ${haloSize}px;`,
        `border-radius: ${haloRadius}px;`,
        "background-color: transparent;",
        "border: none;",
        `box-shadow: 0 0 ${glowBlur}px ${glowSpread}px ${clickRgba(colorRgb, 0.34)};`,
    ].join(" "));

    // ── Pulse ring: expanding outline that travels outward when the user
    // enabled the "Animate clicks" option. Matches the per-frame ring in
    // the C++ preview (`baseRadius + phase * 30`). ──────────────────────
    overlayState.clickPulseActor.set_size(markerSize, markerSize);
    overlayState.clickPulseActor.set_style([
        `width: ${markerSize}px;`,
        `height: ${markerSize}px;`,
        `border-radius: ${markerRadius}px;`,
        "background-color: transparent;",
        `border: 2.4px solid ${colorString};`,
    ].join(" "));

    // ── Marker: the actual click indicator. Two styles, each chosen to
    // read clearly on any background:
    //   • Filled  → solid colour disc with a thin white inner rim and a
    //               soft drop shadow that lifts it off light pixels.
    //   • Outline → 3 px coloured ring with a faint translucent fill that
    //               keeps the centre legible against busy backgrounds. ──
    overlayState.clickMarkerActor.set_size(markerSize, markerSize);
    overlayState.clickMarkerActor.set_style((
        isFilled
            ? [
                `width: ${markerSize}px;`,
                `height: ${markerSize}px;`,
                `border-radius: ${markerRadius}px;`,
                `background-color: ${colorString};`,
                `border: 1.5px solid rgba(255, 255, 255, 0.55);`,
                `box-shadow: 0 2px 10px rgba(0, 0, 0, 0.42);`,
            ]
            : [
                `width: ${markerSize}px;`,
                `height: ${markerSize}px;`,
                `border-radius: ${markerRadius}px;`,
                `background-color: ${clickRgba(colorRgb, 0.16)};`,
                `border: 3px solid ${colorString};`,
                `box-shadow: 0 2px 10px rgba(0, 0, 0, 0.42);`,
            ]
    ).join(" "));

    // ── Layout / positioning ────────────────────────────────────────────
    overlayState.clickPulseStackActor.set_size(containerSize, containerSize);
    overlayState.clickPulseStackActor.set_position(0, 0);
    const bounds = clampPlacement(
        rect,
        click.x - containerHalf,
        click.y - containerHalf,
        containerSize,
        containerSize,
        CLICK_INDICATOR_MARGIN
    );
    overlayState.clicksActor.set_size(bounds.width, bounds.height);
    overlayState.clicksActor.set_position(bounds.x, bounds.y);

    // ── Animation ───────────────────────────────────────────────────────
    overlayState.clicksActor.remove_all_transitions();
    overlayState.clickHaloActor.remove_all_transitions();
    overlayState.clickPulseActor.remove_all_transitions();
    overlayState.clickMarkerActor.remove_all_transitions();

    overlayState.clickHaloActor.set_pivot_point(0.5, 0.5);
    overlayState.clickPulseActor.set_pivot_point(0.5, 0.5);
    overlayState.clickMarkerActor.set_pivot_point(0.5, 0.5);

    overlayState.clicksActor.opacity = 255;
    overlayState.clicksActor.show();

    // Marker pops in slightly under-sized then settles to 1.0 — feels like
    // a real button press without being cartoonish.
    overlayState.clickMarkerActor.set_scale(0.78, 0.78);
    overlayState.clickMarkerActor.opacity = 255;
    overlayState.clickMarkerActor.ease({
        scale_x: 1.0,
        scale_y: 1.0,
        duration: 130,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
    });

    // Halo blooms briefly, then fades. Stays subtle when animation is off
    // so it doesn't draw attention to itself.
    overlayState.clickHaloActor.set_scale(0.85, 0.85);
    overlayState.clickHaloActor.opacity = snapshot.click_animate ? 220 : 160;
    overlayState.clickHaloActor.ease({
        scale_x: 1.0,
        scale_y: 1.0,
        duration: 140,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
    });

    // Pulse ring only renders when the user opted in.
    if (snapshot.click_animate) {
        overlayState.clickPulseActor.show();
        overlayState.clickPulseActor.set_scale(1.0, 1.0);
        overlayState.clickPulseActor.opacity = 210;
        overlayState.clickPulseActor.ease({
            scale_x: pulseMaxScale,
            scale_y: pulseMaxScale,
            duration: 480,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
        overlayState.clickPulseActor.ease({
            opacity: 0,
            duration: 480,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
    } else {
        overlayState.clickPulseActor.opacity = 0;
        overlayState.clickPulseActor.set_scale(1.0, 1.0);
    }

    // Coordinated fade-out of every layer. The total visible time for the
    // marker is ~440 ms (animated) / ~280 ms (static) — long enough to
    // register on a recording at typical frame rates without bleeding into
    // the next click.
    const fadeDuration = snapshot.click_animate ? 360 : 220;
    const fadeDelay = snapshot.click_animate ? 80 : 40;
    overlayState.clickMarkerActor.ease({
        opacity: 0,
        duration: fadeDuration,
        delay: fadeDelay,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
    });
    overlayState.clickHaloActor.ease({
        opacity: 0,
        duration: fadeDuration,
        delay: fadeDelay,
        mode: Clutter.AnimationMode.EASE_OUT_QUAD,
    });
    overlayState.clicksActor.ease({
        opacity: 0,
        duration: fadeDuration,
        delay: fadeDelay,
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
    overlayState.clickMarkerActor = null;
    overlayState.clickLabelActor = null;
    overlayState.keystrokesActor = null;
    overlayState.keystrokeLabelActor = null;
}

export function shouldExcludeOverlayEvent(sessionState, target) {
    return isSelfOwnedActor(sessionState, target);
}

// SPDX-License-Identifier: AGPL-3.0-or-later

function cloneRect(rect) {
    if (!rect)
        return null;

    return {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
}

const DEFAULT_RUNTIME_OVERLAY_SNAPSHOT = Object.freeze({
    mic_visible: false,
    speaker_visible: false,
    webcam_enabled: false,
    webcam_rel_x: 0,
    webcam_rel_y: 0,
    webcam_size: 1,
    webcam_shape: 3,
    webcam_flip: false,
    webcam_device: -1,
    clicks_enabled: false,
    click_size: 0.3,
    click_color: 0,
    click_style: 0,
    click_animate: true,
    keystrokes_enabled: false,
    key_size: 0.32,
    key_position: 0,
    key_appearance: 0,
    key_blur_bg: true,
    key_filter: 0,
});

function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
}

function normalizeBoolean(value, fallback) {
    return typeof value === "boolean" ? value : fallback;
}

function normalizeNumber(value, fallback, min, max) {
    if (!Number.isFinite(value))
        return fallback;
    return clamp(value, min, max);
}

function normalizeInteger(value, fallback, min, max) {
    if (!Number.isFinite(value))
        return fallback;
    return clamp(Math.trunc(value), min, max);
}

function createRuntimeOverlayVisibility(snapshot = null) {
    return {
        mic: snapshot?.mic_visible ?? false,
        speaker: snapshot?.speaker_visible ?? false,
        webcam: snapshot?.webcam_enabled ?? false,
        clicks: snapshot?.clicks_enabled ?? false,
        keystrokes: snapshot?.keystrokes_enabled ?? false,
    };
}

function createRuntimeOverlayState() {
    return {
        chrome: null,
        webcamActor: null,
        clicksActor: null,
        keystrokesActor: null,
        audioIndicatorsActor: null,
        micIndicatorActor: null,
        speakerIndicatorActor: null,
        visibility: createRuntimeOverlayVisibility(),
    };
}

function applyRuntimeOverlayVisibility(sessionState, snapshot) {
    sessionState.runtimeOverlayState.visibility = createRuntimeOverlayVisibility(snapshot);
}

export function parseRuntimeOverlaySnapshot(payload) {
    if (!payload)
        return null;

    let parsed = payload;
    if (typeof payload === "string") {
        try {
            parsed = JSON.parse(payload);
        } catch (_) {
            return null;
        }
    }

    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
        return null;

    const snapshot = {
        mic_visible: normalizeBoolean(parsed.mic_visible, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.mic_visible),
        speaker_visible: normalizeBoolean(parsed.speaker_visible, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.speaker_visible),
        webcam_enabled: normalizeBoolean(parsed.webcam_enabled, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_enabled),
        webcam_rel_x: normalizeNumber(parsed.webcam_rel_x, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_rel_x, 0, 1),
        webcam_rel_y: normalizeNumber(parsed.webcam_rel_y, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_rel_y, 0, 1),
        webcam_size: normalizeInteger(parsed.webcam_size, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_size, 0, 4),
        webcam_shape: normalizeInteger(parsed.webcam_shape, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_shape, 0, 3),
        webcam_flip: normalizeBoolean(parsed.webcam_flip, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_flip),
        webcam_device: Number.isFinite(parsed.webcam_device)
            ? Math.trunc(parsed.webcam_device)
            : DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.webcam_device,
        clicks_enabled: normalizeBoolean(parsed.clicks_enabled, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.clicks_enabled),
        click_size: normalizeNumber(parsed.click_size, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.click_size, 0, 1),
        click_color: normalizeInteger(parsed.click_color, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.click_color, 0, 8),
        click_style: normalizeInteger(parsed.click_style, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.click_style, 0, 1),
        click_animate: normalizeBoolean(parsed.click_animate, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.click_animate),
        keystrokes_enabled: normalizeBoolean(parsed.keystrokes_enabled, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.keystrokes_enabled),
        key_size: normalizeNumber(parsed.key_size, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.key_size, 0, 1),
        key_position: normalizeInteger(parsed.key_position, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.key_position, 0, 5),
        key_appearance: normalizeInteger(parsed.key_appearance, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.key_appearance, 0, 1),
        key_blur_bg: normalizeBoolean(parsed.key_blur_bg, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.key_blur_bg),
        key_filter: normalizeInteger(parsed.key_filter, DEFAULT_RUNTIME_OVERLAY_SNAPSHOT.key_filter, 0, 1),
    };

    return Object.freeze(snapshot);
}

export function createSessionState() {
    return {
        currentRect: null,
        controlsState: null,
        runtimeOverlaySnapshot: null,
        runtimeOverlayState: createRuntimeOverlayState(),
        shortcutEditActive: false,
    };
}

export function setCurrentRect(sessionState, rect) {
    sessionState.currentRect = cloneRect(rect);
    return sessionState.currentRect;
}

export function clearCurrentRect(sessionState) {
    sessionState.currentRect = null;
}

export function setRuntimeOverlaySnapshot(sessionState, payload) {
    const runtimeOverlaySnapshot = parseRuntimeOverlaySnapshot(payload);
    sessionState.runtimeOverlaySnapshot = runtimeOverlaySnapshot;
    if (sessionState.controlsState)
        sessionState.controlsState.runtimeOverlaySnapshot = runtimeOverlaySnapshot;
    applyRuntimeOverlayVisibility(sessionState, runtimeOverlaySnapshot);
    return runtimeOverlaySnapshot;
}

export function setControlsState(sessionState, spec, runningStartMs) {
    const rect = cloneRect(spec.rect);
    const runtimeOverlaySnapshot = setRuntimeOverlaySnapshot(sessionState, spec.runtimeOverlaySnapshot);

    sessionState.controlsState = {
        dbusDest: spec.dbusDest,
        sessionId: spec.sessionId,
        rect,
        isFullscreen: spec.isFullscreen,
        showTimer: spec.showTimer,
        runtimeOverlaySnapshot,
        paused: false,
        elapsedBeforePauseMs: 0,
        runningStartMs,
    };
    sessionState.shortcutEditActive = false;
    return sessionState.controlsState;
}

export function clearControlsState(sessionState) {
    sessionState.controlsState = null;
    sessionState.runtimeOverlaySnapshot = null;
    applyRuntimeOverlayVisibility(sessionState, null);
    sessionState.shortcutEditActive = false;
}

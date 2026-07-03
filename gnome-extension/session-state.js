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
});

const RUNTIME_OVERLAY_VISIBILITY_KEYS = Object.freeze([
    "mic",
    "speaker",
]);

function normalizeBoolean(value, fallback) {
    return typeof value === "boolean" ? value : fallback;
}

function normalizeNonNegativeInteger(value, fallback) {
    if (!Number.isFinite(value))
        return fallback;
    return Math.max(0, Math.trunc(value));
}

function normalizeControlsVisibilityPolicy(value, fallback = "visible") {
    return value === "area-outside-capture" || value === "hidden" || value === "visible"
        ? value
        : fallback;
}

function normalizeRuntimeOverlayVisibilityKey(key) {
    return RUNTIME_OVERLAY_VISIBILITY_KEYS.includes(key) ? key : null;
}

export function createRuntimeOverlayVisibility(snapshot = null) {
    return {
        mic: snapshot?.mic_visible ?? false,
        speaker: snapshot?.speaker_visible ?? false,
    };
}

function createRuntimeOverlayState() {
    return {
        chrome: null,
        audioIndicatorsActor: null,
        micIndicatorActor: null,
        speakerIndicatorActor: null,
        visibility: createRuntimeOverlayVisibility(),
        selfOwnedActors: new WeakSet(),
        selfOwnedActorOwners: new WeakMap(),
    };
}

function ensureRuntimeOverlayState(sessionState) {
    sessionState.runtimeOverlayState ??= createRuntimeOverlayState();
    sessionState.runtimeOverlayState.visibility ??= createRuntimeOverlayVisibility();
    sessionState.runtimeOverlayState.selfOwnedActors ??= new WeakSet();
    sessionState.runtimeOverlayState.selfOwnedActorOwners ??= new WeakMap();
    return sessionState.runtimeOverlayState;
}

function applyRuntimeOverlayVisibility(sessionState, snapshot) {
    ensureRuntimeOverlayState(sessionState).visibility = createRuntimeOverlayVisibility(snapshot);
}

export function getRuntimeOverlayVisibility(sessionState, key) {
    const visibilityKey = normalizeRuntimeOverlayVisibilityKey(key);
    if (!visibilityKey || !sessionState)
        return false;

    return ensureRuntimeOverlayState(sessionState).visibility[visibilityKey];
}

export function setRuntimeOverlayVisibility(sessionState, key, visible) {
    const visibilityKey = normalizeRuntimeOverlayVisibilityKey(key);
    if (!visibilityKey || !sessionState?.runtimeOverlaySnapshot)
        return false;

    ensureRuntimeOverlayState(sessionState).visibility[visibilityKey] = Boolean(visible);
    return true;
}

export function toggleRuntimeOverlayVisibility(sessionState, key) {
    const visibilityKey = normalizeRuntimeOverlayVisibilityKey(key);
    if (!visibilityKey)
        return false;

    const nextVisible = !getRuntimeOverlayVisibility(sessionState, visibilityKey);
    return setRuntimeOverlayVisibility(sessionState, visibilityKey, nextVisible)
        ? nextVisible
        : false;
}

export function registerSelfOwnedActor(sessionState, actor, owner = "extension-ui") {
    if (!sessionState || !actor)
        return actor;

    const overlayState = ensureRuntimeOverlayState(sessionState);
    overlayState.selfOwnedActors.add(actor);
    overlayState.selfOwnedActorOwners.set(actor, owner);
    actor._apexshotSelfOwned = true;
    actor._apexshotSelfOwnedOwner = owner;
    return actor;
}

export function isSelfOwnedActor(sessionState, actor) {
    const ownedActors = sessionState?.runtimeOverlayState?.selfOwnedActors ?? null;
    let current = actor ?? null;
    while (current) {
        if (current._apexshotSelfOwned || ownedActors?.has(current))
            return true;
        current = typeof current.get_parent === "function"
            ? current.get_parent()
            : null;
    }
    return false;
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
        visibilityPolicy: normalizeControlsVisibilityPolicy(
            spec.visibilityPolicy,
            spec.isFullscreen ? "visible" : "area-outside-capture"
        ),
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
    sessionState.runtimeOverlayState = createRuntimeOverlayState();
    sessionState.shortcutEditActive = false;
}

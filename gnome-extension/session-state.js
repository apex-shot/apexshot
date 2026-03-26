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

export function createSessionState() {
    return {
        currentRect: null,
        controlsState: null,
        runtimeOverlaySnapshot: null,
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

export function setControlsState(sessionState, spec, runningStartMs) {
    const rect = cloneRect(spec.rect);
    const runtimeOverlaySnapshot = spec.runtimeOverlaySnapshot || null;

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
    sessionState.runtimeOverlaySnapshot = runtimeOverlaySnapshot;
    sessionState.shortcutEditActive = false;
    return sessionState.controlsState;
}

export function clearControlsState(sessionState) {
    sessionState.controlsState = null;
    sessionState.runtimeOverlaySnapshot = null;
    sessionState.shortcutEditActive = false;
}

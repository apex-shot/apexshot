// SPDX-License-Identifier: AGPL-3.0-or-later

export function createRenderableRuntimeOverlayVisibility(visibility) {
    return {
        mic: false,
        speaker: false,
        webcam: Boolean(visibility?.webcam),
        clicks: Boolean(visibility?.clicks),
        keystrokes: Boolean(visibility?.keystrokes),
    };
}

export function hasRenderableRuntimeOverlays(visibility) {
    const renderable = createRenderableRuntimeOverlayVisibility(visibility);
    return renderable.webcam || renderable.clicks || renderable.keystrokes;
}

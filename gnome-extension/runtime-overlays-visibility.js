// SPDX-License-Identifier: AGPL-3.0-or-later

export function createRenderableRuntimeOverlayVisibility(visibility) {
    return {
        mic: false,
        speaker: false,
        webcam: Boolean(visibility?.webcam),
    };
}

export function hasRenderableRuntimeOverlays(visibility) {
    const renderable = createRenderableRuntimeOverlayVisibility(visibility);
    return renderable.webcam;
}

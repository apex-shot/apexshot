// SPDX-License-Identifier: AGPL-3.0-or-later

export function createRenderableRuntimeOverlayVisibility(visibility) {
    return {
        mic: false,
        speaker: false,
    };
}

export function hasRenderableRuntimeOverlays(visibility) {
    createRenderableRuntimeOverlayVisibility(visibility);
    return false;
}

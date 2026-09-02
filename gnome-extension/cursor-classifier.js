// SPDX-License-Identifier: AGPL-3.0-or-later

/// Classifies the common cursor shapes from the sprite geometry Mutter exposes.
///
/// Mutter does not expose the semantic Meta.Cursor value for the current
/// client-provided cursor. Cursor themes do, however, preserve the hotspot
/// layout: arrows point near the top-left, hands near the upper middle, and
/// text cursors at their centre.
export function classifyCursorGeometry(width, height, hotX, hotY) {
    if (![width, height, hotX, hotY].every(Number.isFinite) ||
        width <= 0 || height <= 0 || hotX < 0 || hotY < 0 ||
        hotX >= width || hotY >= height)
        return 'default';

    const x = hotX / width;
    const y = hotY / height;

    if (x >= 0.36 && y >= 0.36)
        return 'text';
    if (x >= 0.22 && y < 0.36)
        return 'hand';
    return 'default';
}

/// Reads only safe, introspected CursorTracker and CoglTexture APIs.
export function classifyCursorTracker(tracker) {
    if (!tracker || typeof tracker.get_hot !== 'function' ||
        typeof tracker.get_sprite !== 'function')
        return 'default';

    try {
        const sprite = tracker.get_sprite();
        if (!sprite || typeof sprite.get_width !== 'function' ||
            typeof sprite.get_height !== 'function')
            return 'default';

        const [hotX, hotY] = tracker.get_hot();
        return classifyCursorGeometry(
            sprite.get_width(), sprite.get_height(), hotX, hotY);
    } catch (error) {
        return 'default';
    }
}
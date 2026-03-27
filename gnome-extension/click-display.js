// SPDX-License-Identifier: AGPL-3.0-or-later

export function createClickOverlayModel() {
    return {
        lifetimeMs: 180,
        buttons: {
            left: false,
            middle: false,
            right: false,
        },
        lastClick: null,
    };
}

export function recordPointerSample(model, sample = {}) {
    if (!model)
        return [];

    const clicks = [];
    const captureClicks = sample.capture !== false;
    for (const button of ["left", "middle", "right"]) {
        const wasPressed = Boolean(model.buttons[button]);
        const isPressed = Boolean(sample[button]);
        if (captureClicks && !wasPressed && isPressed) {
            const click = {
                button,
                x: Math.round(sample.x ?? 0),
                y: Math.round(sample.y ?? 0),
                timestampMs: sample.timestampMs ?? 0,
            };
            clicks.push(click);
            model.lastClick = click;
        }
        model.buttons[button] = isPressed;
    }

    return clicks;
}

export function getActiveClickIndicator(model, nowMs) {
    if (!model?.lastClick)
        return null;

    if ((nowMs - model.lastClick.timestampMs) >= model.lifetimeMs)
        return null;

    return model.lastClick;
}

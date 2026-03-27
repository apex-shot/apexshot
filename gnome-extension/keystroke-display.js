// SPDX-License-Identifier: AGPL-3.0-or-later

const MODIFIER_ONLY_KEYS = new Set([
    "Alt_L",
    "Alt_R",
    "Caps_Lock",
    "Control_L",
    "Control_R",
    "ISO_Level3_Shift",
    "Meta_L",
    "Meta_R",
    "Shift_L",
    "Shift_R",
    "Super_L",
    "Super_R",
]);

const SPECIAL_KEY_LABELS = Object.freeze({
    BackSpace: "Backspace",
    Delete: "Delete",
    Down: "Down",
    Escape: "Esc",
    ISO_Left_Tab: "Tab",
    KP_Enter: "Enter",
    Left: "Left",
    Return: "Enter",
    Right: "Right",
    space: "Space",
    Tab: "Tab",
    Up: "Up",
});

export function createKeystrokeOverlayModel() {
    return {
        entries: [],
        lifetimeMs: 2000,
        maxEntries: 5,
    };
}

export function pruneKeystrokeEntries(model, nowMs) {
    if (!model)
        return [];

    model.entries = model.entries.filter(entry => (nowMs - entry.timestampMs) < model.lifetimeMs);
    return model.entries;
}

function titleCaseWords(text) {
    return text
        .split(/[_\s]+/)
        .filter(Boolean)
        .map(part => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
        .join(" ");
}

function normalizeKeyLabel({keySymbol, unicodeChar}) {
    if (!keySymbol)
        return unicodeChar && unicodeChar.trim()
            ? unicodeChar.toUpperCase()
            : null;

    if (MODIFIER_ONLY_KEYS.has(keySymbol))
        return null;

    if (Object.hasOwn(SPECIAL_KEY_LABELS, keySymbol))
        return SPECIAL_KEY_LABELS[keySymbol];

    if (unicodeChar && unicodeChar.trim())
        return unicodeChar.toUpperCase();

    if (keySymbol.length === 1)
        return keySymbol.toUpperCase();

    if (/^F\d{1,2}$/.test(keySymbol))
        return keySymbol;

    if (keySymbol.startsWith("KP_"))
        return titleCaseWords(keySymbol.slice(3));

    return titleCaseWords(keySymbol);
}

export function formatKeystrokeEvent({
    keySymbol,
    unicodeChar = "",
    ctrl = false,
    alt = false,
    shift = false,
    meta = false,
    filterMode = 0,
} = {}) {
    const keyLabel = normalizeKeyLabel({keySymbol, unicodeChar});
    if (!keyLabel)
        return null;

    const hasCommandModifier = ctrl || alt || meta;
    if (filterMode === 1 && !hasCommandModifier)
        return null;

    const parts = [];
    if (ctrl)
        parts.push("Ctrl");
    if (alt)
        parts.push("Alt");
    if (shift)
        parts.push("Shift");
    if (meta)
        parts.push("Super");
    parts.push(keyLabel);
    return parts.join(" + ");
}

export function recordKeystrokeEvent(model, event) {
    if (!model)
        return false;

    const timestampMs = event?.timestampMs ?? 0;
    pruneKeystrokeEntries(model, timestampMs);

    const text = formatKeystrokeEvent(event);
    if (!text)
        return false;

    model.entries.push({text, timestampMs});
    if (model.entries.length > model.maxEntries)
        model.entries = model.entries.slice(model.entries.length - model.maxEntries);
    return true;
}

export function recordKeystrokeText(model, text, timestampMs) {
    if (!model || !text)
        return false;

    pruneKeystrokeEntries(model, timestampMs);
    model.entries.push({text, timestampMs});
    if (model.entries.length > model.maxEntries)
        model.entries = model.entries.slice(model.entries.length - model.maxEntries);
    return true;
}

export function getKeystrokeOverlayText(model, nowMs) {
    const entries = pruneKeystrokeEntries(model, nowMs);
    if (!entries.length)
        return "";
    return entries.map(entry => entry.text).join("   ");
}

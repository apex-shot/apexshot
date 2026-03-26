// SPDX-License-Identifier: AGPL-3.0-or-later

import St from "gi://St";
import Clutter from "gi://Clutter";
import * as Main from "resource:///org/gnome/shell/ui/main.js";

const OVERLAY_MARGIN = 10;
const AUDIO_INDICATORS_MARGIN = 14;
const CLICK_INDICATOR_MARGIN = 18;
const KEYSTROKE_INDICATOR_MARGIN = 18;
const WEBCAM_SIZE_MAP = Object.freeze({
    1: {width: 120, height: 160},
    2: {width: 200, height: 260},
    3: {width: 280, height: 370},
    4: {width: 360, height: 480},
});
const CLICK_COLOR_MAP = Object.freeze({
    0: "rgb(180, 180, 180)",
    1: "rgb(122, 100, 255)",
    2: "rgb(255, 60, 60)",
    3: "rgb(60, 120, 255)",
    4: "rgb(60, 200, 80)",
    5: "rgb(255, 210, 50)",
    6: "rgb(255, 150, 30)",
    7: "rgb(180, 60, 220)",
    8: "rgb(255, 255, 255)",
});

function createVisibilityState() {
    return {
        mic: false,
        speaker: false,
        webcam: false,
        clicks: false,
        keystrokes: false,
    };
}

function ensureRuntimeOverlayState(sessionState) {
    if (!sessionState.runtimeOverlayState) {
        sessionState.runtimeOverlayState = {
            chrome: null,
            webcamActor: null,
            webcamIconActor: null,
            webcamLabelActor: null,
            clicksActor: null,
            clickPulseStackActor: null,
            clickPulseActor: null,
            clickHaloActor: null,
            clickLabelActor: null,
            keystrokesActor: null,
            keystrokeLabelActor: null,
            audioIndicatorsActor: null,
            micIndicatorActor: null,
            speakerIndicatorActor: null,
            visibility: createVisibilityState(),
        };
    }

    const state = sessionState.runtimeOverlayState;
    state.visibility ??= createVisibilityState();
    return state;
}

function hasVisibleRuntimeOverlays(overlayState) {
    const visibility = overlayState.visibility ?? createVisibilityState();
    return visibility.mic ||
        visibility.speaker ||
        visibility.webcam ||
        visibility.clicks ||
        visibility.keystrokes;
}

function createChip(iconName, labelText, style) {
    const chip = new St.BoxLayout({
        reactive: false,
        style: [
            "padding: 8px 12px;",
            "spacing: 8px;",
            "border-radius: 16px;",
            style,
        ].join(" "),
    });

    chip.add_child(new St.Icon({
        icon_name: iconName,
        style: "icon-size: 15px;",
        y_align: Clutter.ActorAlign.CENTER,
    }));
    chip.add_child(new St.Label({
        text: labelText,
        y_align: Clutter.ActorAlign.CENTER,
        style: "font-size: 13px; font-weight: 700;",
    }));
    return chip;
}

function createWebcamActor(overlayState) {
    const actor = new St.BoxLayout({
        vertical: true,
        reactive: false,
        style: [
            "background-color: rgba(8, 10, 18, 0.88);",
            "border: 1px solid rgba(255, 255, 255, 0.18);",
            "padding: 16px;",
            "spacing: 10px;",
            "box-shadow: 0 14px 34px rgba(0, 0, 0, 0.4);",
        ].join(" "),
    });

    overlayState.webcamIconActor = new St.Icon({
        icon_name: "camera-web-symbolic",
        style: "icon-size: 42px; color: rgba(255, 255, 255, 0.9);",
        x_align: Clutter.ActorAlign.CENTER,
    });
    actor.add_child(overlayState.webcamIconActor);

    overlayState.webcamLabelActor = new St.Label({
        text: "Webcam",
        x_align: Clutter.ActorAlign.CENTER,
        style: "font-size: 13px; font-weight: 700; color: rgba(255, 255, 255, 0.78);",
    });
    actor.add_child(overlayState.webcamLabelActor);

    return actor;
}

function createClicksActor(overlayState) {
    const actor = new St.BoxLayout({
        reactive: false,
        style: [
            "background-color: rgba(12, 14, 22, 0.82);",
            "border: 1px solid rgba(255, 255, 255, 0.14);",
            "border-radius: 20px;",
            "padding: 10px 12px;",
            "spacing: 12px;",
        ].join(" "),
    });

    overlayState.clickPulseStackActor = new St.Widget({
        reactive: false,
        layout_manager: new Clutter.BinLayout(),
    });
    actor.add_child(overlayState.clickPulseStackActor);

    overlayState.clickHaloActor = new St.Widget({reactive: false});
    overlayState.clickPulseStackActor.add_child(overlayState.clickHaloActor);

    overlayState.clickPulseActor = new St.Widget({reactive: false});
    overlayState.clickPulseStackActor.add_child(overlayState.clickPulseActor);

    overlayState.clickLabelActor = new St.Label({
        text: "Clicks",
        y_align: Clutter.ActorAlign.CENTER,
        style: "font-size: 13px; font-weight: 700; color: rgba(255, 255, 255, 0.86);",
    });
    actor.add_child(overlayState.clickLabelActor);

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

function createAudioIndicatorsActor(overlayState) {
    const actor = new St.BoxLayout({
        reactive: false,
        style: "spacing: 8px;",
    });

    overlayState.micIndicatorActor = createChip(
        "audio-input-microphone-symbolic",
        "Mic",
        "background-color: rgba(26, 91, 60, 0.84); color: white;"
    );
    actor.add_child(overlayState.micIndicatorActor);

    overlayState.speakerIndicatorActor = createChip(
        "audio-volume-high-symbolic",
        "Speaker",
        "background-color: rgba(31, 59, 120, 0.84); color: white;"
    );
    actor.add_child(overlayState.speakerIndicatorActor);

    return actor;
}

function webcamPreviewSize(snapshot, rect) {
    const base = WEBCAM_SIZE_MAP[snapshot.webcam_size] ?? WEBCAM_SIZE_MAP[2];
    let width = base.width;
    let height = base.height;

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
        return 18;
    default:
        return 12;
    }
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

function updateAudioIndicators(overlayState, snapshot, rect) {
    const showAudio = overlayState.visibility.mic || overlayState.visibility.speaker;
    setActorVisible(overlayState.audioIndicatorsActor, showAudio);
    setActorVisible(overlayState.micIndicatorActor, overlayState.visibility.mic);
    setActorVisible(overlayState.speakerIndicatorActor, overlayState.visibility.speaker);

    if (!showAudio)
        return;

    overlayState.audioIndicatorsActor.set_position(
        rect.x + AUDIO_INDICATORS_MARGIN,
        rect.y + AUDIO_INDICATORS_MARGIN
    );
}

function updateWebcamActor(overlayState, snapshot, rect) {
    const visible = overlayState.visibility.webcam;
    setActorVisible(overlayState.webcamActor, visible);
    if (!visible)
        return;

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
    overlayState.webcamActor.set_style([
        "background-color: rgba(8, 10, 18, 0.88);",
        "border: 1px solid rgba(255, 255, 255, 0.18);",
        `border-radius: ${radius}px;`,
        "padding: 16px;",
        "spacing: 10px;",
        "box-shadow: 0 14px 34px rgba(0, 0, 0, 0.4);",
    ].join(" "));
    overlayState.webcamIconActor.set_style([
        "icon-size: 42px;",
        "color: rgba(255, 255, 255, 0.9);",
    ].join(" "));
    const webcamLabel = snapshot.webcam_device >= 0
        ? `Camera ${snapshot.webcam_device}`
        : "Webcam";
    overlayState.webcamLabelActor.text = snapshot.webcam_flip
        ? `${webcamLabel} mirrored`
        : webcamLabel;
}

function updateClicksActor(overlayState, snapshot, rect) {
    const visible = overlayState.visibility.clicks;
    setActorVisible(overlayState.clicksActor, visible);
    if (!visible)
        return;

    const clickSize = 12 + Math.round(snapshot.click_size * 28);
    const haloSize = snapshot.click_animate ? clickSize + 20 : clickSize + 8;
    const clickColor = CLICK_COLOR_MAP[snapshot.click_color] ?? CLICK_COLOR_MAP[0];
    const borderWidth = snapshot.click_style === 1 ? 0 : 3;
    const fillColor = snapshot.click_style === 1 ? clickColor : "transparent";

    overlayState.clickHaloActor.set_size(haloSize, haloSize);
    overlayState.clickHaloActor.set_style([
        `width: ${haloSize}px;`,
        `height: ${haloSize}px;`,
        `border-radius: ${Math.floor(haloSize / 2)}px;`,
        `border: 2px solid ${clickColor};`,
        `background-color: ${snapshot.click_animate ? "rgba(255, 255, 255, 0.04)" : "transparent"};`,
        `opacity: ${snapshot.click_animate ? 72 : 40};`,
    ].join(" "));

    overlayState.clickPulseActor.set_size(clickSize, clickSize);
    overlayState.clickPulseActor.set_style([
        `width: ${clickSize}px;`,
        `height: ${clickSize}px;`,
        `border-radius: ${Math.floor(clickSize / 2)}px;`,
        borderWidth > 0 ? `border: ${borderWidth}px solid ${clickColor};` : "border: none;",
        `background-color: ${fillColor};`,
    ].join(" "));
    overlayState.clickPulseStackActor.set_size(haloSize, haloSize);

    const chipWidth = 110;
    const chipHeight = 44;
    overlayState.clicksActor.set_size(chipWidth, chipHeight);
    overlayState.clicksActor.set_position(
        rect.x + CLICK_INDICATOR_MARGIN,
        rect.y + rect.height - chipHeight - CLICK_INDICATOR_MARGIN
    );
    overlayState.clicksActor.set_style([
        "background-color: rgba(12, 14, 22, 0.82);",
        "border: 1px solid rgba(255, 255, 255, 0.14);",
        "border-radius: 20px;",
        "padding: 10px 12px;",
        "spacing: 12px;",
    ].join(" "));
    overlayState.clickLabelActor.text = snapshot.click_animate ? "Clicks live" : "Clicks";
}

function updateKeystrokesActor(overlayState, snapshot, rect) {
    const visible = overlayState.visibility.keystrokes;
    setActorVisible(overlayState.keystrokesActor, visible);
    if (!visible)
        return;

    const darkAppearance = snapshot.key_appearance === 0;
    const backgroundColor = darkAppearance
        ? (snapshot.key_blur_bg ? "rgba(20, 20, 24, 0.48)" : "rgba(20, 20, 24, 0.9)")
        : (snapshot.key_blur_bg ? "rgba(245, 245, 250, 0.48)" : "rgba(245, 245, 250, 0.9)");
    const textColor = darkAppearance ? "rgb(255, 255, 255)" : "rgb(20, 20, 24)";
    const scale = 0.85 + (snapshot.key_size * 0.75);
    const width = Math.round(124 * scale);
    const height = Math.round(46 * scale);
    const [x, y] = keyPositionCoords(snapshot, rect, width, height);

    overlayState.keystrokesActor.set_size(width, height);
    overlayState.keystrokesActor.set_position(x, y);
    overlayState.keystrokesActor.set_style([
        `background-color: ${backgroundColor};`,
        `color: ${textColor};`,
        `border: 1px solid ${darkAppearance ? "rgba(255, 255, 255, 0.16)" : "rgba(20, 20, 24, 0.12)"};`,
        `border-radius: ${Math.round(12 * scale)}px;`,
        `padding: ${Math.round(10 * scale)}px ${Math.round(16 * scale)}px;`,
        `spacing: ${Math.round(10 * scale)}px;`,
        snapshot.key_blur_bg ? "box-shadow: 0 12px 24px rgba(0, 0, 0, 0.18);" : "",
    ].join(" "));
    overlayState.keystrokeLabelActor.text = snapshot.key_filter === 1 ? "Ctrl + K" : "Shift + A";
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
    if (!hasVisibleRuntimeOverlays(overlayState)) {
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

    overlayState.webcamActor = createWebcamActor(overlayState);
    overlayState.clicksActor = createClicksActor(overlayState);
    overlayState.keystrokesActor = createKeystrokesActor(overlayState);
    overlayState.audioIndicatorsActor = createAudioIndicatorsActor(overlayState);

    overlayState.chrome.add_child(overlayState.webcamActor);
    overlayState.chrome.add_child(overlayState.clicksActor);
    overlayState.chrome.add_child(overlayState.keystrokesActor);
    overlayState.chrome.add_child(overlayState.audioIndicatorsActor);

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
    updateAudioIndicators(overlayState, snapshot, rect);
    updateWebcamActor(overlayState, snapshot, rect);
    updateClicksActor(overlayState, snapshot, rect);
    updateKeystrokesActor(overlayState, snapshot, rect);
}

export function destroyRuntimeOverlays(sessionState) {
    if (!sessionState)
        return;

    const overlayState = ensureRuntimeOverlayState(sessionState);
    if (overlayState.chrome) {
        if (overlayState.chrome.get_parent())
            Main.layoutManager.removeChrome(overlayState.chrome);
        overlayState.chrome.destroy();
    }

    overlayState.chrome = null;
    overlayState.webcamActor = null;
    overlayState.webcamIconActor = null;
    overlayState.webcamLabelActor = null;
    overlayState.clicksActor = null;
    overlayState.clickPulseStackActor = null;
    overlayState.clickPulseActor = null;
    overlayState.clickHaloActor = null;
    overlayState.clickLabelActor = null;
    overlayState.keystrokesActor = null;
    overlayState.keystrokeLabelActor = null;
    overlayState.audioIndicatorsActor = null;
    overlayState.micIndicatorActor = null;
    overlayState.speakerIndicatorActor = null;
    overlayState.visibility = createVisibilityState();
}

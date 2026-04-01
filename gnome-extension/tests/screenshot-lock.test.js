// SPDX-License-Identifier: AGPL-3.0-or-later

import {ScreenshotLockController} from "../screenshot-lock.js";

function assert(condition, message) {
    if (!condition)
        throw new Error(message);
}

function assertEqual(actual, expected, message) {
    if (actual !== expected)
        throw new Error(`${message}: expected ${expected}, got ${actual}`);
}

function runTest(name, fn) {
    try {
        fn();
        print(`ok - ${name}`);
    } catch (error) {
        printerr(`not ok - ${name}`);
        printerr(error?.stack ?? String(error));
        throw error;
    }
}

function createHarness() {
    const calls = [];
    let nextTimeoutId = 1;
    const timeouts = new Map();
    const stageSignals = new Map();
    const actor = {
        visible: false,
        connections: new Map(),
        show() { this.visible = true; calls.push("show"); },
        hide() { this.visible = false; calls.push("hide"); },
        set_size(width, height) { calls.push(`size:${width}x${height}`); },
        connect(signal, handler) {
            this.connections.set(signal, handler);
            return signal;
        },
        disconnect(signal) {
            this.connections.delete(signal);
        },
        destroy() { calls.push("destroy"); },
    };

    const controller = new ScreenshotLockController({
        createActor: () => actor,
        getStageSize: () => ({width: 1920, height: 1080}),
        addActor: () => calls.push("addActor"),
        removeActor: () => calls.push("removeActor"),
        pushModal: () => {
            calls.push("pushModal");
            return {id: "grab"};
        },
        popModal: () => calls.push("popModal"),
        connectStage: (signal, handler) => {
            stageSignals.set(signal, handler);
            calls.push(`connectStage:${signal}`);
            return signal;
        },
        disconnectStage: signal => {
            stageSignals.delete(signal);
            calls.push(`disconnectStage:${signal}`);
        },
        scheduleTimeout: (_ms, callback) => {
            const id = nextTimeoutId++;
            timeouts.set(id, callback);
            calls.push("scheduleTimeout");
            return id;
        },
        cancelTimeout: id => {
            timeouts.delete(id);
            calls.push(`cancelTimeout:${id}`);
        },
        sendCancelRequest: sessionId => calls.push(`cancel:${sessionId}`),
        escapeKeySymbol: "Escape",
        eventStop: "stop",
        eventPropagate: "propagate",
        getKeySymbol: event => event.keySymbol,
        shouldBlockEventTarget: target => target?.kind === "shell",
        getEventTarget: event => event.target,
        log: message => calls.push(`log:${message}`),
    });

    return {controller, calls, actor, timeouts, stageSignals};
}

runTest("begin creates modal lock and duplicate begin is idempotent", () => {
    const {controller, calls} = createHarness();

    controller.begin("capture-1");
    controller.begin("capture-1");

    assertEqual(calls.filter(call => call === "pushModal").length, 0, "deprecated lock should not push modal");
    assertEqual(calls.filter(call => call === "addActor").length, 0, "deprecated lock should not add actors");
    assert(!controller.isActive(), "deprecated lock should stay inactive");
});

runTest("escape sends cancel request and releases lock", () => {
    const {controller, calls} = createHarness();

    controller.begin("capture-esc");
    const result = controller.handleKeyPress({keySymbol: "Escape"});

    assertEqual(result, "propagate", "deprecated lock should not consume keys");
    assert(!calls.includes("cancel:capture-esc"), "deprecated lock should not send cancel");
    assert(!calls.includes("popModal"), "deprecated lock should not release modal state");
    assert(!controller.isActive(), "lock should be inactive after escape");
});

runTest("timeout cleanup releases stale lock", () => {
    const {controller, calls, timeouts} = createHarness();

    controller.begin("capture-timeout");
    assertEqual(timeouts.size, 0, "deprecated lock should not schedule timeouts");
    assert(!controller.isActive(), "lock should be inactive after timeout");
});

runTest("new session tears down old state and reuses controller cleanly", () => {
    const {controller, calls} = createHarness();

    controller.begin("capture-1");
    controller.begin("capture-2");

    assertEqual(calls.filter(call => call === "pushModal").length, 0, "deprecated lock should not push modal on reuse");
    assert(!calls.includes("popModal"), "deprecated lock should not pop modal on reuse");
    assert(!controller.isActive(), "controller should remain inactive");
});

runTest("deprecated lock never installs captured-event filtering", () => {
    const {controller, stageSignals} = createHarness();

    controller.begin("capture-filter");
    assertEqual(stageSignals.size, 0, "deprecated lock should not connect stage handlers");
});

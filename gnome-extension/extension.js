// SPDX-License-Identifier: AGPL-3.0-or-later
// ApexShot Preview Helper

import Meta from "gi://Meta";

export default class ApexShotPreview {
    constructor() {
        this._windowCreatedId = null;
        this._trackedWindows = new Map();
        this._pollInterval = null;
    }

    enable() {
        // Connect to window-created signal
        this._windowCreatedId = global.display.connect(
            "window-created",
            (display, window) => this._onWindowCreated(window)
        );

        // Check existing windows
        this._checkExistingWindows();
        
        // VERY aggressive polling - every 50ms
        this._pollInterval = setInterval(() => {
            this._pollWindows();
        }, 50);
        
        // Watch focus changes
        global.display.connect('notify::focus-window', () => {
            this._onFocusChange();
        });
    }

    disable() {
        if (this._windowCreatedId) {
            global.display.disconnect(this._windowCreatedId);
            this._windowCreatedId = null;
        }
        if (this._pollInterval) {
            clearInterval(this._pollInterval);
        }
        this._trackedWindows.clear();
    }

    _checkExistingWindows() {
        const windows = global.get_window_actors();
        windows.forEach((actor) => {
            const window = actor.get_meta_window();
            if (window) {
                this._processWindow(window);
            }
        });
    }

    _onWindowCreated(window) {
        if (!window) {
            return;
        }

        // Wait for title to be set
        const sourceId = window.connect("notify::title", () => {
            this._processWindow(window);
            window.disconnect(sourceId);
        });

        this._processWindow(window);
    }

    _onFocusChange() {
        // Whenever focus changes, re-apply to all tracked windows
        for (let [id, data] of this._trackedWindows) {
            if (data.window) {
                this._applyAbove(data.window);
            }
        }
    }

    _pollWindows() {
        // Check all windows for our preview
        const windows = global.get_window_actors();
        windows.forEach((actor) => {
            const window = actor.get_meta_window();
            if (window) {
                this._processWindow(window);
            }
        });
    }

    _processWindow(window) {
        if (!window) return;

        let title = window.get_title() ?? "";
        let wmClass = window.get_wm_class() ?? "";
        let windowId = window.get_id();

        // Check if this is our preview window
        const isPreview = title.includes('Screenshot') || 
                          wmClass.toLowerCase().includes('apexshot');

        if (isPreview) {
            // Only attach handlers once
            if (!this._trackedWindows.has(windowId)) {
                this._trackedWindows.set(windowId, { window: window, processed: true });
                
                // Watch for minimize state changes
                window.connect('notify::minimized', () => {
                    if (!window.minimized) {
                        this._applyAbove(window);
                    }
                });
                
                // Watch for hidden state changes
                window.connect('notify::hidden', () => {
                    if (!window.is_hidden()) {
                        this._applyAbove(window);
                    }
                });
                
                // Watch for window layer changes
                window.connect('notify::layer', () => {
                    this._applyAbove(window);
                });
            }
            
            this._applyAbove(window);
        }
    }

    _applyAbove(window) {
        try {
            window.make_above();
            window.stick();
            window.unminimize();
        } catch(e) {
            // Ignore errors
        }
    }
}
// SPDX-License-Identifier: AGPL-3.0-or-later
// ApexShot Preview Helper - keeps preview windows on top

import {Extension} from "resource:///org/gnome/shell/extensions/extension.js";

let _checkInterval = null;
let _trackedWindows = new Map();

export default class ApexShotPreview extends Extension {
    _applyStacking(metaWindow) {
        let windowId = metaWindow.get_id();
        let title = metaWindow.get_title() || '(no title)';
        
        if (_trackedWindows.has(windowId)) {
            metaWindow.make_above();
            return;
        }
        
        try {
            metaWindow.make_above();
            metaWindow.skip_taskbar = true;
            metaWindow.skip_pager = true;
            _trackedWindows.set(windowId, metaWindow);
            console.log('ApexShot: Tracking ' + title);
        } catch (e) {
            console.log('ApexShot: Error: ' + e);
        }
    }

    _scanForPreviewWindows() {
        try {
            let display = global.display;
            let workspaces = display.get_workspaces();
            
            for (let wsi = 0; wsi < workspaces.length; wsi++) {
                let windows = workspaces[wsi].list_windows();
                
                for (let i = 0; i < windows.length; i++) {
                    let w = windows[i];
                    let title = w.get_title() || '';
                    
                    if (title.includes('Screenshot') || title.includes('apexshot')) {
                        this._applyStacking(w);
                    }
                }
            }
        } catch (e) {
            console.log('ApexShot: Scan: ' + e);
        }
    }

    _onFocusOut(display, event) {
        try {
            let focusedWindow = event.get_focused_window();
            
            if (focusedWindow && _trackedWindows.has(focusedWindow.get_id())) {
                focusedWindow.raise();
                focusedWindow.make_above();
            }
            
            this._scanForPreviewWindows();
        } catch (e) {
            console.log('ApexShot: Focus: ' + e);
        }
    }

    enable() {
        console.log('ApexShot: Extension enabled');
        
        _checkInterval = setInterval(() => this._scanForPreviewWindows(), 500);
        global.display.connect('focus-out', (d, e) => this._onFocusOut(d, e));
        this._scanForPreviewWindows();
    }

    disable() {
        console.log('ApexShot: Extension disabled');
        
        if (_checkInterval) {
            clearInterval(_checkInterval);
            _checkInterval = null;
        }
        _trackedWindows.clear();
    }
}
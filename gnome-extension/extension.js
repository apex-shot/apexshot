'use strict';

const { GLib, Meta } = imports.gi;

let _previewXids = new Set();

function _applyStackingConstraints(metaWindow) {
    metaWindow.make_above();
    metaWindow.skip_taskbar = true;
    metaWindow.skip_pager = true;
    
    const xid = metaWindow.get_id();
    _previewXids.add(xid);
}

function _removeStackingConstraints(metaWindow) {
    metaWindow.make_above();
    const xid = metaWindow.get_id();
    _previewXids.delete(xid);
}

let _signalSubscription = null;

function _connectToApexshot() {
    try {
        const connection = GLib.DBusConnection.get(GLib.BusType.SESSION, null);
        
        if (!connection) {
            return;
        }
        
        _signalSubscription = connection.signal_subscribe(
            null,
            'org.apexshot.Preview',
            'PreviewOpened',
            '/org/apexshot/Preview',
            null,
            0,
            (conn, sender, object_path, iface, signal, params) => {
                const xid = params.get_child_value(0).get_uint32();
                _onPreviewOpened(xid);
            }
        );
        
        connection.signal_subscribe(
            null,
            'org.apexshot.Preview',
            'PreviewClosed',
            '/org/apexshot/Preview',
            null,
            0,
            (conn, sender, object_path, iface, signal, params) => {
                const xid = params.get_child_value(0).get_uint32();
                _onPreviewClosed(xid);
            }
        );
        
        log('ApexShot Preview Helper: Connected to D-Bus');
    } catch (e) {
        log(`ApexShot Preview Helper: Could not connect to D-Bus: ${e.message}`);
    }
}

function _onPreviewOpened(xid) {
    const display = global.display;
    
    if (xid > 0) {
        const windowActor = display.get_window_actors().find(
            w => w.get_meta_window().get_id() === xid
        );
        if (windowActor) {
            const metaWindow = windowActor.get_meta_window();
            _applyStackingConstraints(metaWindow);
            log(`ApexShot Preview Helper: Applied stacking to window ${xid}`);
            return;
        }
    }
    
    const windows = display.get_workspace(0).list_windows();
    for (const w of windows) {
        const title = w.get_title();
        if (title && (title === 'Screenshot' || title.includes('apexshot') || title.includes('ApexShot'))) {
            _applyStackingConstraints(w);
            log(`ApexShot Preview Helper: Applied stacking to window by title: ${title}`);
            break;
        }
    }
}

function _onPreviewClosed(xid) {
    const display = global.display;
    const windows = display.get_window_actors();
    
    for (const wa of windows) {
        const mw = wa.get_meta_window();
        if (xid > 0 && mw.get_id() === xid) {
            _removeStackingConstraints(mw);
            log(`ApexShot Preview Helper: Removed stacking from window ${xid}`);
            break;
        }
    }
}

let _focusOutId = null;

function _setupFocusOutHandler() {
    _focusOutId = global.display.connect('focus-out', (display, event) => {
        const focusedWindow = event.get_focused_window();
        if (focusedWindow && _previewXids.has(focusedWindow.get_id())) {
            focusedWindow.raise();
            focusedWindow.make_above();
        }
    });
}

function init() {
    _connectToApexshot();
    _setupFocusOutHandler();
}

function enable() {
    log('ApexShot Preview Helper: Extension enabled');
}

function disable() {
    const display = global.display;
    for (const wa of display.get_window_actors()) {
        const mw = wa.get_meta_window();
        if (_previewXids.has(mw.get_id())) {
            mw.delete_property('above');
            mw.skip_taskbar = false;
            mw.skip_pager = false;
        }
    }
    _previewXids.clear();
    log('ApexShot Preview Helper: Extension disabled');
}
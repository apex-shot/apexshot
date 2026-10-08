// SPDX-License-Identifier: AGPL-3.0-or-later

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import Gio from 'gi://Gio';

import {PreviewStacker} from './preview-stacking.js';
import {ShellOverlayService} from './shell-overlay.js';
import {WindowListService} from './window-list.js';

export default class ApexShotExtension extends Extension {
    enable() {
        const address = Gio.dbus_address_get_for_bus_sync(Gio.BusType.SESSION, null);
        this._dbusConnection = Gio.DBusConnection.new_for_address_sync(address,
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT |
            Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
            null,
            null);
        this._dbusConnection.set_exit_on_close(false);

        this._previewStacker = new PreviewStacker();
        this._previewStacker.enable(this._dbusConnection);

        this._shellOverlay = new ShellOverlayService();
        this._shellOverlay.enable(this._dbusConnection);

        this._windowList = new WindowListService();
        this._windowList.enable(this._dbusConnection);
    }

    disable() {
        this._windowList?.disable();
        this._windowList = null;

        this._shellOverlay?.disable();
        this._shellOverlay = null;

        this._previewStacker?.disable();
        this._previewStacker = null;

        if (this._dbusConnection) {
            this._dbusConnection.close_sync(null);
            this._dbusConnection = null;
        }
    }
}

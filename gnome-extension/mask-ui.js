// SPDX-License-Identifier: AGPL-3.0-or-later

import St from "gi://St";
import {
    clearCurrentRect,
    setCurrentRect,
} from "./session-state.js";

export class MaskUi {
    constructor(sessionState) {
        this._sessionState = sessionState;
        this._maskGroup = null;
    }

    showMask(x, y, width, height) {
        setCurrentRect(this._sessionState, {x, y, width, height});

        this.ensureMaskGroup();
        this._maskGroup.remove_all_children();

        const stage = global.stage;
        const stageWidth = stage.width;
        const stageHeight = stage.height;
        this._maskGroup.set_position(0, 0);
        this._maskGroup.set_size(stageWidth, stageHeight);

        const left = Math.max(0, x);
        const top = Math.max(0, y);
        const right = Math.min(stageWidth, x + width);
        const bottom = Math.min(stageHeight, y + height);

        const rects = [
            [0, 0, stageWidth, top],
            [0, top, left, Math.max(0, bottom - top)],
            [right, top, Math.max(0, stageWidth - right), Math.max(0, bottom - top)],
            [0, bottom, stageWidth, Math.max(0, stageHeight - bottom)],
        ];

        for (const [rectX, rectY, rectWidth, rectHeight] of rects) {
            if (rectWidth <= 0 || rectHeight <= 0)
                continue;

            const region = new St.Widget({
                reactive: false,
                x: rectX,
                y: rectY,
                width: rectWidth,
                height: rectHeight,
                style: "background-color: rgba(0, 0, 0, 0.55);",
            });
            this._maskGroup.add_child(region);
        }

        if (!this._maskGroup.get_parent())
            global.window_group.add_child(this._maskGroup);

        this._maskGroup.show();
    }

    hideMask() {
        clearCurrentRect(this._sessionState);

        if (!this._maskGroup)
            return;

        this._maskGroup.remove_all_children();
        this._maskGroup.hide();
        if (this._maskGroup.get_parent())
            this._maskGroup.get_parent().remove_child(this._maskGroup);
    }

    ensureMaskGroup() {
        if (this._maskGroup)
            return;

        this._maskGroup = new St.Widget({
            reactive: false,
            x: 0,
            y: 0,
            width: global.stage.width,
            height: global.stage.height,
        });
    }

    refresh() {
        const rect = this._sessionState.currentRect;
        if (!rect)
            return;

        this.showMask(rect.x, rect.y, rect.width, rect.height);
    }
}

use super::footer;

use crate::recording::editor::model::VideoEditState;
use gtk4::{prelude::*, ApplicationWindow, Box as GtkBox, Orientation};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) const INSPECTOR_WIDTH: i32 = 240;

pub(super) fn build_inspector(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-inspector");
    root.set_width_request(INSPECTOR_WIDTH);
    root.set_hexpand(false);
    root.set_vexpand(true);

    let actions = footer::build_inspector_actions(window, state, exporting);
    root.append(&actions);
    root
}

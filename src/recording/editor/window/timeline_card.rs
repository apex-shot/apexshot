use crate::capture::editor::window::icon_names;
use crate::recording::editor::model::{
    format_zoom_scale, playhead_for_replay, snap_range_to_target, snap_to_target,
    usable_media_timestamp_seconds, VideoEditState, ZoomMode, DEFAULT_CURSOR_HIDE_DURATION_SECONDS,
    DEFAULT_ZOOM_DURATION_SECONDS,
};
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    prelude::*, Adjustment, Align, Box as GtkBox, Button, DrawingArea, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, GestureClick, GestureDrag, Image, Label,
    MediaFile, Orientation, Overlay, Scale, Scrollbar, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};

mod shell {
    use super::*;
    include!("timeline_card_parts/shell.rs");
}
mod format {
    use super::*;
    include!("timeline_card_parts/format.rs");
}
mod interaction {
    use super::*;
    include!("timeline_card_parts/interaction.rs");
}
mod geometry {
    use super::*;
    include!("timeline_card_parts/geometry.rs");
}
mod painting {
    use super::*;
    include!("timeline_card_parts/painting.rs");
}

pub(super) use format::*;
pub(super) use geometry::*;
pub(super) use interaction::*;
pub(super) use painting::*;
pub(super) use shell::build_timeline_card;

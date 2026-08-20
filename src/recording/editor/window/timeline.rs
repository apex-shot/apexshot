use super::footer;
use crate::recording::editor::model::{ProjectMedia, ProjectMediaKind, VideoEditState};
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    gdk::prelude::GdkCairoContextExt, prelude::*, Adjustment, Align, Box as GtkBox, Button,
    DrawingArea, EventControllerMotion, EventControllerScroll, EventControllerScrollFlags,
    GestureClick, GestureDrag, Image, Label, MediaFile, Orientation, Overlay, Scrollbar,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const RAIL_HEADER_WIDTH: i32 = 120;
const TRACK_GAP: f64 = 8.0;
const ZERO_INSET: f64 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RailKind {
    Video,
    Audio,
    Zoom,
}

mod build {
    use super::*;
    include!("timeline_parts/build.rs");
}
mod tracks {
    use super::*;
    include!("timeline_parts/tracks.rs");
}
mod playback {
    use super::*;
    include!("timeline_parts/playback.rs");
}
mod media {
    use super::*;
    include!("timeline_parts/media.rs");
}
mod zoom {
    use super::*;
    include!("timeline_parts/zoom.rs");
}
mod trim {
    use super::*;
    include!("timeline_parts/trim.rs");
}

pub(super) use build::build_timeline;
pub(super) use media::*;
pub(super) use playback::*;
pub(super) use tracks::*;
pub(super) use trim::*;
pub(super) use zoom::*;

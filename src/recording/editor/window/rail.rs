use super::media_library;
use crate::recording::editor::model::VideoEditState;
use gtk4::Box as GtkBox;
use std::sync::{Arc, Mutex};

pub(super) struct ToolChrome {
    pub panel: GtkBox,
}

pub(super) fn build_tool_chrome(
    state: Option<Arc<Mutex<VideoEditState>>>,
    estimate_label: Option<gtk4::Label>,
    empty_open: Option<media_library::EmptyOpenHooks>,
) -> ToolChrome {
    ToolChrome {
        panel: media_library::build_media_library(state, estimate_label, empty_open),
    }
}

use super::media_library;
use crate::recording::editor::model::VideoEditState;
use gtk4::Box as GtkBox;
use std::sync::{Arc, Mutex};

pub(super) struct ToolChrome {
    pub panel: GtkBox,
}

pub(super) fn build_tool_chrome(
    state: Option<Arc<Mutex<VideoEditState>>>,
    empty_open: Option<media_library::EmptyOpenHooks>,
) -> ToolChrome {
    ToolChrome {
        panel: media_library::build_media_library(state, empty_open),
    }
}

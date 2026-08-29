use super::sidecar::PointerSidecar;
use super::zoom_suggest;
use std::path::{Path, PathBuf};

mod types {
    use super::*;
    include!("model_parts/types.rs");
}
pub use types::*;
use types::{seed_project_media, MIN_DIMENSION};

mod state {
    use super::*;
    include!("model_parts/state_impl.rs");
}

mod timeline {
    use super::*;
    include!("model_parts/timeline_impl.rs");
}

mod output {
    use super::*;
    include!("model_parts/output_core_impl.rs");
    include!("model_parts/output_impl.rs");
}

mod zoom {
    use super::*;
    include!("model_parts/zoom_impl.rs");
}

mod cursor_hide {
    use super::*;
    include!("model_parts/cursor_hide_impl.rs");
}

include!("model_parts/helpers.rs");

#[cfg(test)]
mod tests {
    include!("model_parts/tests.rs");
}

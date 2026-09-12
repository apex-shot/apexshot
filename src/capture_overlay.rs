//! Bridge to the native C++ Qt5 capture overlay binary (`apexshot-capture`).
//!
//! The binary is compiled from `capture-overlay/` by `build.rs` and placed
//! next to the Rust binary. This module finds and runs it as a subprocess,
//! parses the JSON output, and returns selection/capture data.
//!
//! Protocol:
//!   overlay mode: exit 0 + `{"x":N,"y":N,"width":N,"height":N}`
//!   capture mode: exit 0 + `{"path":"/tmp/...png",...}`
//!   exit 1 → cancelled by user
//!   exit 2 → error
//!
//! Warm helper:
//!   When possible, jobs are sent to a long-lived `apexshot-capture --worker`
//!   process over `$XDG_RUNTIME_DIR/apexshot-capture-worker.sock` so Qt cold
//!   start is paid once. Disable with `APEXSHOT_DISABLE_WARM_CAPTURE=1`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{net::UnixStream, process::ExitStatusExt},
};

use crate::{
    backend::{CaptureData, DisplayBackend, PixelFormat, WaylandBackend},
    gnome_integration::{emit_tracked_window_closed, emit_tracked_window_opened},
    overlay::{OverlaySelection, SelectionArea, SelectionError, SelectionResult},
};
use gtk4::gdk;
use serde::{Deserialize, Serialize};

include!("capture_overlay/types.rs");
include!("capture_overlay/environment.rs");
include!("capture_overlay/session.rs");
include!("capture_overlay/binary_locator.rs");
include!("capture_overlay/process.rs");
include!("capture_overlay/worker.rs");
include!("capture_overlay/protocol.rs");
include!("capture_overlay/errors.rs");
include!("capture_overlay/image_io.rs");
include!("capture_overlay/arguments.rs");
include!("capture_overlay/wlroots.rs");
include!("capture_overlay/portal.rs");
include!("capture_overlay/recording_controls.rs");
include!("capture_overlay/api.rs");
include!("capture_overlay/tests.rs");

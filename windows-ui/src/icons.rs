//! One registration of the bundled icon resource for every window in the
//! process.
//!
//! GTK registers a GResource once per process; the onboarding and the settings
//! window both need the same names, and whichever opens first installs them.

use std::sync::Once;

pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        relm4_icons::initialize_icons(
            crate::capture::editor::window::icon_names::GRESOURCE_BYTES,
            crate::capture::editor::window::icon_names::RESOURCE_PREFIX,
        );
    });
}

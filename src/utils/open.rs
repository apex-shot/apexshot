//! Open URLs and files without host shell-outs.
//!
//! Uses GIO `AppInfo::launch_default_for_uri`, which routes through the
//! OpenURI portal inside Flatpak and works from any thread (unlike
//! GTK `UriLauncher`, which requires a main-context owner).

use std::path::Path;

use gtk4::gio;
use gtk4::prelude::FileExt;

/// Open a URI (http(s), file://, …) with the desktop default handler.
pub fn open_uri(uri: &str) -> Result<(), String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return Err("empty URI".into());
    }
    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
        .map_err(|e| format!("Could not open URI: {e}"))
}

/// Open a local path with the desktop default handler.
pub fn open_path(path: &Path) -> Result<(), String> {
    let file = gio::File::for_path(path);
    let uri = file.uri();
    open_uri(&uri)
}

/// Open a URL in the default browser (alias of [`open_uri`]).
pub fn open_url(url: &str) -> Result<(), String> {
    open_uri(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_uri_is_rejected() {
        assert!(open_uri("").is_err());
        assert!(open_uri("   ").is_err());
    }
}

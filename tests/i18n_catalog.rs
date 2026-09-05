//! Run the catalog coverage check as part of the Rust test suite.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn shipped_catalogs_cover_registered_ui_messages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg(root.join("scripts/check-i18n-catalogs.py"))
        .current_dir(&root)
        .output()
        .expect("run the catalog coverage checker");

    assert!(
        output.status.success(),
        "catalog coverage checker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

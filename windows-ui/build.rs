fn main() {
    println!("cargo:rerun-if-changed=../data/icons");

    // The settings window reuses the editor's icon bundle so its dropdown
    // chevrons and custom page icons match the Linux build.
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        Some("../data/icons"),
        [
            "chevron-down-regular",
            "chevron-right-regular",
            "pointer-primary-click",
            // The clip menu's rows use the same fluent glyphs as the Linux
            // editor, so a right-click reads the same on both platforms.
            "copy-regular",
            "copy-arrow-right-regular",
            "cut-regular",
            "clipboard-paste-regular",
            "eye-regular",
            "eye-off-regular",
        ],
    );
}

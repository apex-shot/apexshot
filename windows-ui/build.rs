fn main() {
    println!("cargo:rerun-if-changed=../data/icons");

    // The settings window reuses the editor's icon bundle so its dropdown
    // chevrons and custom page icons match the Linux build. The image editor
    // adds the annotation tools on top of the same bundle.
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        Some("../data/icons"),
        [
            "chevron-down-regular",
            "search-regular",
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
            // Image editor: the one fluent glyph the tool rail keeps; every
            // other tool icon comes from the bundled symbolic set.
            "number-circle-1-regular",
            // The Appearance panel's Image tab trails the Linux chooser's
            // folder affordance.
            "folder-open-regular",
            // The Motion notch borrows the watermark tab's photo glyph.
            "image-regular",
        ],
    );
}

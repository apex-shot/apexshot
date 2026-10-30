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
            // Image editor: tools, history and the empty-state artwork.
            "select",
            "crop",
            "pen-regular",
            "highlight-regular",
            "arrow-up-right-regular",
            "draw-line",
            "rectangle-landscape-regular",
            "circle-regular",
            "text-t-regular",
            "number-circle-1-regular",
            "blur",
            "small-rectangle-in-focus",
            "arrow-undo-regular",
            "arrow-redo-regular",
            "delete-regular",
            "image-regular",
            "cloud-arrow-up-regular",
        ],
    );
}

fn main() {
    println!("cargo:rerun-if-changed=../data/icons");

    // The settings window reuses the editor's icon bundle so its dropdown
    // chevrons and custom page icons match the Linux build.
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        Some("../data/icons"),
        ["chevron-down-regular", "chevron-right-regular"],
    );
}

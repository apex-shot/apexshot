//! The slice of `crate::capture` the settings pages need.

pub mod editor {
    //! The annotation model the Windows image editor shares with the Linux
    //! editor. Platform-neutral pieces (types, palette, pen weights, the
    //! Cairo renderer and hit testing) are the same source files the Linux
    //! binary compiles, attached here through `#[path]`; the session state,
    //! export and text detection are Windows ports of their Linux siblings.

    /// Annotation document types (actions, tools, colours, transforms).
    #[path = "../../../../src/capture/editor/types.rs"]
    #[rustfmt::skip]
    pub mod types;

    /// The shared drawing palette and value clamps.
    #[path = "../../../../src/capture/editor/color.rs"]
    #[rustfmt::skip]
    pub mod color;

    /// Stroke thickness presets for the pen and highlighter.
    #[path = "../../../../src/capture/editor/pen_weight.rs"]
    #[rustfmt::skip]
    pub mod pen_weight;

    /// Numbering marker styles and sizes.
    #[path = "../../../../src/capture/editor/numbering_style.rs"]
    #[rustfmt::skip]
    pub mod numbering_style;

    /// Annotation hit testing, bounds and geometry editing.
    #[path = "../../../../src/capture/editor/selection.rs"]
    #[rustfmt::skip]
    pub mod selection;

    /// The Cairo annotation renderer every annotation goes through.
    #[path = "../../../../src/capture/editor/render/mod.rs"]
    #[rustfmt::skip]
    pub mod render;

    /// Source upscaling options. The Windows editor ships the enum so the
    /// shared types compile; the neural path itself is not offered yet.
    pub mod upscale;

    /// Composition layout types. Only the rectangle the renderer's frame
    /// border helper needs is ported; Windows uses its own Appearance layout.
    pub mod composition;

    /// The still image camera pose, shared with the Linux editor.
    #[path = "../../../../src/capture/editor/image_transform.rs"]
    #[rustfmt::skip]
    pub mod image_transform;

    pub(crate) mod glass_render;
    pub(crate) mod shadow_render;
    pub(crate) mod style_render;
    /// Perspective projection for the Windows still-image editor.
    pub(crate) mod transform_render;

    /// Windows Runtime OCR used by the highlighter's Detect text mode.
    pub mod ocr;

    /// Text region detection feeding the text-aware highlighter.
    pub mod text_detect;

    /// The annotation session: document, tools, undo history and effects.
    pub mod state;

    /// Save/copy paths for the flattened document.
    pub mod io_ops;

    pub mod ui_support {
        #[cfg(target_os = "windows")]
        const THEME_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

        /// Resolve the configured theme. "System" follows the Windows app
        /// theme, then the GTK preference.
        pub fn prefers_dark_glass_theme() -> bool {
            match crate::config::load_config().ui_theme.as_str() {
                "dark" => true,
                "light" => false,
                _ => system_prefers_dark()
                    .or_else(|| {
                        gtk4::Settings::default()
                            .map(|settings| settings.is_gtk_application_prefer_dark_theme())
                    })
                    .unwrap_or(true),
            }
        }

        /// Read the app theme straight from the registry. A `reg query`
        /// subprocess here cost about 30 ms per call, and the windows call it
        /// two or three times while they are still building.
        #[cfg(target_os = "windows")]
        fn system_prefers_dark() -> Option<bool> {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Foundation::ERROR_SUCCESS;
            use windows_sys::Win32::System::Registry::{
                RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD,
            };

            let key: Vec<u16> = std::ffi::OsStr::new(THEME_KEY)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let value: Vec<u16> = std::ffi::OsStr::new("AppsUseLightTheme")
                .encode_wide()
                .chain(Some(0))
                .collect();
            let mut light = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let result = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    RRF_RT_REG_DWORD,
                    std::ptr::null_mut(),
                    &mut light as *mut u32 as *mut std::ffi::c_void,
                    &mut size,
                )
            };
            (result == ERROR_SUCCESS).then_some(light == 0)
        }

        #[cfg(not(target_os = "windows"))]
        fn system_prefers_dark() -> Option<bool> {
            None
        }

        pub fn apply_theme_to_gtk() {
            if let Some(settings) = gtk4::Settings::default() {
                settings.set_gtk_application_prefer_dark_theme(prefers_dark_glass_theme());
            }
        }
    }

    pub mod window {
        #[allow(dead_code)]
        pub mod icon_names {
            pub use shipped::*;
            include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
        }
    }
}

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
    /// border helper needs is ported; the full background compositor stays
    /// behind the Appearance backend, which is not wired yet.
    pub mod composition;

    /// Windows Runtime OCR used by the highlighter's Detect text mode.
    pub mod ocr;

    /// Text region detection feeding the text-aware highlighter.
    pub mod text_detect;

    /// The annotation session: document, tools, undo history and effects.
    pub mod state;

    /// Save/copy paths for the flattened document.
    pub mod io_ops;

    pub mod ui_support {
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

        #[cfg(target_os = "windows")]
        fn system_prefers_dark() -> Option<bool> {
            let output = std::process::Command::new("reg")
                .args([
                    "query",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                    "/v",
                    "AppsUseLightTheme",
                ])
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&output.stdout);
            apps_use_light_theme(&text).map(|light| !light)
        }

        /// `reg query` prints DWORDs as `0x0` / `0x1`, not decimal.
        fn apps_use_light_theme(output: &str) -> Option<bool> {
            let value = output
                .lines()
                .find(|line| line.contains("AppsUseLightTheme"))?
                .split_whitespace()
                .last()?;
            let number = match value
                .strip_prefix("0x")
                .or_else(|| value.strip_prefix("0X"))
            {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => value.parse::<u32>().ok()?,
            };
            Some(number != 0)
        }

        #[cfg(not(target_os = "windows"))]
        fn system_prefers_dark() -> Option<bool> {
            None
        }

        #[cfg(test)]
        mod tests {
            use super::apps_use_light_theme;

            #[test]
            fn registry_dword_is_parsed_as_hex() {
                assert_eq!(
                    apps_use_light_theme("    AppsUseLightTheme    REG_DWORD    0x0\n"),
                    Some(false)
                );
                assert_eq!(
                    apps_use_light_theme("    AppsUseLightTheme    REG_DWORD    0x1\n"),
                    Some(true)
                );
                assert_eq!(apps_use_light_theme(""), None);
            }
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

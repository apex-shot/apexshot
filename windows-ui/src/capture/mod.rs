//! The slice of `crate::capture` the settings pages need.

pub mod editor {
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

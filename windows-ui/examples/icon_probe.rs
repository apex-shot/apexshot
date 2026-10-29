//! Check how the bundled navigation glyphs resolve in GTK's icon theme.
//!
//! A symbolic icon is recoloured by forcing a fill colour onto every shape, so
//! stroke-drawn glyphs must resolve as plain icons instead:
//!
//! `cargo run --example icon_probe`

use apexshot_settings::capture::editor::window::icon_names::custom;
use gtk4::{gdk, IconLookupFlags, IconTheme, TextDirection};

fn main() {
    gtk4::init().unwrap();
    apexshot_settings::icons::install();

    let names = [
        custom::SETTINGS_2,
        custom::SETTINGS_2_LIGHT,
        custom::KEYBOARD,
        custom::PANELS_TOP_LEFT,
        custom::VIDEO,
        custom::CAMERA,
        custom::PEN_TOOL,
        custom::SLIDERS_HORIZONTAL,
        custom::CLOUD,
        custom::STUDIO,
        custom::ABOUT,
        custom::SEARCH,
    ];

    let display = gdk::Display::default().expect("a display");
    let theme = IconTheme::for_display(&display);

    let mut failures = 0;
    for name in names {
        let icon = theme.lookup_icon(
            name,
            &[],
            16,
            1,
            TextDirection::Ltr,
            IconLookupFlags::empty(),
        );
        let present = theme.has_icon(name);
        println!("{name}: symbolic={} present={present}", icon.is_symbolic());
        if icon.is_symbolic() || !present {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!("{failures} glyph(s) resolve as symbolic or missing");
        std::process::exit(1);
    }
}

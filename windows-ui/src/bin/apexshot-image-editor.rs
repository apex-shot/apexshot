#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    apexshot_settings::platform::prepare_process();
    apexshot_settings::i18n::init_from_config();

    let initial =
        apexshot_settings::image_editor::initial_image_from_args(std::env::args_os().skip(1));
    if let Err(error) = apexshot_settings::image_editor::show_image_editor_window(initial) {
        eprintln!("Failed to open image editor: {error}");
        std::process::exit(1);
    }
}

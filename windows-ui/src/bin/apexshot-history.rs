#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    apexshot_settings::platform::prepare_process();
    apexshot_settings::i18n::init_from_config();
    if let Err(error) = apexshot_settings::history::show_history_window() {
        eprintln!("Failed to open history window: {error}");
        std::process::exit(1);
    }
}

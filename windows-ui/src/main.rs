fn main() {
    apexshot_settings::platform::prepare_process();
    apexshot_settings::i18n::init_from_config();
    if let Err(error) = apexshot_settings::settings::show_settings_window() {
        eprintln!("Failed to open settings window: {error}");
        std::process::exit(1);
    }
}

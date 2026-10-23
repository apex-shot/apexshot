fn main() {
    apexshot_settings::platform::prepare_process();
    apexshot_settings::i18n::init_from_config();

    let initial =
        apexshot_settings::video_editor::initial_video_from_args(std::env::args_os().skip(1));
    if let Err(error) = apexshot_settings::video_editor::show_video_editor_window(initial) {
        eprintln!("Failed to open video editor: {error}");
        std::process::exit(1);
    }
}

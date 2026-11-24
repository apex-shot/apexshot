#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    apexshot_settings::platform::prepare_process();

    // The release smoke test runs the installed editor with `--self-check
    // <report>`: GTK must start and the bundled ffmpeg and ffprobe must run.
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|flag| flag == "--self-check") {
        let report = args.next().map(std::path::PathBuf::from);
        let result = gtk4::init()
            .map_err(|error| format!("GTK failed to start: {error}"))
            .and_then(|()| apexshot_settings::recording::editor::ffmpeg::ensure_tools_available());
        let text = match &result {
            Ok(()) => "ok".to_string(),
            Err(error) => format!("failed: {error}"),
        };
        if let Some(report) = report {
            let _ = std::fs::write(report, &text);
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    apexshot_settings::i18n::init_from_config();

    let initial =
        apexshot_settings::video_editor::initial_video_from_args(std::env::args_os().skip(1));
    if let Err(error) = apexshot_settings::video_editor::show_video_editor_window(initial) {
        eprintln!("Failed to open video editor: {error}");
        std::process::exit(1);
    }
}

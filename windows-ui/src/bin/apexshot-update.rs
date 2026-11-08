#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The window a resolved update opens, and the daily check that resolves one.
//! `--check` looks for a newer stable release and presents it; the explicit
//! flags present a release another caller has already resolved.

use apexshot_settings::update::Command;

fn main() {
    apexshot_settings::platform::prepare_process();
    apexshot_settings::i18n::init_from_config();

    let command = match apexshot_settings::update::command_from_args(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    let result = match command {
        Command::Help => {
            println!("{}", apexshot_settings::update::usage());
            return;
        }
        Command::Check => apexshot_settings::update::check_and_show(),
        Command::Show(release) => apexshot_settings::update::show(release),
    };

    if let Err(error) = result {
        eprintln!("Failed to open the update window: {error}");
        std::process::exit(1);
    }
}

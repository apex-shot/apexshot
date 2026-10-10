use std::process::Command;

pub(crate) fn run_update(args: &[String]) -> Result<(), String> {
    match &args[2..] {
        [] => {}
        [flag] if matches!(flag.as_str(), "--help" | "-h") => {
            println!("Usage: apexshot update");
            println!("Update ApexShot using the official distro-aware updater.");
            println!("Run as your normal user; the updater asks for sudo when needed.");
            return Ok(());
        }
        _ => return Err("Usage: apexshot update (no options required)".into()),
    }

    if apexshot::app_identity::portal_only() {
        return Err(format!(
            "This installation is managed by Flatpak. Run `flatpak update {}` from a host terminal.",
            apexshot::app_identity::app_id()
        ));
    }
    if cfg!(feature = "nix") {
        return Err("This installation is managed by Nix. Update the ApexShot flake input and rebuild your configuration, or upgrade its Nix profile entry.".into());
    }

    println!("Checking for ApexShot updates...");
    let Some(update) = apexshot::update::check_for_update_now()
        .map_err(|error| format!("Could not check the latest release: {error}"))?
    else {
        println!(
            "ApexShot {} is already up to date.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    };
    println!(
        "Updating ApexShot {} to {}...",
        env!("CARGO_PKG_VERSION"),
        update.version
    );
    let status = updater_command()
        .status()
        .map_err(|error| format!("Could not start the updater: {error}"))?;
    if !status.success() {
        return Err(format!("The ApexShot updater failed ({status})."));
    }
    Ok(())
}

fn updater_command() -> Command {
    let script = format!(
        "set -eu\n\
         workdir=$(mktemp -d)\n\
         trap 'rm -rf \"$workdir\"' EXIT\n\
         curl --fail --silent --show-error --location --connect-timeout 10 --max-time 60 \
         --output \"$workdir/update.sh\" {}\n\
         sh \"$workdir/update.sh\"",
        apexshot::update::UPDATE_SCRIPT_URL
    );
    let mut command = Command::new("bash");
    command.args(["-c", &script]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(feature = "nix", not(feature = "flatpak")))]
    fn nix_update_returns_package_guidance_without_accessing_the_network() {
        let args = ["apexshot", "update"].map(String::from);
        assert!(run_update(&args).unwrap_err().contains("managed by Nix"));
    }

    #[test]
    fn update_help_returns_without_starting_the_updater() {
        for flag in ["--help", "-h"] {
            let args = ["apexshot", "update", flag].map(String::from);
            assert!(run_update(&args).is_ok());
        }
    }

    #[test]
    fn update_rejects_unknown_options_before_any_network_request() {
        let args = ["apexshot", "update", "--force"].map(String::from);
        assert!(run_update(&args)
            .unwrap_err()
            .contains("Usage: apexshot update"));
    }

    #[test]
    fn updater_downloads_the_complete_official_script_before_running_it() {
        let command = updater_command();
        assert_eq!(command.get_program(), "bash");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args[0], "-c");
        let script = args[1].to_str().unwrap();
        assert!(script.starts_with("set -eu\n"));
        assert!(script.contains("mktemp -d"));
        assert!(script.contains("https://apexshot.org/update"));
        assert!(script.contains("--fail --silent --show-error --location"));
        assert!(script.contains("--output \"$workdir/update.sh\""));
        assert!(script.ends_with("sh \"$workdir/update.sh\""));
        assert!(script.contains("trap 'rm -rf \"$workdir\"' EXIT"));
    }
}

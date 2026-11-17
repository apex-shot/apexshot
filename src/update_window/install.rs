//! Package installation for the update window.
//!
//! A native install goes through the system's package service when it is
//! available, and through `pkexec` for a single package file when it is not.
//! Both keep the user out of the terminal: PackageKit raises the desktop's own
//! authentication dialog, and `pkexec` raises the polkit one. Arch always uses
//! `pkexec` because the package needs pacman's `--overwrite` flag.
//!
//! The worker thread owns all of the blocking work and reports everything the
//! window shows through [`InstallEvent`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};

use super::download;
use crate::gnome_extension::{self, ExtensionRefresh};
use crate::i18n::{t, tfmt};

const PACKAGEKIT_SERVICE: &str = "org.freedesktop.PackageKit";
const PACKAGEKIT_PATH: &str = "/org/freedesktop/PackageKit";
const PACKAGEKIT_INTERFACE: &str = "org.freedesktop.PackageKit";
const PACKAGEKIT_TRANSACTION_INTERFACE: &str = "org.freedesktop.PackageKit.Transaction";
const FLATPAK_PORTAL: &str = "org.freedesktop.portal.Flatpak";
const FLATPAK_PORTAL_PATH: &str = "/org/freedesktop/portal/Flatpak";
const FLATPAK_MONITOR_INTERFACE: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";

/// The package formats ApexShot publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    Deb,
    Rpm,
    Arch,
}

/// How this machine can install a downloaded package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// The system's PackageKit service, with its own polkit dialog.
    PackageKit(PackageFormat),
    /// `pkexec` runs the distro's package manager for one file.
    Pkexec(PackageFormat),
    /// The Flatpak portal updates the sandboxed app from its repository.
    Flatpak,
    /// ApexShot is not installed from a package; the terminal updater is the
    /// only route.
    Terminal,
    /// The installation is managed elsewhere (Nix) and the message explains it.
    Unsupported(String),
}

/// What the installing page shows while the worker runs.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallEvent {
    /// A short line for the status row, already translated.
    Status(String),
    /// Overall progress; `None` means indeterminate.
    Progress(Option<f64>),
    /// The package and the post-install steps are done.
    Finished(Installed),
    /// The user dismissed the authentication dialog or stopped the install.
    Cancelled,
    /// The install failed. The message is safe to show.
    Failed(String),
}

/// What a finished install changed besides the package itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The GNOME extension result, when an archive was refreshed.
    pub extension: Option<ExtensionRefresh>,
}

/// Everything one install needs.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub method: InstallMethod,
    /// The downloaded package; absent for Flatpak.
    pub package: Option<PathBuf>,
    /// The verified GNOME extension archive, when the download fetched one.
    pub extension_zip: Option<PathBuf>,
}

/// Detect how this machine can install a downloaded package.
pub fn detect() -> InstallMethod {
    if crate::app_identity::portal_only() {
        return InstallMethod::Flatpak;
    }
    if cfg!(feature = "nix") {
        return InstallMethod::Unsupported(t(
            "This installation is managed by Nix. Update the ApexShot flake input and rebuild your configuration, or upgrade its Nix profile entry.",
        ));
    }
    // The release only publishes x86_64 packages; other machines keep the
    // terminal updater, which can pick whatever their distribution ships.
    if !supports_native_install(std::env::consts::ARCH) {
        return InstallMethod::Terminal;
    }
    let Some(format) = installed_package_format() else {
        return InstallMethod::Terminal;
    };
    if format == PackageFormat::Arch {
        return InstallMethod::Pkexec(format);
    }
    if packagekit_available() {
        InstallMethod::PackageKit(format)
    } else {
        InstallMethod::Pkexec(format)
    }
}

/// The release publishes `amd64`/`x86_64` packages only.
fn supports_native_install(arch: &str) -> bool {
    arch == "x86_64"
}

/// Which package owns this binary, when one does.
fn installed_package_format() -> Option<PackageFormat> {
    let dpkg = Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", "apexshot"])
        .output()
        .ok();
    if dpkg.is_some_and(|output| {
        output.status.success()
            && dpkg_status_says_installed(&String::from_utf8_lossy(&output.stdout))
    }) {
        return Some(PackageFormat::Deb);
    }

    if Command::new("rpm")
        .args(["-q", "apexshot"])
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Some(PackageFormat::Rpm);
    }

    if Command::new("pacman")
        .args(["-Q", "apexshot"])
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Some(PackageFormat::Arch);
    }

    None
}

/// `dpkg-query -W -f=${Status}` says `install ok installed` for an installed
/// package, and anything else (`deinstall ok config-files`, …) means the
/// package is not there.
fn dpkg_status_says_installed(status: &str) -> bool {
    status.split_whitespace().collect::<Vec<_>>().join(" ") == "install ok installed"
}

/// PackageKit is usable when it owns its bus name or the bus could start it.
fn packagekit_available() -> bool {
    let Ok(connection) = zbus::blocking::Connection::system() else {
        return false;
    };
    let owned = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(PACKAGEKIT_SERVICE,),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false);
    let activatable: Vec<String> = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListActivatableNames",
            &(),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<Vec<String>>().ok())
        .unwrap_or_default();
    packagekit_available_from(owned, &activatable)
}

fn packagekit_available_from(owned: bool, activatable: &[String]) -> bool {
    owned || activatable.iter().any(|name| name == PACKAGEKIT_SERVICE)
}

/// Run the install on a worker thread and stream its events.
pub fn start(request: InstallRequest) -> Receiver<InstallEvent> {
    let (sender, receiver) = async_channel::bounded(32);
    std::thread::spawn(move || {
        let native = !matches!(&request.method, InstallMethod::Flatpak);
        let result = match (&request.method, request.package.as_deref()) {
            (InstallMethod::PackageKit(_), Some(package)) => run_packagekit(package, &sender),
            (InstallMethod::Pkexec(format), Some(package)) => run_pkexec(*format, package, &sender),
            (InstallMethod::Flatpak, _) => run_flatpak(&sender),
            (InstallMethod::Unsupported(message), _) => Err(InstallEvent::Failed(message.clone())),
            (InstallMethod::Terminal, _) => Err(InstallEvent::Failed(t(
                "This system needs the terminal updater.",
            ))),
            _ => Err(InstallEvent::Failed(t("The update package is missing."))),
        };

        match result {
            Ok(()) => {
                let installed = post_install(native, request.extension_zip.as_deref(), &sender);
                let _ = sender.send_blocking(InstallEvent::Finished(installed));
            }
            Err(event) => {
                let _ = sender.send_blocking(event);
            }
        }
    });
    receiver
}

/// PackageKit's transaction: create it, ask for interactivity (which is what
/// raises the desktop's polkit dialog), subscribe to its signals, then hand it
/// the package path. The status and percentage arrive as property changes and
/// the outcome as `Finished`.
fn run_packagekit(package: &Path, sender: &Sender<InstallEvent>) -> Result<(), InstallEvent> {
    let connection = zbus::blocking::Connection::system().map_err(|error| {
        InstallEvent::Failed(tfmt(
            "Could not reach the package service: {error}",
            &[("error", &error.to_string())],
        ))
    })?;
    let manager = zbus::blocking::Proxy::new(
        &connection,
        PACKAGEKIT_SERVICE,
        PACKAGEKIT_PATH,
        PACKAGEKIT_INTERFACE,
    )
    .map_err(|error| InstallEvent::Failed(error.to_string()))?;
    let transaction_path: zbus::zvariant::OwnedObjectPath = manager
        .call("CreateTransaction", &())
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;
    let transaction = zbus::blocking::Proxy::new(
        &connection,
        PACKAGEKIT_SERVICE,
        transaction_path.as_str(),
        PACKAGEKIT_TRANSACTION_INTERFACE,
    )
    .map_err(|error| InstallEvent::Failed(error.to_string()))?;

    let hints = vec![
        "interactive=true".to_string(),
        format!("locale={}", locale_hint()),
    ];
    let _ = transaction.call::<_, _, ()>("SetHints", &(hints,));

    // The transaction's own signals carry the outcome; the progress and
    // status properties arrive as `PropertiesChanged` on the Properties
    // interface, which a second subscription forwards from its own thread.
    let progress_stop = Arc::new(AtomicBool::new(false));
    {
        let connection = connection.clone();
        let transaction_path = transaction_path.clone();
        let stop = progress_stop.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            monitor_transaction_properties(connection, transaction_path, stop, sender)
        });
    }

    let signal_proxy = transaction.clone();
    let signals = signal_proxy
        .receive_all_signals()
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;

    let paths = vec![package.to_string_lossy().to_string()];
    transaction
        .call::<_, _, ()>("InstallFiles", &(0u64, paths))
        .map_err(|error| {
            InstallEvent::Failed(tfmt(
                "The package service refused the update: {error}",
                &[("error", &error.to_string())],
            ))
        })?;

    let _ = sender.send_blocking(InstallEvent::Status(t("Installing...")));
    let mut last_error: Option<String> = None;
    let mut not_authorized = false;

    for message in signals {
        let member = message
            .header()
            .member()
            .map(|member| member.as_str().to_owned())
            .unwrap_or_default();
        match member.as_str() {
            "ErrorCode" => {
                if let Ok((code, details)) = message.body().deserialize::<(u32, String)>() {
                    // 48 is NOT_AUTHORIZED: the authentication dialog was
                    // dismissed or refused, which the page treats as a cancel.
                    not_authorized |= code == 48;
                    last_error = Some(details);
                }
            }
            "Finished" => {
                progress_stop.store(true, Ordering::Relaxed);
                let (exit, _runtime) = message.body().deserialize::<(u32, u32)>().unwrap_or((2, 0));
                return match exit {
                    1 => Ok(()),
                    // 3 is cancelled and 9 cancelled for a higher-priority
                    // transaction; neither is a failure of the update itself.
                    3 | 9 => Err(InstallEvent::Cancelled),
                    _ if not_authorized => Err(InstallEvent::Cancelled),
                    _ => Err(InstallEvent::Failed(
                        last_error
                            .take()
                            .filter(|message| !message.trim().is_empty())
                            .unwrap_or_else(|| {
                                t("The package service could not install the update.")
                            }),
                    )),
                };
            }
            _ => {}
        }
    }

    progress_stop.store(true, Ordering::Relaxed);
    Err(InstallEvent::Failed(t(
        "The package service stopped without finishing.",
    )))
}

/// Forward the transaction's `Percentage` and `Status` property changes to the
/// installing page.
///
/// This runs on its own thread: zbus filters a subscription by interface, so
/// the transaction's `Finished`/`ErrorCode` and the `Properties` interface
/// cannot share one iterator. The thread ends on the terminal "finished"
/// status, so it does not outlive the install.
fn monitor_transaction_properties(
    connection: zbus::blocking::Connection,
    transaction_path: zbus::zvariant::OwnedObjectPath,
    stop: Arc<AtomicBool>,
    sender: Sender<InstallEvent>,
) {
    let Ok(properties) = zbus::blocking::Proxy::new(
        &connection,
        PACKAGEKIT_SERVICE,
        transaction_path.as_str(),
        "org.freedesktop.DBus.Properties",
    ) else {
        return;
    };
    let Ok(signals) = properties.receive_signal("PropertiesChanged") else {
        return;
    };

    let mut last_status = None;
    let mut last_percentage = 1000u32;
    for message in signals {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let body = message.body();
        let Ok((interface, changed, _invalidated)) = body.deserialize::<(
            String,
            HashMap<String, zbus::zvariant::OwnedValue>,
            Vec<String>,
        )>() else {
            continue;
        };
        if interface != PACKAGEKIT_TRANSACTION_INTERFACE {
            continue;
        }
        let status = changed
            .get("Status")
            .and_then(|value| value.downcast_ref::<u32>().ok());
        let percentage = changed
            .get("Percentage")
            .and_then(|value| value.downcast_ref::<u32>().ok());
        if let Some(status) = status {
            if last_status != Some(status) {
                last_status = Some(status);
                let _ = sender.send_blocking(InstallEvent::Status(status_text(status)));
            }
        }
        if let Some(percentage) = percentage {
            if percentage != last_percentage {
                last_percentage = percentage;
                // PackageKit reports 101 when it has no estimate.
                let progress = (percentage <= 100).then(|| f64::from(percentage) / 100.0);
                let _ = sender.send_blocking(InstallEvent::Progress(progress));
            }
        }
        // 18 is the terminal "finished" status.
        if status == Some(18) {
            break;
        }
    }
}

/// The `LANG` the auth dialog and package messages should use.
fn locale_hint() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    "en_US.UTF-8".to_string()
}

/// The short status line for the PackageKit status enum values the install
/// actually passes through.
fn status_text(status: u32) -> String {
    let msgid = match status {
        1 => "Waiting for the package service...",
        2 => "Preparing the update...",
        8 => "Downloading the package...",
        9 => "Installing the package...",
        10 => "Updating the package...",
        13 => "Resolving dependencies...",
        14 => "Verifying the package signature...",
        16 | 26 => "Writing the update...",
        18 => "Finishing the update...",
        27 => "Loading the package cache...",
        _ => "Installing...",
    };
    t(msgid)
}

/// `pkexec` runs one package manager command as root; the polkit dialog is its
/// only authentication UI.
fn pkexec_command(format: PackageFormat, package: &Path) -> (String, Vec<String>) {
    let package = package.to_string_lossy().to_string();
    let args: Vec<String> = match format {
        PackageFormat::Deb => vec!["apt-get".into(), "install".into(), "-y".into(), package],
        PackageFormat::Rpm => vec!["dnf".into(), "install".into(), "-y".into(), package],
        PackageFormat::Arch => vec![
            "pacman".into(),
            "-U".into(),
            "--noconfirm".into(),
            "--overwrite".into(),
            // The Arch package owns the sound files; overwriting the old
            // copies is what the updater script tells pacman to do.
            "usr/share/apexshot/sounds/*.ogg".into(),
            package,
        ],
    };
    ("pkexec".to_string(), args)
}

fn run_pkexec(
    format: PackageFormat,
    package: &Path,
    sender: &Sender<InstallEvent>,
) -> Result<(), InstallEvent> {
    let (program, args) = pkexec_command(format, package);
    let log_path = install_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log = std::fs::File::create(&log_path)
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;

    let _ = sender.send_blocking(InstallEvent::Status(t("Waiting for permission...")));
    let mut child = Command::new(&program)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| {
            InstallEvent::Failed(tfmt(
                "Could not start the package manager: {error}",
                &[("error", &error.to_string())],
            ))
        })?;

    let _ = sender.send_blocking(InstallEvent::Status(t("Installing...")));
    let _ = sender.send_blocking(InstallEvent::Progress(None));

    let status = child
        .wait()
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;
    if status.success() {
        let _ = sender.send_blocking(InstallEvent::Progress(Some(1.0)));
        return Ok(());
    }
    if is_cancelled_exit(status.code()) {
        return Err(InstallEvent::Cancelled);
    }
    let stderr = std::fs::read_to_string(&log_path).unwrap_or_default();
    Err(InstallEvent::Failed(stderr_tail(&stderr)))
}

fn install_log_path() -> PathBuf {
    download::update_directory()
        .unwrap_or_else(std::env::temp_dir)
        .join("install.log")
}

/// `pkexec`'s own exits for "dismissed" and "not authorized".
fn is_cancelled_exit(code: Option<i32>) -> bool {
    matches!(code, Some(126) | Some(127))
}

/// The last few meaningful lines of the package manager's stderr, or a generic
/// failure when it wrote nothing.
fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let start = lines.len().saturating_sub(3);
    let tail = lines[start..].join("\n");
    if tail.is_empty() {
        t("The package manager could not install the update.")
    } else {
        tail
    }
}

/// The Flatpak portal's own update monitor. The sandbox cannot run the host's
/// package manager, so the portal replaces both the download and the install.
fn run_flatpak(sender: &Sender<InstallEvent>) -> Result<(), InstallEvent> {
    let connection = zbus::blocking::Connection::session().map_err(|error| {
        InstallEvent::Failed(tfmt(
            "Could not reach the Flatpak service: {error}",
            &[("error", &error.to_string())],
        ))
    })?;
    let portal = zbus::blocking::Proxy::new(
        &connection,
        FLATPAK_PORTAL,
        FLATPAK_PORTAL_PATH,
        FLATPAK_PORTAL,
    )
    .map_err(|error| InstallEvent::Failed(error.to_string()))?;
    let monitor_path: zbus::zvariant::OwnedObjectPath = portal
        .call(
            "CreateUpdateMonitor",
            &(HashMap::<String, zbus::zvariant::OwnedValue>::new(),),
        )
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;
    let monitor = zbus::blocking::Proxy::new(
        &connection,
        FLATPAK_PORTAL,
        monitor_path.as_str(),
        FLATPAK_MONITOR_INTERFACE,
    )
    .map_err(|error| InstallEvent::Failed(error.to_string()))?;

    let signal_proxy = monitor.clone();
    let signals = signal_proxy
        .receive_signal("Progress")
        .map_err(|error| InstallEvent::Failed(error.to_string()))?;

    monitor
        .call::<_, _, ()>(
            "Update",
            &("", HashMap::<String, zbus::zvariant::OwnedValue>::new()),
        )
        .map_err(|error| {
            InstallEvent::Failed(tfmt(
                "The Flatpak update could not start: {error}",
                &[("error", &error.to_string())],
            ))
        })?;

    let _ = sender.send_blocking(InstallEvent::Status(t("Updating...")));

    for message in signals {
        let body = message.body();
        let Ok(progress) = body.deserialize::<HashMap<String, zbus::zvariant::OwnedValue>>() else {
            continue;
        };
        let number = |key: &str| {
            progress
                .get(key)
                .and_then(|value| value.downcast_ref::<u32>().ok())
        };
        let status = number("status").unwrap_or(0);
        let current = number("progress");
        let n_ops = number("n_ops");
        let op = number("op");

        match status {
            // 1: the portal could not find anything to update in its sources.
            1 => {
                let _ = monitor.call::<_, _, ()>("Close", &());
                return Err(InstallEvent::Failed(t(
                    "This version is not in your Flatpak repository yet. Try again later.",
                )));
            }
            2 => {
                let _ = monitor.call::<_, _, ()>("Close", &());
                return Ok(());
            }
            3 => {
                let _ = monitor.call::<_, _, ()>("Close", &());
                let message = progress
                    .get("error_message")
                    .and_then(|value| value.downcast_ref::<&str>().ok())
                    .filter(|message| !message.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| t("The Flatpak update failed."));
                return Err(InstallEvent::Failed(message));
            }
            _ => {
                if let (Some(current), Some(n_ops), Some(op)) = (current, n_ops, op) {
                    let total = f64::from(n_ops.max(1));
                    let fraction = (f64::from(op) + f64::from(current) / 100.0) / total;
                    let _ = sender
                        .send_blocking(InstallEvent::Progress(Some(fraction.clamp(0.0, 1.0))));
                } else if let Some(current) = current {
                    let _ = sender.send_blocking(InstallEvent::Progress(Some(
                        f64::from(current).clamp(0.0, 100.0) / 100.0,
                    )));
                }
            }
        }
    }

    // The stream ended without a terminal status; release the monitor so the
    // portal does not keep it for the rest of the session.
    let _ = monitor.call::<_, _, ()>("Close", &());
    Err(InstallEvent::Failed(t(
        "The Flatpak update stopped without finishing.",
    )))
}

/// The steps that follow a package install, each announced on the status row.
fn post_install(
    native: bool,
    extension_zip: Option<&Path>,
    sender: &Sender<InstallEvent>,
) -> Installed {
    let mut installed = Installed { extension: None };
    if !native {
        return installed;
    }

    if let Some(zip) = extension_zip {
        let _ = sender.send_blocking(InstallEvent::Status(t("Refreshing the GNOME extension...")));
        installed.extension = Some(gnome_extension::refresh_from_archive(zip));
    }

    let _ = sender.send_blocking(InstallEvent::Status(t("Tidying the startup entries...")));
    fixup_autostart();
    installed
}

/// Rewrite the user's own autostart entries so they point at the package's
/// binary. Only files that already exist are touched; the package's
/// `/etc/xdg/autostart` entry is authoritative otherwise.
pub fn fixup_autostart() -> usize {
    if crate::app_identity::portal_only() {
        return 0;
    }
    let Some(config_home) = config_home() else {
        return 0;
    };
    let autostart = config_home.join("autostart");
    let mut rewritten = 0;
    for name in ["apexshot.desktop", "apexshot-daemon.desktop"] {
        let path = autostart.join(name);
        if path.is_file() && std::fs::write(&path, autostart_entry()).is_ok() {
            rewritten += 1;
        }
    }
    rewritten
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// The autostart content the updater writes, matching the packaged daemon
/// entry so a stale user copy cannot shadow it.
pub fn autostart_entry() -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=ApexShot Daemon\n\
         Comment=ApexShot screenshot daemon — tray icon and hotkey listener\n\
         Exec={} daemon\n\
         Icon={}\n\
         Categories=Utility;\n\
         Keywords=screenshot;capture;record;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=2\n\
         Hidden=false\n\
         NoDisplay=true\n",
        crate::app_identity::OFFICIAL_BINARY,
        crate::app_identity::icon_name(),
    )
}

/// Stop the running daemon and start the freshly installed binary detached.
/// The caller polls [`crate::daemon::is_daemon_running`] first so the new
/// process does not find the old one still holding the bus name.
pub fn spawn_daemon(binary: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(binary);
    super::without_update_previews(&mut command);
    command
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command.spawn().map(|_| ())
}

/// How long the restart waits for the old daemon to release its bus name.
pub const RESTART_WAIT: Duration = Duration::from_millis(100);
pub const RESTART_ATTEMPTS: u32 = 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_installs_only_target_x86_64() {
        assert!(supports_native_install("x86_64"));
        assert!(!supports_native_install("aarch64"));
        assert!(!supports_native_install("riscv64"));
    }

    #[test]
    fn dpkg_status_lines_decide_whether_the_package_is_installed() {
        assert!(dpkg_status_says_installed("install ok installed"));
        assert!(dpkg_status_says_installed("  install ok installed\n"));
        assert!(!dpkg_status_says_installed("deinstall ok config-files"));
        assert!(!dpkg_status_says_installed("unknown ok not-installed"));
        assert!(!dpkg_status_says_installed(""));
    }

    #[test]
    fn packagekit_counts_an_activatable_service() {
        assert!(packagekit_available_from(true, &[]));
        assert!(packagekit_available_from(
            false,
            &["org.freedesktop.PackageKit".to_string()]
        ));
        assert!(!packagekit_available_from(
            false,
            &["org.freedesktop.login1".to_string()]
        ));
        assert!(!packagekit_available_from(false, &[]));
    }

    #[test]
    fn pkexec_commands_name_the_distro_package_manager() {
        let (program, args) = pkexec_command(PackageFormat::Deb, Path::new("/tmp/a.deb"));
        assert_eq!(program, "pkexec");
        assert_eq!(args, vec!["apt-get", "install", "-y", "/tmp/a.deb"]);

        let (program, args) = pkexec_command(PackageFormat::Rpm, Path::new("/tmp/a.rpm"));
        assert_eq!(program, "pkexec");
        assert_eq!(args, vec!["dnf", "install", "-y", "/tmp/a.rpm"]);

        let (program, args) = pkexec_command(PackageFormat::Arch, Path::new("/tmp/a.pkg"));
        assert_eq!(program, "pkexec");
        assert_eq!(
            args,
            vec![
                "pacman",
                "-U",
                "--noconfirm",
                "--overwrite",
                "usr/share/apexshot/sounds/*.ogg",
                "/tmp/a.pkg"
            ]
        );
    }

    #[test]
    fn a_dismissed_polkit_prompt_reads_as_a_cancellation() {
        assert!(is_cancelled_exit(Some(126)));
        assert!(is_cancelled_exit(Some(127)));
        assert!(!is_cancelled_exit(Some(0)));
        assert!(!is_cancelled_exit(Some(1)));
        assert!(!is_cancelled_exit(None));
    }

    #[test]
    fn the_failure_message_is_the_tail_of_the_package_manager_output() {
        let stderr = "one\ntwo\n\nthree\nfour\nfive\n";
        assert_eq!(stderr_tail(stderr), "three\nfour\nfive");
        assert_eq!(
            stderr_tail("\n\n"),
            t("The package manager could not install the update.")
        );
    }

    #[test]
    fn the_autostart_entry_runs_the_installed_daemon() {
        let entry = autostart_entry();
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains(&format!(
            "Exec={} daemon\n",
            crate::app_identity::OFFICIAL_BINARY
        )));
        assert!(entry.contains("Hidden=false"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }
}

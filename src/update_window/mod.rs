//! The standalone update window for the Linux app.
//!
//! A found release opens one quiet surface that resolves the release's files,
//! downloads and verifies them, hands them to the system package service (or
//! `pkexec`), refreshes the GNOME extension, and offers to restart the daemon.
//! The window is its own GTK application so a second launch presents the
//! window that is already open instead of building another.

mod download;
mod install;
mod release;
mod running;
mod ui;
mod window;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::Application;

use crate::update::UpdateInfo;

/// What `apexshot update-window` was asked to do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateWindowFlags {
    /// Ignore a snoozed version; the tray's update item uses this.
    pub force: bool,
    /// Print the usage instead of opening a window.
    pub help: bool,
}

/// Parse the flags after `update-window`.
pub fn parse_flags(args: &[String]) -> Result<UpdateWindowFlags, String> {
    let mut flags = UpdateWindowFlags::default();
    for arg in args {
        match arg.as_str() {
            "--force" => flags.force = true,
            "--help" | "-h" => flags.help = true,
            other => {
                return Err(format!(
                    "unknown option '{other}'. usage: apexshot update-window [--force]"
                ))
            }
        }
    }
    Ok(flags)
}

/// The window's usage line.
pub const USAGE: &str = "usage: apexshot update-window [--force]";

/// The object a second launch calls on the window that already owns the name.
const WINDOW_OBJECT_PATH: &str = "/org/apexshot/UpdateWindow";
const WINDOW_INTERFACE: &str = "org.apexshot.UpdateWindow";
/// How long a second launch waits for the open window to answer.
const PRESENT_TIMEOUT: Duration = Duration::from_secs(2);

/// Present `update`, snoozing the version as the prompt appears.
pub fn show(update: UpdateInfo) -> anyhow::Result<()> {
    // GNOME Shell needs the desktop entry's id to draw the app's own name and
    // icon instead of a generic window.
    if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    }

    // Presenting the window counts as seeing this version; a dismissal rests
    // it for a day, and the tray action opens it again with --force.
    crate::update::snooze_prompt(&update);

    // One update window per session. The window shares the main application
    // id below, so GApplication cannot enforce that itself; a bus name owned
    // for the life of the process does. A second launch (the daemon's prompt
    // racing a tray click) asks the owner to come forward instead of leaving
    // the user with a hidden window and no visible effect.
    let (present_tx, present_rx) = async_channel::bounded::<()>(1);
    let _instance = match claim_single_instance(present_tx) {
        WindowInstance::AlreadyOpen => {
            // The bus call is the only proof the window is reachable; when it
            // fails the old message stays as the honest answer.
            if !present_open_window() {
                eprintln!("[update] The update window is already open.");
            }
            return Ok(());
        }
        WindowInstance::Primary(connection) => Some(connection),
        // A session without a bus, or one that refuses the name, cannot
        // coordinate two windows; the update still opens one.
        WindowInstance::Uncoordinated => None,
    };

    // The main application id lets GNOME Shell match the window to the
    // installed desktop entry, so the dock shows ApexShot's name and icon like
    // Settings, History, and the editors. NON_UNIQUE keeps it from handing
    // activation to whichever of those already holds the id.
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    // The receiver is drained once the window exists, so a Present that
    // arrives during startup waits in the channel instead of being lost.
    let present_rx = Rc::new(RefCell::new(Some(present_rx)));
    let update_for_activate = update;
    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
        if let Some(existing_window) = application.windows().first() {
            existing_window.present();
            return;
        }
        window::build_window(application, update_for_activate.clone());
        let Some(receiver) = present_rx.borrow_mut().take() else {
            return;
        };
        let application = application.clone();
        gtk4::glib::spawn_future_local(async move {
            while receiver.recv().await.is_ok() {
                if let Some(window) = application.windows().first() {
                    // Presenting a minimized window alone does not restore it
                    // on every compositor, so unminimize first.
                    window.unminimize();
                    window.present();
                }
            }
        });
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

/// Look for a newer release and open the window when there is one.
///
/// `force` skips the per-version snooze; the tray's update item passes it so
/// the window always opens while the item is visible.
pub fn check_and_show(force: bool) -> anyhow::Result<()> {
    // zbus runs on the Tokio executor, and serving the window's `Present`
    // object spawns onto it even through the blocking API. This subcommand
    // starts outside any runtime, so it keeps one entered for its lifetime.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let _runtime = runtime.enter();

    // Development builds can aim the window at any published tag, including
    // the installed one, to exercise the download and install end to end.
    #[cfg(debug_assertions)]
    if let Ok(version) = std::env::var("APEXSHOT_UPDATE_WINDOW_VERSION") {
        let version = version.trim().trim_start_matches('v').to_string();
        return show(UpdateInfo {
            release_url: format!("{}/tag/v{version}", crate::update::RELEASES_URL),
            version,
            studio_release_timestamp: None,
            studio_key_fingerprint: None,
        });
    }
    let update = crate::update::cached_update().or_else(crate::update::check_for_update);
    let Some(update) = update else {
        return Ok(());
    };
    if !force && crate::update::prompt_is_snoozed(&update) {
        return Ok(());
    }
    show(update)
}

/// The ApexShot binary this process should start next — for the restart on the
/// done page and for opening another window of the same build.
///
/// A package upgrade replaces the file that this process is running from, and
/// the kernel then reports the executable as `/usr/bin/apexshot (deleted)`.
/// The installed path is preferred once that has happened; a development build
/// keeps running its own binary.
pub fn binary_path() -> PathBuf {
    let official = PathBuf::from(crate::app_identity::OFFICIAL_BINARY);
    match std::env::current_exe() {
        Ok(path) if strip_deleted_suffix(&path) != path => {
            let stripped = strip_deleted_suffix(&path);
            if official.exists() {
                official
            } else {
                stripped
            }
        }
        Ok(path) => path,
        Err(_) => official,
    }
}

/// Start each remembered window with the freshly installed binary.
///
/// The children are detached into their own process group and quiet on all
/// three streams: they outlive this window, which closes right after a restart,
/// and each one is reaped on its own thread.
pub(super) fn reopen_windows(commands: Vec<Vec<String>>) {
    use std::os::unix::process::CommandExt;

    if commands.is_empty() {
        return;
    }
    let binary = binary_path();
    for args in commands {
        let mut command = std::process::Command::new(&binary);
        without_update_previews(&mut command);
        command
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0);
        match command.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => eprintln!("[update] Could not reopen a window: {error}"),
        }
    }
}

/// Rename this process so the package's own maintainer scripts leave it alone.
///
/// The packages' `prerm`/`postinst` (and the Arch install hook) run
/// `killall -9 apexshot` to clear stale daemons, and the outgoing package's
/// `prerm` runs during every upgrade, so fixing the scripts cannot protect
/// installs already in the field. `killall` matches the kernel's process name;
/// a different name keeps the window alive through the install so it can show
/// the result. The name stays under 15 bytes: at exactly 15, `killall` assumes
/// truncation and falls back to the command line, which still says `apexshot`.
pub fn rename_process() {
    const NAME: &[u8] = b"apexshot-upd\0";
    // SAFETY: PR_SET_NAME reads a NUL-terminated string of at most 16 bytes
    // and only changes the calling thread's name, which is the main thread
    // here, the one /proc/<pid>/stat reports.
    unsafe {
        libc::prctl(libc::PR_SET_NAME, NAME.as_ptr() as libc::c_ulong, 0, 0, 0);
    }
}

/// The interface a second launch calls to bring the open window forward.
struct UpdateWindowIface {
    present: async_channel::Sender<()>,
}

#[zbus::interface(name = "org.apexshot.UpdateWindow")]
impl UpdateWindowIface {
    /// Ask the window's process to present its window.
    fn present(&self) {
        // A full channel already holds a wake-up; the window only needs one.
        let _ = self.present.try_send(());
    }
}

/// What this launch found when it tried to own the window's bus name.
enum WindowInstance {
    /// This process owns the name and serves `Present` on the same connection.
    Primary(zbus::blocking::Connection),
    /// Another process owns the name and shows the window.
    AlreadyOpen,
    /// No session bus, or a bus that refused the name; no coordination is
    /// possible, so the window opens anyway.
    Uncoordinated,
}

/// Own the session bus name that marks an open update window.
fn claim_single_instance(present: async_channel::Sender<()>) -> WindowInstance {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return WindowInstance::Uncoordinated;
    };
    // Serve the interface before the name becomes visible, so a caller that
    // finds the name always finds the object too. Serving is a convenience; a
    // bus that refuses it must not stop the update from opening its window.
    if let Err(error) = connection
        .object_server()
        .at(WINDOW_OBJECT_PATH, UpdateWindowIface { present })
    {
        eprintln!("[update] Could not serve the update window's bus interface: {error}");
    }
    let name = format!("{}.UpdateWindow", crate::app_identity::app_id());
    match connection.request_name_with_flags(
        name.as_str(),
        zbus::fdo::RequestNameFlags::DoNotQueue.into(),
    ) {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner)
        | Ok(zbus::fdo::RequestNameReply::AlreadyOwner) => WindowInstance::Primary(connection),
        // zbus reports a name another process holds as an error, not as an
        // `Exists` reply.
        Ok(_) | Err(zbus::Error::NameTaken) => WindowInstance::AlreadyOpen,
        // A bus that refuses the name (a sandbox without the permission)
        // should not block the update.
        Err(_) => WindowInstance::Uncoordinated,
    }
}

/// Ask the process that owns the window's name to present its window.
fn present_open_window() -> bool {
    let name = format!("{}.UpdateWindow", crate::app_identity::app_id());
    let Ok(connection) = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(PRESENT_TIMEOUT).build())
    else {
        return false;
    };
    connection
        .call_method(
            Some(name.as_str()),
            WINDOW_OBJECT_PATH,
            Some(WINDOW_INTERFACE),
            "Present",
            &(),
        )
        .is_ok()
}

/// Drop the development-only update previews from a process this window
/// starts. The restarted daemon and the reopened windows are the user's
/// normal app; inheriting a forced "update available" would make the new
/// daemon announce the update that was just installed.
pub(crate) fn without_update_previews(command: &mut std::process::Command) {
    command
        .env_remove("APEXSHOT_UPDATE_PREVIEW")
        .env_remove("APEXSHOT_UPDATE_WINDOW_VERSION");
}

/// Strip the kernel's ` (deleted)` marker from an executable path.
pub fn strip_deleted_suffix(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    let Some(stripped) = name.strip_suffix(" (deleted)") else {
        return path.to_path_buf();
    };
    match path.parent() {
        Some(parent) => parent.join(stripped),
        None => PathBuf::from(stripped),
    }
}

/// Inside the sandbox the host's Flatpak instance is the only way to start the
/// new build; `flatpak-spawn` is the portal client that does it.
pub(super) fn flatpak_spawn_available() -> bool {
    std::process::Command::new("flatpak-spawn")
        .arg("--version")
        .output()
        .is_ok()
}

pub(super) fn spawn_flatpak_daemon() -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = std::process::Command::new("flatpak-spawn");
    command
        .args([
            "--host",
            "flatpak",
            "run",
            "--command=apexshot",
            crate::app_identity::app_id(),
            "daemon",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    command.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(args: &[&str]) -> Result<UpdateWindowFlags, String> {
        parse_flags(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_force_flag_skips_the_snooze_and_nothing_else_is_accepted() {
        assert_eq!(flags(&[]), Ok(UpdateWindowFlags::default()));
        assert_eq!(
            flags(&["--force"]),
            Ok(UpdateWindowFlags {
                force: true,
                help: false
            })
        );
        assert_eq!(
            flags(&["--help"]),
            Ok(UpdateWindowFlags {
                force: false,
                help: true
            })
        );
        assert!(flags(&["--force", "--help"]).is_ok());
        assert!(flags(&["--verbose"]).is_err());
    }

    #[test]
    fn a_replaced_binary_loses_the_kernels_deleted_marker() {
        assert_eq!(
            strip_deleted_suffix(Path::new("/usr/bin/apexshot (deleted)")),
            PathBuf::from("/usr/bin/apexshot")
        );
        assert_eq!(
            strip_deleted_suffix(Path::new("/usr/bin/apexshot")),
            PathBuf::from("/usr/bin/apexshot")
        );
        assert_eq!(
            strip_deleted_suffix(Path::new("apexshot (deleted)")),
            PathBuf::from("apexshot")
        );
        // A file whose name merely contains the marker is left alone.
        assert_eq!(
            strip_deleted_suffix(Path::new("/tmp/apexshot (deleted).bak")),
            PathBuf::from("/tmp/apexshot (deleted).bak")
        );
    }
}

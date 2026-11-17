//! The update window itself: available, downloading, ready, installing,
//! done, and failed pages, with no page transitions.
//!
//! The window reads as part of the onboarding flow — brand mark, centred
//! headline, one quiet fact card, one primary action — and every state change
//! is a stack swap. All blocking work happens on worker threads; this file only
//! arranges widgets and drains their event channels on the main loop.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation,
    Overlay, ProgressBar, Stack, StackTransitionType,
};

use crate::i18n::{t, tfmt};
use crate::settings::windowing;
use crate::update::UpdateInfo;

use super::download::{self, Event as DownloadEvent};
use super::install::{self, InstallMethod, PackageFormat};
use super::release;
use super::running::{self, OpenWork};
use super::ui::{
    action_row, body, card, fact_line, file_label, file_size, header, header_parts, install_css,
    page, page_body, primary_button, quiet_button, row, window_controls, InstallUi,
};

/// The window box.
const BASE_WIDTH: i32 = 560;
const BASE_HEIGHT: i32 = 540;

/// The centred column that holds every page.
const PANE_WIDTH: i32 = 480;

/// How long the closed windows get to finish saving and exit before the
/// install is called off.
const CLOSE_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// What the window remembers between states.
#[derive(Default)]
struct WindowState {
    /// Incremented per attempt so events from a superseded download are
    /// ignored even if its last event arrives after a retry.
    generation: u64,
    cancel: Arc<AtomicBool>,
    downloaded: Option<download::Downloaded>,
    assets: Option<release::SelectedAssets>,
    fetching_assets: bool,
}

pub(super) fn build_window(app: &Application, update: UpdateInfo) {
    crate::settings::ui_support::install_settings_css();
    install_css();

    let dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Update"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(BASE_WIDTH)
        .default_height(BASE_HEIGHT)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    root.add_css_class("update-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("update-pane");
    pane.set_size_request(PANE_WIDTH, -1);
    pane.set_halign(Align::Center);
    pane.set_hexpand(false);
    crate::onboarding::install_pane_drag(&pane, &window);

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::None);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);
    pane.append(&stack);

    let state = Rc::new(RefCell::new(WindowState::default()));
    let method = install::detect();
    let restartable = match &method {
        // The sandbox needs the host's Flatpak instance to start the new build.
        InstallMethod::Flatpak => super::flatpak_spawn_available(),
        _ => true,
    };

    // ── Update available ─────────────────────────────────────────────────
    let available = page();
    let available_body = body();
    available_body.append(&header(
        dark,
        &t("Update available"),
        &tfmt(
            "ApexShot v{version} is ready to install.",
            &[("version", &update.version)],
        ),
    ));

    let available_card = card();
    let (installed_row, _) = row(&t("Installed version"), env!("CARGO_PKG_VERSION"));
    available_card.append(&installed_row);
    let (latest_row, _) = row(&t("Latest version"), &update.version);
    available_card.append(&latest_row);
    let (download_size_row, download_size_value) = row(&t("Download size"), "");
    download_size_row.set_visible(false);
    available_card.append(&download_size_row);
    available_body.append(&available_card);

    let link_status = Label::new(None);
    link_status.add_css_class("onboarding-status-error");
    link_status.add_css_class("update-link-status");
    link_status.set_wrap(true);
    link_status.set_max_width_chars(48);
    link_status.set_justify(gtk4::Justification::Center);
    link_status.set_visible(false);

    let available_status = Label::new(None);
    available_status.add_css_class("update-error");
    available_status.add_css_class("update-link-status");
    available_status.set_wrap(true);
    available_status.set_max_width_chars(48);
    available_status.set_justify(gtk4::Justification::Center);
    available_status.set_visible(false);

    if !update.release_url.trim().is_empty() {
        let url = update.release_url.clone();
        let notes = Button::with_label(&format!("{}  →", t("Release notes")));
        notes.add_css_class("update-link");
        notes.set_halign(Align::Center);
        notes.set_margin_top(12);
        if let Some(label) = notes
            .child()
            .and_then(|child| child.downcast::<Label>().ok())
        {
            label.set_markup(&format!(
                "<u>{}</u>  →",
                glib::markup_escape_text(&t("Release notes"))
            ));
        }
        let status = link_status.clone();
        notes.connect_clicked(move |_| crate::onboarding::ui::open_link(&url, &status));
        available_body.append(&notes);
    }
    available_body.append(&available_status);
    available_body.append(&link_status);

    let update_now = primary_button(&t("Update now"));
    let later = quiet_button(&t("Later"));
    later.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let available_actions = action_row(Some(&later), &update_now);
    page_body(&available, &available_body, &available_actions);
    stack.add_named(&available, Some("available"));

    // ── Downloading ──────────────────────────────────────────────────────
    let downloading = page();
    let downloading_body = body();
    downloading_body.append(&header(
        dark,
        &t("Downloading update"),
        &tfmt(
            "ApexShot v{version} is on its way. Keep this window open.",
            &[("version", &update.version)],
        ),
    ));

    let download_card = card();
    let (download_name_row, download_name) = row(&t("Filename"), &update.version);
    download_card.append(&download_name_row);
    let (download_status_row, download_status) = row(&t("Status"), &t("Starting..."));
    download_card.append(&download_status_row);
    let (download_progress_row, download_progress) = row(&t("Progress"), "");
    download_card.append(&download_progress_row);
    let (download_speed_row, download_speed) = row(&t("Download speed"), "0 B/s");
    download_card.append(&download_speed_row);
    let (download_file_size_row, download_size) = row(&t("File size"), "");
    download_card.append(&download_file_size_row);

    let progress_bar = ProgressBar::new();
    progress_bar.add_css_class("update-progress");
    progress_bar.set_show_text(false);
    progress_bar.set_fraction(0.0);
    progress_bar.set_margin_top(10);
    download_card.append(&progress_bar);
    downloading_body.append(&download_card);

    let cancel = quiet_button(&t("Cancel download"));
    cancel.connect_clicked({
        let state = state.clone();
        let window = window.clone();
        move |_| {
            state.borrow().cancel.store(true, Ordering::Relaxed);
            window.close();
        }
    });
    let download_actions = action_row(None, &cancel);
    page_body(&downloading, &downloading_body, &download_actions);
    stack.add_named(&downloading, Some("downloading"));

    // ── Update ready ─────────────────────────────────────────────────────
    let ready = page();
    let ready_body = body();
    ready_body.append(&header(
        dark,
        &t("Update ready"),
        &tfmt(
            "ApexShot v{version} has finished downloading.",
            &[("version", &update.version)],
        ),
    ));

    let ready_card = card();
    let (ready_name_row, ready_name) = row(&t("Filename"), "");
    ready_card.append(&ready_name_row);
    let (ready_status_row, _) = row(&t("Status"), &t("Downloaded"));
    ready_card.append(&ready_status_row);
    let (ready_size_row, ready_size) = row(&t("File size"), "");
    ready_card.append(&ready_size_row);
    ready_body.append(&ready_card);

    let ready_note = Label::new(None);
    ready_note.add_css_class("settings-sub-option");
    ready_note.set_halign(Align::Center);
    ready_note.set_xalign(0.5);
    ready_note.set_justify(gtk4::Justification::Center);
    ready_note.set_wrap(true);
    ready_note.set_max_width_chars(46);
    ready_note.set_margin_top(14);
    ready_note.set_visible(false);
    ready_body.append(&ready_note);

    let install_now = primary_button(&t("Install now"));
    let close_ready = quiet_button(&t("Later"));
    close_ready.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let ready_actions = action_row(Some(&close_ready), &install_now);
    page_body(&ready, &ready_body, &ready_actions);
    stack.add_named(&ready, Some("ready"));

    // ── Recording in progress ────────────────────────────────────────────
    // A native install stops every ApexShot process, so a recording that is
    // still running has to finish first; its file would be lost otherwise.
    let recording_blocked = page();
    let recording_body = body();
    recording_body.append(&header(
        dark,
        &t("Finish your recording first"),
        &t("Installing restarts ApexShot, which would stop the recording in progress."),
    ));

    let recording_install = primary_button(&t("Install now"));
    recording_install.set_sensitive(false);
    let recording_later = quiet_button(&t("Later"));
    recording_later.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let recording_actions = action_row(Some(&recording_later), &recording_install);
    page_body(&recording_blocked, &recording_body, &recording_actions);
    stack.add_named(&recording_blocked, Some("recording-blocked"));

    // ── Open windows ─────────────────────────────────────────────────────
    // The list is filled and refreshed from `/proc`; the launcher and its
    // child collapse into one line per window.
    let work_blocked = page();
    let work_body = body();
    work_body.append(&header(
        dark,
        &t("Your work will reopen"),
        &t("These windows close while ApexShot updates and reopen with your edits afterwards:"),
    ));

    let work_card = card();
    let work_list = GtkBox::new(Orientation::Vertical, 0);
    work_card.append(&work_list);
    work_body.append(&work_card);

    let work_install = primary_button(&t("Install now"));
    let work_back = quiet_button(&t("Back"));
    let work_actions = action_row(Some(&work_back), &work_install);
    page_body(&work_blocked, &work_body, &work_actions);
    stack.add_named(&work_blocked, Some("work-blocked"));

    // ── Installing ───────────────────────────────────────────────────────
    let installing = page();
    let installing_body = body();
    installing_body.append(&header(
        dark,
        &t("Installing update"),
        &tfmt(
            "ApexShot v{version} is being installed. Keep this window open.",
            &[("version", &update.version)],
        ),
    ));

    let installing_card = card();
    let (install_status_row, install_status) = row(&t("Status"), &t("Installing..."));
    installing_card.append(&install_status_row);
    let install_progress = ProgressBar::new();
    install_progress.add_css_class("update-progress");
    install_progress.set_show_text(false);
    install_progress.set_fraction(0.0);
    install_progress.set_pulse_step(0.08);
    install_progress.set_margin_top(10);
    installing_card.append(&install_progress);
    installing_body.append(&installing_card);
    // No actions: once the package manager has started, stopping it would
    // leave the installation half done.
    page_body(
        &installing,
        &installing_body,
        &GtkBox::new(Orientation::Vertical, 0),
    );
    stack.add_named(&installing, Some("installing"));

    let pulse = Rc::new(Cell::new(false));
    {
        let install_progress = install_progress.clone();
        let pulse = pulse.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
            if pulse.get() {
                install_progress.pulse();
            }
            glib::ControlFlow::Continue
        });
    }

    // ── Done ─────────────────────────────────────────────────────────────
    // The public commands that bring back the windows this install closed.
    // Filled when the save-and-close step succeeds; the done page's actions
    // take the list, and the install pages reopen it when the install does not
    // go through.
    let reopen_commands: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    // Set when the failed page is only there because a window would not close:
    // Try again returns to the ready page instead of restarting the download.
    let retry_to_ready = Rc::new(Cell::new(false));

    let done = page();
    let done_body = body();
    let (done_heading, done_copy) = header_parts(
        dark,
        &t("ApexShot is up to date"),
        &tfmt(
            "ApexShot v{version} is installed.",
            &[("version", &update.version)],
        ),
    );
    done_body.append(&done_heading);

    let done_note = Label::new(None);
    done_note.add_css_class("settings-sub-option");
    done_note.set_halign(Align::Center);
    done_note.set_xalign(0.5);
    done_note.set_justify(gtk4::Justification::Center);
    done_note.set_wrap(true);
    done_note.set_max_width_chars(46);
    done_note.set_margin_top(16);
    done_note.set_visible(false);
    done_body.append(&done_note);

    let restart_button = primary_button(&t("Restart ApexShot"));
    restart_button.set_visible(restartable);
    let close_done = quiet_button(&t("Close"));
    close_done.connect_clicked({
        let window = window.clone();
        let reopen_commands = reopen_commands.clone();
        move |_| {
            // Closing here must leave the user where they were working, not
            // with an install they have to reopen by hand.
            super::reopen_windows(std::mem::take(&mut *reopen_commands.borrow_mut()));
            window.close()
        }
    });
    let done_actions = if restartable {
        action_row(Some(&close_done), &restart_button)
    } else {
        action_row(None, &close_done)
    };
    page_body(&done, &done_body, &done_actions);
    stack.add_named(&done, Some("done"));

    restart_button.connect_clicked({
        let window = window.clone();
        let restart_method = method.clone();
        let reopen_commands = reopen_commands.clone();
        move |_| restart_application(&window, &restart_method, &reopen_commands)
    });

    // ── Failed ───────────────────────────────────────────────────────────
    let failed = page();
    let failed_body = body();
    failed_body.append(&header(
        dark,
        &t("Update failed"),
        &tfmt(
            "ApexShot v{version} could not be updated.",
            &[("version", &update.version)],
        ),
    ));

    let failed_card = card();
    let (failed_name_row, failed_name) = row(&t("Filename"), "");
    failed_card.append(&failed_name_row);
    let (failed_status_row, _) = row(&t("Status"), &t("Failed"));
    failed_card.append(&failed_status_row);
    failed_body.append(&failed_card);

    let failure_status = Label::new(None);
    failure_status.add_css_class("update-error");
    failure_status.set_wrap(true);
    failure_status.set_max_width_chars(52);
    failure_status.set_justify(gtk4::Justification::Center);
    failure_status.set_margin_top(14);
    failure_status.set_visible(false);
    failed_body.append(&failure_status);

    let retry = primary_button(&t("Try again"));
    let close_failed = quiet_button(&t("Close"));
    close_failed.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let failed_actions = action_row(Some(&close_failed), &retry);
    page_body(&failed, &failed_body, &failed_actions);
    stack.add_named(&failed, Some("failed"));

    // Set just before a native install that closed windows, so the done page
    // can tell the user how to get them back.
    let closed_windows = Rc::new(Cell::new(false));

    let install_ui = Rc::new(InstallUi {
        stack: stack.clone(),
        status: install_status.clone(),
        progress: install_progress.clone(),
        pulse: pulse.clone(),
        failure: failure_status.clone(),
        done_copy: done_copy.clone(),
        done_note: done_note.clone(),
        ready_note: ready_note.clone(),
        restartable,
        version: update.version.clone(),
        closed_windows: closed_windows.clone(),
        reopen: reopen_commands.clone(),
    });

    // ── Release assets ───────────────────────────────────────────────────
    let fetch_assets: Rc<dyn Fn()> = {
        let state = state.clone();
        let stack = stack.clone();
        let update_now = update_now.clone();
        let failure_status = failure_status.clone();
        let download_size_row = download_size_row.clone();
        let download_size_value = download_size_value.clone();
        let failed_name = failed_name.clone();
        let version = update.version.clone();
        let method = method.clone();
        Rc::new(move || {
            let format = match &method {
                InstallMethod::PackageKit(format) | InstallMethod::Pkexec(format) => *format,
                _ => return,
            };
            if state.borrow().fetching_assets {
                return;
            }
            state.borrow_mut().fetching_assets = true;
            let (tx, rx) = async_channel::bounded(1);
            let fetch_version = version.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking(release::fetch_assets(&fetch_version));
            });

            let state = state.clone();
            let stack = stack.clone();
            let update_now = update_now.clone();
            let failure_status = failure_status.clone();
            let download_size_row = download_size_row.clone();
            let download_size_value = download_size_value.clone();
            let failed_name = failed_name.clone();
            let version_for_selection = version.clone();

            glib::spawn_future_local(async move {
                let result = rx.recv().await;
                state.borrow_mut().fetching_assets = false;
                let assets = match result {
                    Ok(Ok(assets)) => assets,
                    Ok(Err(error)) => {
                        failure_status.set_text(&tfmt(
                            "Could not check the release: {error}",
                            &[("error", &error)],
                        ));
                        failure_status.set_visible(true);
                        stack.set_visible_child_name("failed");
                        return;
                    }
                    Err(_) => return,
                };
                let host_fedora = match format {
                    PackageFormat::Rpm => release::host_fedora_version(),
                    _ => None,
                };
                match release::select_assets(&assets, format, &version_for_selection, host_fedora) {
                    Ok(selected) => {
                        if selected.package.size > 0 {
                            download_size_value
                                .set_text(&download::format_bytes(selected.package.size));
                            download_size_row.set_visible(true);
                        }
                        failed_name.set_text(&selected.package.name);
                        update_now.set_sensitive(true);
                        state.borrow_mut().assets = Some(selected);
                    }
                    Err(error) => {
                        failure_status.set_text(&error.message());
                        failure_status.set_visible(true);
                        stack.set_visible_child_name("failed");
                    }
                }
            });
        })
    };

    // ── Download ─────────────────────────────────────────────────────────
    let begin_download: Rc<dyn Fn()> = {
        let state = state.clone();
        let stack = stack.clone();
        let failure_status = failure_status.clone();
        let download_name = download_name.clone();
        let download_status = download_status.clone();
        let download_progress = download_progress.clone();
        let download_speed = download_speed.clone();
        let download_size = download_size.clone();
        let progress_bar = progress_bar.clone();
        let ready_name = ready_name.clone();
        let ready_size = ready_size.clone();
        Rc::new(move || {
            let Some(selected) = state.borrow().assets.clone() else {
                return;
            };
            failure_status.set_visible(false);
            let (generation, cancel) = {
                let mut state = state.borrow_mut();
                state.cancel.store(true, Ordering::Relaxed);
                state.generation = state.generation.wrapping_add(1);
                let cancel = Arc::new(AtomicBool::new(false));
                state.cancel = cancel.clone();
                state.downloaded = None;
                (state.generation, cancel)
            };
            let total = (selected.package.size > 0).then_some(selected.package.size);
            download_name.set_text(&selected.package.name);
            download_status.set_text(&t("Starting..."));
            download_progress.set_text(&download::progress_text(0, total));
            download_speed.set_text(&download::format_rate(0.0));
            download_size.set_text(&download::size_text(0, total));
            progress_bar.set_fraction(0.0);
            stack.set_visible_child_name("downloading");

            // The extension archive is only fetched for a GNOME session; the
            // refresh itself skips when the user never installed it.
            let extension = selected
                .extension
                .clone()
                .filter(|_| crate::gnome_extension::is_gnome_session());
            let request = download::DownloadRequest {
                package: selected.package,
                checksums: selected.checksums,
                extension,
            };

            match download::start(request, cancel) {
                Ok(events) => {
                    let state = state.clone();
                    let stack = stack.clone();
                    let failure_status = failure_status.clone();
                    let download_status = download_status.clone();
                    let download_progress = download_progress.clone();
                    let download_speed = download_speed.clone();
                    let download_size = download_size.clone();
                    let progress_bar = progress_bar.clone();
                    let ready_name = ready_name.clone();
                    let ready_size = ready_size.clone();
                    glib::spawn_future_local(async move {
                        while let Ok(event) = events.recv().await {
                            if state.borrow().generation != generation {
                                break;
                            }
                            match event {
                                DownloadEvent::Progress {
                                    received,
                                    total,
                                    bytes_per_second,
                                } => {
                                    download_status.set_text(&t("Downloading..."));
                                    download_progress
                                        .set_text(&download::progress_text(received, total));
                                    download_speed
                                        .set_text(&download::format_rate(bytes_per_second));
                                    download_size.set_text(&download::size_text(received, total));
                                    if let Some(total) = total.filter(|total| *total > 0) {
                                        progress_bar.set_fraction(
                                            (received as f64 / total as f64).clamp(0.0, 1.0),
                                        );
                                    }
                                }
                                DownloadEvent::Verifying => {
                                    // The transfer is complete; keep the bar full
                                    // instead of switching it to activity mode.
                                    download_status.set_text(&t("Verifying download..."));
                                    download_speed.set_text(&download::format_rate(0.0));
                                    progress_bar.set_fraction(1.0);
                                }
                                DownloadEvent::Finished(downloaded) => {
                                    let size = file_size(&downloaded.package);
                                    ready_name.set_text(&file_label(&downloaded.package));
                                    ready_size.set_text(&download::format_bytes(size));
                                    state.borrow_mut().downloaded = Some(downloaded);
                                    stack.set_visible_child_name("ready");
                                }
                                DownloadEvent::Failed(message) => {
                                    failure_status.set_text(&message);
                                    failure_status.set_visible(true);
                                    stack.set_visible_child_name("failed");
                                }
                                DownloadEvent::Cancelled => {}
                            }
                        }
                    });
                }
                Err(error) => {
                    failure_status.set_text(&error);
                    failure_status.set_visible(true);
                    stack.set_visible_child_name("failed");
                }
            }
        })
    };

    // ── Primary action ───────────────────────────────────────────────────
    let begin_update: Rc<dyn Fn()> = {
        let state = state.clone();
        let stack = stack.clone();
        let window = window.clone();
        let method = method.clone();
        let fetch_assets = fetch_assets.clone();
        let begin_download = begin_download.clone();
        let failure_status = failure_status.clone();
        let install_ui = install_ui.clone();
        Rc::new(move || match &method {
            InstallMethod::Terminal => match crate::update::launch_update() {
                Ok(()) => window.close(),
                Err(error) => {
                    failure_status.set_text(&error);
                    failure_status.set_visible(true);
                    stack.set_visible_child_name("failed");
                }
            },
            InstallMethod::Unsupported(message) => {
                failure_status.set_text(message);
                failure_status.set_visible(true);
                stack.set_visible_child_name("failed");
            }
            InstallMethod::Flatpak => {
                // The portal owns both the download and the install inside
                // the sandbox, so this page goes straight to the install.
                install_ui.start(InstallMethod::Flatpak, None, None);
            }
            InstallMethod::PackageKit(_) | InstallMethod::Pkexec(_) => {
                if state.borrow().assets.is_some() {
                    begin_download();
                } else {
                    fetch_assets();
                }
            }
        })
    };
    update_now.connect_clicked({
        let begin_update = begin_update.clone();
        move |_| begin_update()
    });
    retry.connect_clicked({
        let begin_update = begin_update.clone();
        let stack = stack.clone();
        let retry_to_ready = retry_to_ready.clone();
        move |_| {
            // After a window refused to close there is nothing to retry: the
            // download is still on disk and the ready page's Install now is
            // the way back in.
            if retry_to_ready.replace(false) {
                stack.set_visible_child_name("ready");
                return;
            }
            begin_update()
        }
    });

    // ── Install gate ─────────────────────────────────────────────────────
    // The native packages stop every ApexShot process before replacing the
    // files, and the outgoing package's script runs during an upgrade, so the
    // window checks for a recording and for open windows before it hands the
    // package over. Flatpak updates leave running instances alone.
    let native_install = matches!(
        &method,
        InstallMethod::PackageKit(_) | InstallMethod::Pkexec(_)
    );

    let poll_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let stop_poll: Rc<dyn Fn()> = {
        let poll_source = poll_source.clone();
        Rc::new(move || {
            if let Some(source) = poll_source.borrow_mut().take() {
                source.remove();
            }
        })
    };

    let refresh_work_list: Rc<dyn Fn(&[OpenWork])> = {
        let work_list = work_list.clone();
        Rc::new(move |work| {
            while let Some(child) = work_list.first_child() {
                work_list.remove(&child);
            }
            for line in running::summary(work) {
                work_list.append(&fact_line(&line));
            }
        })
    };

    let begin_install: Rc<dyn Fn()> = {
        let state = state.clone();
        let method = method.clone();
        let install_ui = install_ui.clone();
        let closed_windows = closed_windows.clone();
        let reopen_commands = reopen_commands.clone();
        let stop_poll = stop_poll.clone();
        Rc::new(move || {
            let Some(downloaded) = state.borrow().downloaded.clone() else {
                return;
            };
            // The done page only promises to reopen windows that this install
            // really did close.
            closed_windows.set(!reopen_commands.borrow().is_empty());
            stop_poll();
            install_ui.start(
                method.clone(),
                Some(downloaded.package),
                downloaded.extension,
            );
        })
    };

    // The poll keeps both checks current while a close-work page is visible.
    // It never installs on its own: only the primary button does that.
    let start_poll: Rc<dyn Fn()> = {
        let poll_source = poll_source.clone();
        let stack = stack.clone();
        let recording_install = recording_install.clone();
        let refresh_work_list = refresh_work_list.clone();
        Rc::new(move || {
            if let Some(source) = poll_source.borrow_mut().take() {
                source.remove();
            }
            let poll_slot = poll_source.clone();
            let stack = stack.clone();
            let recording_install = recording_install.clone();
            let refresh_work_list = refresh_work_list.clone();
            let source = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
                match stack.visible_child_name().as_deref() {
                    Some("recording-blocked") => {
                        recording_install.set_sensitive(!running::recording_active());
                    }
                    Some("work-blocked") => {
                        let work = running::open_work();
                        refresh_work_list(work.as_slice());
                    }
                    // The page was left or the window closed.
                    _ => {
                        *poll_slot.borrow_mut() = None;
                        return glib::ControlFlow::Break;
                    }
                }
                glib::ControlFlow::Continue
            });
            *poll_source.borrow_mut() = Some(source);
        })
    };

    // "Install now" on the ready page, and the recording page's primary once
    // the recording ends: run the checks before installing.
    let request_install: Rc<dyn Fn()> = {
        let stack = stack.clone();
        let recording_install = recording_install.clone();
        let refresh_work_list = refresh_work_list.clone();
        let start_poll = start_poll.clone();
        let begin_install = begin_install.clone();
        Rc::new(move || {
            if native_install && running::recording_active() {
                recording_install.set_sensitive(false);
                stack.set_visible_child_name("recording-blocked");
                start_poll();
                return;
            }
            if native_install {
                let work = running::open_work();
                if !work.is_empty() {
                    refresh_work_list(work.as_slice());
                    stack.set_visible_child_name("work-blocked");
                    start_poll();
                    return;
                }
            }
            begin_install();
        })
    };

    // "Install now" on the open-window page: close each window through its own
    // close handlers first, so the editors write their session sidecars, then
    // wait for those processes to exit before the package manager replaces the
    // files. An export that refuses to close leaves the window open and stops
    // the install instead of losing the encode.
    let save_and_install: Rc<dyn Fn()> = {
        let stack = stack.clone();
        let recording_install = recording_install.clone();
        let start_poll = start_poll.clone();
        let begin_install = begin_install.clone();
        let install_status = install_status.clone();
        let install_progress = install_progress.clone();
        let pulse = pulse.clone();
        let failure_status = failure_status.clone();
        let retry_to_ready = retry_to_ready.clone();
        let reopen_commands = reopen_commands.clone();
        Rc::new(move || {
            if native_install && running::recording_active() {
                recording_install.set_sensitive(false);
                stack.set_visible_child_name("recording-blocked");
                start_poll();
                return;
            }
            let windows = if native_install {
                running::running_windows()
            } else {
                Vec::new()
            };
            if windows.is_empty() {
                begin_install();
                return;
            }

            for window in &windows {
                running::request_graceful_close(window.pid);
            }
            install_status.set_text(&t("Saving your work…"));
            install_progress.set_fraction(0.0);
            pulse.set(true);
            stack.set_visible_child_name("installing");

            let (sender, receiver) = async_channel::bounded(1);
            let pids: Vec<i32> = windows.iter().map(|window| window.pid).collect();
            std::thread::spawn(move || {
                let survivors = running::wait_for_windows_to_close(&pids, CLOSE_WAIT_TIMEOUT);
                let _ = sender.send_blocking(survivors);
            });

            let stack = stack.clone();
            let begin_install = begin_install.clone();
            let pulse = pulse.clone();
            let failure_status = failure_status.clone();
            let retry_to_ready = retry_to_ready.clone();
            let reopen_commands = reopen_commands.clone();
            glib::spawn_future_local(async move {
                let Ok(survivors) = receiver.recv().await else {
                    return;
                };
                if survivors.is_empty() {
                    reopen_commands.replace(
                        windows
                            .into_iter()
                            .map(|window| window.reopen)
                            .filter(|reopen| !reopen.is_empty())
                            .collect(),
                    );
                    begin_install();
                    return;
                }
                // A window stayed open: an export is still encoding. Put back
                // the windows that did close and leave the install for later.
                let closed: Vec<Vec<String>> = windows
                    .iter()
                    .filter(|window| !survivors.contains(&window.pid))
                    .map(|window| window.reopen.clone())
                    .filter(|reopen| !reopen.is_empty())
                    .collect();
                super::reopen_windows(closed);
                pulse.set(false);
                failure_status.set_text(&t(
                    "An export is still running. Let it finish, then try again.",
                ));
                failure_status.set_visible(true);
                retry_to_ready.set(true);
                stack.set_visible_child_name("failed");
            });
        })
    };

    install_now.connect_clicked({
        let request_install = request_install.clone();
        move |_| request_install()
    });
    recording_install.connect_clicked({
        let request_install = request_install.clone();
        move |_| request_install()
    });
    work_install.connect_clicked({
        let save_and_install = save_and_install.clone();
        move |_| save_and_install()
    });
    work_back.connect_clicked({
        let stack = stack.clone();
        let stop_poll = stop_poll.clone();
        move |_| {
            stop_poll();
            stack.set_visible_child_name("ready");
        }
    });

    window.connect_close_request({
        let state = state.clone();
        let update = update.clone();
        let stop_poll = stop_poll.clone();
        let stack = stack.clone();
        let reopen_commands = reopen_commands.clone();
        move |_| {
            state.borrow().cancel.store(true, Ordering::Relaxed);
            stop_poll();
            // The done page's own Close and Restart already took the list; the
            // window's corner close has to reopen the windows as well.
            if stack.visible_child_name().as_deref() == Some("done") {
                super::reopen_windows(std::mem::take(&mut *reopen_commands.borrow_mut()));
            }
            // Dismissing the prompt rests this version for a day; the tray
            // action still opens the window with --force.
            crate::update::snooze_prompt(&update);
            glib::Propagation::Proceed
        }
    });

    match &method {
        InstallMethod::PackageKit(_) | InstallMethod::Pkexec(_) => {
            update_now.set_sensitive(false);
            fetch_assets();
        }
        InstallMethod::Unsupported(message) => {
            available_status.set_text(message);
            available_status.set_visible(true);
            update_now.set_sensitive(false);
        }
        _ => {}
    }

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&pane));
    overlay.add_overlay(&window_controls(&window));
    root.append(&overlay);
    window.set_child(Some(&root));
    window.present();
}

/// Stop the daemon and start the freshly installed binary, closing the window
/// once the new process is away. Polls instead of sleeping so the window stays
/// responsive while the old daemon releases its bus name.
fn restart_application(
    window: &ApplicationWindow,
    method: &InstallMethod,
    reopen_commands: &Rc<RefCell<Vec<Vec<String>>>>,
) {
    let attempts = Rc::new(Cell::new(0u32));
    crate::daemon::stop_daemon_via_dbus();
    let window = window.clone();
    let method = method.clone();
    let reopen_commands = reopen_commands.clone();
    glib::timeout_add_local(install::RESTART_WAIT, move || {
        if crate::daemon::is_daemon_running() && attempts.get() < install::RESTART_ATTEMPTS {
            attempts.set(attempts.get() + 1);
            return glib::ControlFlow::Continue;
        }
        let result = match &method {
            InstallMethod::Flatpak => super::spawn_flatpak_daemon(),
            _ => install::spawn_daemon(&super::binary_path()),
        };
        if let Err(error) = result {
            eprintln!("[update] Could not restart ApexShot: {error}");
        }
        // The editors were closed before the install; bring them back with the
        // new build once the daemon it belongs to is on its way.
        super::reopen_windows(std::mem::take(&mut *reopen_commands.borrow_mut()));
        window.close();
        glib::ControlFlow::Break
    });
}

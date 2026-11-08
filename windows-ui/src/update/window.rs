//! The update window and its four states: available, downloading, ready, and
//! failed. Pages swap without a transition.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label,
    Orientation, Overlay, ProgressBar, Stack, StackTransitionType,
};

use crate::i18n::{t, tfmt};
use crate::settings::ui_support::{window_control_button, WindowControl};
use crate::settings::windowing;

use super::download::{self, Event};
use super::Release;

/// The window box at 100% display scaling.
const BASE_WIDTH: f64 = 560.0;
const BASE_HEIGHT: f64 = 540.0;

/// The centred column that holds every page.
const PANE_WIDTH: f64 = 480.0;

/// Inset of a page from the pane's edges.
const PAGE_INSET: f64 = 36.0;

fn px(scale: f64, value: f64) -> i32 {
    (value * scale).round() as i32
}

/// What the window remembers between states.
#[derive(Default)]
struct DownloadState {
    /// Incremented per attempt so events from a superseded download are
    /// ignored even if its last event arrives after a retry.
    generation: u64,
    cancel: Arc<AtomicBool>,
    downloaded: Option<PathBuf>,
}

pub(super) fn build_window(app: &Application, release: Release) {
    crate::icons::install();
    crate::settings::ui_support::install_settings_css();
    crate::settings::install_window_css();
    let scale = crate::platform::ui_scale();
    crate::onboarding::install_css(scale);
    install_css(scale);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Update"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(px(scale, BASE_WIDTH))
        .default_height(px(scale, BASE_HEIGHT))
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");

    let dark = windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    root.add_css_class("update-root");
    if !dark {
        root.add_css_class("editor-theme-light");
    }
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("update-pane");
    pane.set_size_request(px(scale, PANE_WIDTH), -1);
    pane.set_halign(Align::Center);
    pane.set_hexpand(false);
    windowing::install_window_drag(&pane, &window);

    // No crossfade: the update prompt is a dialog, not a step flow.
    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::None);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);
    pane.append(&stack);

    let state = Rc::new(RefCell::new(DownloadState::default()));

    // ── Update available ─────────────────────────────────────────────────
    let available = page(scale);
    let available_body = body();
    available_body.append(&header(
        scale,
        dark,
        &t("Update available"),
        &tfmt(
            "ApexShot v{version} is ready to install.",
            &[("version", &release.version)],
        ),
    ));

    let installed_card = card(scale);
    let (installed_row, _) = row(scale, &t("Installed version"), env!("CARGO_PKG_VERSION"));
    installed_card.append(&installed_row);
    let (latest_row, _) = row(scale, &t("Latest version"), &release.version);
    installed_card.append(&latest_row);
    if let Some(size) = release.asset_size {
        let (size_row, _) = row(scale, &t("Download size"), &download::format_bytes(size));
        installed_card.append(&size_row);
    }
    available_body.append(&installed_card);

    let link_status = Label::new(None);
    link_status.add_css_class("onboarding-status-error");
    link_status.add_css_class("update-link-status");
    link_status.set_wrap(true);
    link_status.set_max_width_chars(48);
    link_status.set_justify(gtk4::Justification::Center);
    link_status.set_visible(false);
    if let Some(url) = release.release_url.clone() {
        let notes = Button::with_label(&format!("{}  →", t("Release notes")));
        notes.add_css_class("update-link");
        notes.set_halign(Align::Center);
        notes.set_margin_top(px(scale, 12.0));
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
    available_body.append(&link_status);

    let update_now = primary_button(&t("Update now"));
    let later = quiet_button(&t("Later"));
    later.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let available_actions = action_row(scale, Some(&later), &update_now);
    page_body(&available, &available_body, &available_actions);
    stack.add_named(&available, Some("available"));

    // ── Downloading ──────────────────────────────────────────────────────
    let downloading = page(scale);
    let downloading_body = body();
    downloading_body.append(&header(
        scale,
        dark,
        &t("Downloading update"),
        &tfmt(
            "ApexShot v{version} is on its way. Keep this window open.",
            &[("version", &release.version)],
        ),
    ));

    let download_card = card(scale);
    let (download_name_row, download_name) = row(scale, &t("Filename"), &release.asset_name);
    download_card.append(&download_name_row);
    let (download_status_row, download_status) = row(scale, &t("Status"), &t("Starting..."));
    download_card.append(&download_status_row);
    let (download_progress_row, download_progress) = row(
        scale,
        &t("Progress"),
        &download::progress_text(0, release.asset_size),
    );
    download_card.append(&download_progress_row);
    let (download_speed_row, download_speed) = row(scale, &t("Download speed"), "0 B/s");
    download_card.append(&download_speed_row);
    let download_size_text = match release.asset_size {
        Some(total) => download::size_text(0, Some(total)),
        None => download::format_bytes(0),
    };
    let (download_size_row, download_size) = row(scale, &t("File size"), &download_size_text);
    download_card.append(&download_size_row);

    let progress_bar = ProgressBar::new();
    progress_bar.add_css_class("update-progress");
    progress_bar.set_show_text(false);
    progress_bar.set_fraction(0.0);
    progress_bar.set_margin_top(px(scale, 10.0));
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
    let download_actions = action_row(scale, None, &cancel);
    page_body(&downloading, &downloading_body, &download_actions);
    stack.add_named(&downloading, Some("downloading"));

    // ── Update ready ─────────────────────────────────────────────────────
    let ready = page(scale);
    let ready_body = body();
    ready_body.append(&header(
        scale,
        dark,
        &t("Update ready"),
        &tfmt(
            "ApexShot v{version} has finished downloading.",
            &[("version", &release.version)],
        ),
    ));

    let ready_card = card(scale);
    let (ready_name_row, ready_name) = row(scale, &t("Filename"), &release.asset_name);
    ready_card.append(&ready_name_row);
    let (ready_status_row, _) = row(scale, &t("Status"), &t("Downloaded"));
    ready_card.append(&ready_status_row);
    let (ready_size_row, ready_size) = row(scale, &t("File size"), "");
    ready_card.append(&ready_size_row);
    ready_body.append(&ready_card);

    let install_now = primary_button(&t("Install now"));
    let close_ready = quiet_button(&t("Later"));
    close_ready.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let ready_actions = action_row(scale, Some(&close_ready), &install_now);
    page_body(&ready, &ready_body, &ready_actions);
    stack.add_named(&ready, Some("ready"));

    // ── Failed ───────────────────────────────────────────────────────────
    let failed = page(scale);
    let failed_body = body();
    failed_body.append(&header(
        scale,
        dark,
        &t("Update failed"),
        &tfmt(
            "ApexShot v{version} could not be updated.",
            &[("version", &release.version)],
        ),
    ));

    let failed_card = card(scale);
    let (failed_name_row, _) = row(scale, &t("Filename"), &release.asset_name);
    failed_card.append(&failed_name_row);
    let (failed_status_row, _) = row(scale, &t("Status"), &t("Failed"));
    failed_card.append(&failed_status_row);
    failed_body.append(&failed_card);

    let failure_status = Label::new(None);
    failure_status.add_css_class("update-error");
    failure_status.set_wrap(true);
    failure_status.set_max_width_chars(52);
    failure_status.set_justify(gtk4::Justification::Center);
    failure_status.set_margin_top(px(scale, 14.0));
    failure_status.set_visible(false);
    failed_body.append(&failure_status);

    let retry = primary_button(&t("Try again"));
    let close_failed = quiet_button(&t("Close"));
    close_failed.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    let failed_actions = action_row(scale, Some(&close_failed), &retry);
    page_body(&failed, &failed_body, &failed_actions);
    stack.add_named(&failed, Some("failed"));

    // ── Wiring ───────────────────────────────────────────────────────────
    let begin_download: Rc<dyn Fn()> = {
        let stack = stack.clone();
        let state = state.clone();
        let release = release.clone();
        let download_name = download_name.clone();
        let download_status = download_status.clone();
        let download_progress = download_progress.clone();
        let download_speed = download_speed.clone();
        let download_size = download_size.clone();
        let progress_bar = progress_bar.clone();
        let failure_status = failure_status.clone();
        let ready_name = ready_name.clone();
        let ready_size = ready_size.clone();

        Rc::new(move || {
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

            download_name.set_text(&release.asset_name);
            download_status.set_text(&t("Starting..."));
            download_progress.set_text(&download::progress_text(0, release.asset_size));
            download_speed.set_text(&download::format_rate(0.0));
            download_size.set_text(&match release.asset_size {
                Some(total) => download::size_text(0, Some(total)),
                None => download::format_bytes(0),
            });
            progress_bar.set_fraction(0.0);
            stack.set_visible_child_name("downloading");

            match download::start(&release.asset_url, &release.asset_name, cancel) {
                Ok((events, destination)) => {
                    download_name.set_text(&file_label(&destination));
                    let stack = stack.clone();
                    let state = state.clone();
                    let download_status = download_status.clone();
                    let download_progress = download_progress.clone();
                    let download_speed = download_speed.clone();
                    let download_size = download_size.clone();
                    let progress_bar = progress_bar.clone();
                    let failure_status = failure_status.clone();
                    let ready_name = ready_name.clone();
                    let ready_size = ready_size.clone();
                    glib::spawn_future_local(async move {
                        while let Ok(event) = events.recv().await {
                            if state.borrow().generation != generation {
                                break;
                            }
                            match event {
                                Event::Progress {
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
                                Event::Finished(path) => {
                                    state.borrow_mut().downloaded = Some(path.clone());
                                    ready_name.set_text(&file_label(&path));
                                    ready_size.set_text(&download::format_bytes(file_size(&path)));
                                    stack.set_visible_child_name("ready");
                                }
                                Event::Failed(message) => {
                                    failure_status.set_text(&message);
                                    failure_status.set_visible(true);
                                    stack.set_visible_child_name("failed");
                                }
                                Event::Cancelled => {}
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

    update_now.connect_clicked({
        let begin_download = begin_download.clone();
        move |_| begin_download()
    });
    retry.connect_clicked({
        let begin_download = begin_download.clone();
        move |_| begin_download()
    });

    install_now.connect_clicked({
        let state = state.clone();
        let window = window.clone();
        let stack = stack.clone();
        let failure_status = failure_status.clone();
        move |_| {
            let Some(path) = state.borrow().downloaded.clone() else {
                return;
            };
            match launch_installer(&path) {
                Ok(()) => window.close(),
                Err(error) => {
                    failure_status.set_text(&tfmt(
                        "Could not start the installer: {error}",
                        &[("error", &error.to_string())],
                    ));
                    failure_status.set_visible(true);
                    stack.set_visible_child_name("failed");
                }
            }
        }
    });

    window.connect_close_request({
        let state = state.clone();
        let release = release.clone();
        move |_| {
            state.borrow().cancel.store(true, Ordering::Relaxed);
            // Dismissing the prompt rests this version for a day; the next
            // check after that offers it again if it is still installed.
            super::check::snooze(&release.version);
            glib::Propagation::Proceed
        }
    });

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&pane));
    overlay.add_overlay(&window_controls(&window, scale));
    root.append(&overlay);
    window.set_child(Some(&root));
    window.present();
}

/// The brand mark above a centred headline, shared by every state.
fn header(scale: f64, dark: bool, title: &str, copy: &str) -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, 0);
    let mark = crate::onboarding::showcase::brand_mark(scale, dark);
    mark.set_margin_bottom(px(scale, 18.0));
    block.append(&mark);
    block.append(&crate::onboarding::ui::heading(scale, title, copy));
    block
}

fn page(scale: f64) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("update-page");
    page.set_halign(Align::Fill);
    // The actions sit on the page's bottom edge instead of floating under the
    // reading column, which leaves the window's lower inset to the row itself.
    page.set_valign(Align::Fill);
    page.set_vexpand(true);
    page.set_margin_start(px(scale, PAGE_INSET));
    page.set_margin_end(px(scale, PAGE_INSET));
    page.set_margin_top(px(scale, 28.0));
    page.set_margin_bottom(px(scale, 28.0));
    page
}

/// The reading column above the actions.
fn body() -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, 0);
    body.set_halign(Align::Fill);
    body
}

/// Stack the reading column and the bottom action row, centring the column in
/// the space the actions leave.
fn page_body(page: &GtkBox, body: &GtkBox, actions: &GtkBox) {
    let top = GtkBox::new(Orientation::Vertical, 0);
    top.set_vexpand(true);
    page.append(&top);
    page.append(body);
    let middle = GtkBox::new(Orientation::Vertical, 0);
    middle.set_vexpand(true);
    page.append(&middle);
    page.append(actions);
}

/// One action at each edge of the row: the quiet choice on the left, the
/// primary action on the right. A lone action sits in the middle.
fn action_row(scale: f64, left: Option<&Button>, right: &Button) -> GtkBox {
    let actions = GtkBox::new(Orientation::Horizontal, px(scale, 12.0));
    actions.add_css_class("onboarding-actions");
    actions.add_css_class("update-actions");
    actions.set_halign(Align::Fill);
    actions.set_margin_top(px(scale, 28.0));

    let spacer = || {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        spacer
    };

    match left {
        Some(left) => {
            left.set_halign(Align::Start);
            actions.append(left);
            actions.append(&spacer());
            right.set_halign(Align::End);
            actions.append(right);
        }
        None => {
            // With nothing to balance the row against, a single action reads
            // better centred than pushed into the primary's corner.
            actions.append(&spacer());
            right.set_halign(Align::Center);
            actions.append(right);
            actions.append(&spacer());
        }
    }
    actions
}

fn card(scale: f64) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("update-card");
    card.set_margin_top(px(scale, 22.0));
    card.set_halign(Align::Fill);
    card.set_hexpand(true);
    card
}

fn row(scale: f64, key: &str, value: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, px(scale, 16.0));
    row.add_css_class("update-row");
    row.set_hexpand(true);

    let key_label = Label::new(Some(key));
    key_label.add_css_class("update-row-key");
    key_label.set_halign(Align::Start);
    key_label.set_valign(Align::Center);
    row.append(&key_label);

    let value_label = Label::new(Some(value));
    value_label.add_css_class("update-row-value");
    value_label.set_halign(Align::End);
    value_label.set_xalign(1.0);
    value_label.set_hexpand(true);
    value_label.set_valign(Align::Center);
    value_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    row.append(&value_label);

    (row, value_label)
}

fn primary_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("settings-primary-btn");
    button.set_halign(Align::Fill);
    button
}

fn quiet_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("onboarding-text-button");
    button.set_halign(Align::Center);
    button
}

fn window_controls(window: &ApplicationWindow, scale: f64) -> GtkBox {
    let minimize = window_control_button(WindowControl::Minimize, &t("Minimize"), scale);
    minimize.connect_clicked({
        let window = window.clone();
        move |_| window.minimize()
    });

    let close = window_control_button(WindowControl::Close, &t("Close"), scale);
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    let controls = GtkBox::new(Orientation::Horizontal, 0);
    controls.add_css_class("settings-window-controls");
    controls.add_css_class("onboarding-toolbar");
    controls.set_halign(Align::End);
    controls.set_valign(Align::Start);
    controls.append(&minimize);
    controls.append(&close);
    controls
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn launch_installer(path: &Path) -> std::io::Result<()> {
    std::process::Command::new(path).spawn().map(|_| ())
}

/// Load the update-specific surfaces after the onboarding sheet, at the same
/// display scale.
fn install_css(scale: f64) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        include_str!("update.css"),
        scale,
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
}

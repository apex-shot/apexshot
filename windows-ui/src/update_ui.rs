//! Compact, non-blocking update card shared by the Windows windows.
//!
//! The card matches the Linux prompt: it appears once a cached or background
//! check finds a newer release, sits bottom-right inside a window's overlay,
//! and hands the release it already resolved to the standalone update window.
//! The window under it is never blocked, and no page is replaced.

use std::sync::mpsc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, CssProvider, Label, Orientation, Overlay};

use crate::i18n::{t, tfmt};
use crate::settings::windowing;
use crate::update::Release;

const UPDATE_CSS: &str = r#"
    .apexshot-update-surface {
        background: #0f0f0f;
        color: #f1f1f3;
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 14px;
        box-shadow: 0 18px 50px rgba(0, 0, 0, 0.42);
        padding: 16px;
        font-family: 'Inter', 'Noto Sans', system-ui, sans-serif;
    }
    .apexshot-update-title { font-size: 14px; font-weight: 700; }
    .apexshot-update-copy { font-size: 12px; color: rgba(241, 241, 243, 0.64); }
    button.apexshot-update-close {
        min-width: 28px; min-height: 28px; padding: 0; border-radius: 999px;
        border: none; background: transparent; color: rgba(241, 241, 243, 0.66);
    }
    button.apexshot-update-close:hover { background: rgba(255, 255, 255, 0.08); color: #fff; }
    button.apexshot-update-primary {
        min-height: 32px; padding: 4px 14px; border: none; border-radius: 7px;
        background: #ff670d; color: white; font-size: 12px; font-weight: 700;
    }
    button.apexshot-update-primary:hover { background: #ff7c2f; }
    button.apexshot-update-link {
        padding: 0; min-height: 0; border: none; background: transparent;
        color: #ff670d; font-size: 12px;
    }
    button.apexshot-update-link:hover { color: #ff7c2f; text-decoration: underline; }

    /* The card carries its own theme class: it is an overlay child of the
       window, not a descendant of the editor's themed root. */
    .apexshot-update-surface.editor-theme-light {
        background: #ffffff;
        color: #1d2129;
        border: 1px solid alpha(#111827, 0.10);
        box-shadow: 0 18px 50px alpha(#111827, 0.18);
    }
    .apexshot-update-surface.editor-theme-light .apexshot-update-title { color: #1d2129; }
    .apexshot-update-surface.editor-theme-light .apexshot-update-copy {
        color: alpha(#1d2129, 0.64);
    }
    .apexshot-update-surface.editor-theme-light button.apexshot-update-close {
        color: alpha(#1d2129, 0.66);
    }
    .apexshot-update-surface.editor-theme-light button.apexshot-update-close:hover {
        background: alpha(#111827, 0.08); color: #1d2129;
    }
    .apexshot-update-surface.editor-theme-light button.apexshot-update-link {
        color: #cf530b;
    }
    .apexshot-update-surface.editor-theme-light button.apexshot-update-link:hover {
        color: #b34709; text-decoration: underline;
    }
"#;

fn px(scale: f64, value: f64) -> i32 {
    (value * scale).round() as i32
}

fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        UPDATE_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 7,
    );
}

/// Show the card once a cached or background check finds a newer release.
/// Using an Overlay pins it to the bottom right of the window it sits in.
pub fn present_if_needed(overlay: &Overlay) {
    let overlay = overlay.clone();
    if let Some(release) = crate::update::cached_release() {
        present_card(&overlay, release);
        return;
    }

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(crate::update::check());
    });
    glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
        Ok(Some(release)) => {
            present_card(&overlay, release);
            glib::ControlFlow::Break
        }
        Ok(None) | Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
    });
}

fn present_card(overlay: &Overlay, release: Release) {
    if crate::update::prompt_is_snoozed(&release.version) {
        return;
    }
    install_css();

    let scale = crate::platform::ui_scale();
    let dark = windowing::prefers_dark_glass_theme();

    let surface = GtkBox::new(Orientation::Vertical, 12);
    surface.add_css_class("apexshot-update-surface");
    if !dark {
        surface.add_css_class("editor-theme-light");
    }
    surface.set_width_request(px(scale, 370.0));
    surface.set_halign(Align::End);
    surface.set_valign(Align::End);
    surface.set_margin_end(px(scale, 16.0));
    surface.set_margin_bottom(px(scale, 16.0));

    let heading = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(&t("Update available")));
    title.add_css_class("apexshot-update-title");
    title.set_halign(Align::Start);
    title.set_hexpand(true);
    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("apexshot-update-close");
    close.set_tooltip_text(Some(&t("Close")));
    heading.append(&title);
    heading.append(&close);

    let copy = Label::new(Some(&tfmt(
        "ApexShot v{version} is ready. Your current work will not be interrupted.",
        &[("version", &release.version)],
    )));
    copy.add_css_class("apexshot-update-copy");
    copy.set_wrap(true);
    copy.set_halign(Align::Start);
    copy.set_xalign(0.0);

    let link_status = Label::new(None);
    link_status.add_css_class("onboarding-status-error");
    link_status.set_wrap(true);
    link_status.set_halign(Align::Start);
    link_status.set_xalign(0.0);
    link_status.set_visible(false);

    let release_notes = Button::with_label(&format!("{}  →", t("Release notes")));
    release_notes.add_css_class("apexshot-update-link");
    release_notes.set_halign(Align::Start);
    if let Some(label) = release_notes
        .child()
        .and_then(|child| child.downcast::<Label>().ok())
    {
        label.set_markup(&format!(
            "<u>{}</u>  →",
            glib::markup_escape_text(&t("Release notes"))
        ));
    }
    if let Some(url) = release.release_url.clone() {
        let status = link_status.clone();
        release_notes.connect_clicked(move |_| crate::onboarding::ui::open_link(&url, &status));
    } else {
        release_notes.set_visible(false);
    }

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::Fill);
    actions.set_hexpand(true);
    let update_btn = Button::with_label(&t("Update ApexShot"));
    update_btn.add_css_class("apexshot-update-primary");
    let action_spacer = GtkBox::new(Orientation::Horizontal, 0);
    action_spacer.set_hexpand(true);
    actions.append(&release_notes);
    actions.append(&action_spacer);
    actions.append(&update_btn);

    surface.append(&heading);
    surface.append(&copy);
    surface.append(&link_status);
    surface.append(&actions);

    let close_surface = surface.clone();
    let close_overlay = overlay.clone();
    let close_version = release.version.clone();
    close.connect_clicked(move |_| {
        crate::update::snooze(&close_version);
        close_overlay.remove_overlay(&close_surface);
    });

    let update_release = release.clone();
    let update_surface = surface.clone();
    let update_overlay = overlay.clone();
    update_btn.connect_clicked(move |_| {
        crate::update::snooze(&update_release.version);
        open_update_window(&update_release);
        update_overlay.remove_overlay(&update_surface);
    });

    overlay.add_overlay(&surface);
    overlay.set_clip_overlay(&surface, false);
}

/// Hand the already-resolved release to the standalone update window.
fn open_update_window(release: &Release) {
    let Some(executable) = crate::update::executable_path() else {
        eprintln!("[update] Could not find the update window binary beside this one.");
        return;
    };
    let mut command = crate::utils::hidden_command(&executable);
    command
        .args(release.command_line())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            // The card lives in long-lived windows; reap the child so no
            // zombie is left behind when the update window closes.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => eprintln!("[update] Could not open the update window: {error}"),
    }
}

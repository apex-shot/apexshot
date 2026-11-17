use gtk4::{prelude::*, Align, Button, Label};
use std::time::Duration;

use super::ui::{copy_with_feedback, feature_card_list, heading, open_link};
use crate::capture::editor::window::icon_names::custom;
use crate::gnome_extension::{self, RELEASES_URL as GNOME_EXTENSION_RELEASES_URL};
use crate::i18n::{self, t};

pub const CHROME_EXTENSION_URL: &str =
    "https://chromewebstore.google.com/detail/apexshot/kaejmfabajnakpodjffipckmcpfpdenj";
fn open_url(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        let _ = crate::utils::open::open_url(&url);
    });
}

fn is_extension_enabled() -> bool {
    crate::gnome_shell::is_shell_overlay_service_available()
}

fn install_extension(button: gtk4::glib::SendWeakRef<Button>, already_installed: bool) {
    std::thread::spawn(move || {
        let archive = std::env::temp_dir().join(gnome_extension::ARCHIVE_NAME);
        let mut installed = already_installed;
        if !installed {
            let downloaded = gnome_extension::latest_download_url()
                .is_some_and(|url| gnome_extension::download_archive(&url, &archive).is_ok());
            installed = downloaded && gnome_extension::install_archive(&archive);
        }
        let _ = std::fs::remove_file(archive);

        let mut is_enabled = false;
        if installed && gnome_extension::enable_extension() {
            for _ in 0..8 {
                if is_extension_enabled() {
                    is_enabled = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        gtk4::glib::MainContext::default().invoke(move || {
            if let Some(button) = button.upgrade() {
                let label = if is_enabled {
                    t("Extension Enabled ✓")
                } else if installed {
                    t("Extension is installed but inactive — enable it in GNOME Extensions")
                } else {
                    t("Extension installation failed — try again")
                };
                button.set_label(&label);
                button.set_sensitive(!is_enabled);
            }
        });
    });
}

pub fn build_gnome(content: &gtk4::Box) {
    // Check if running GNOME
    if !gnome_extension::is_gnome_session() {
        let title = Label::new(None);
        title.set_markup(&i18n::markup_title("GNOME Shell Extension Required"));
        title.set_halign(Align::Center);
        title.set_margin_bottom(8);
        content.append(&title);

        let desc = Label::new(Some(&t(
            "ApexShot requires GNOME Shell. Please install on a GNOME desktop to continue.",
        )));
        desc.set_halign(Align::Center);
        desc.set_wrap(true);
        desc.set_width_request(500);
        desc.add_css_class("settings-sub-option");
        content.append(&desc);

        let exit_btn = Button::with_label(&t("Exit"));
        exit_btn.add_css_class("settings-primary-btn");
        exit_btn.set_halign(Align::Center);
        exit_btn.set_margin_top(32);
        exit_btn.connect_clicked(|_| {
            // Close the onboarding window
            // This will be handled by the parent window
        });
        content.append(&exit_btn);
        return;
    }

    // Check for old extension
    let has_old_extension = gnome_extension::is_old_installed();
    if has_old_extension {
        let title = Label::new(None);
        title.set_markup(&i18n::markup_title("Update GNOME Extension"));
        title.set_halign(Align::Center);
        title.set_margin_bottom(8);
        content.append(&title);

        let desc = Label::new(Some(&t(
            "An old version of the ApexShot extension is installed. It needs to be removed before installing the new version.",
        )));
        desc.set_halign(Align::Center);
        desc.set_wrap(true);
        desc.set_width_request(500);
        desc.add_css_class("settings-sub-option");
        content.append(&desc);

        let remove_btn = Button::with_label(&t("Remove Old Extension"));
        remove_btn.add_css_class("settings-primary-btn");
        remove_btn.set_halign(Align::Center);
        remove_btn.set_margin_top(32);
        remove_btn.connect_clicked(|_| {
            gnome_extension::remove_old_extension();
        });
        content.append(&remove_btn);
        return;
    }

    // Title
    let title = Label::new(None);
    title.set_markup(&i18n::markup_title("GNOME Shell Extension"));
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description
    let desc = Label::new(Some(&t(
        "On GNOME Wayland, the shell extension unlocks the polished capture experience:",
    )));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_justify(gtk4::Justification::Center);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    let preview_title = t("Floating preview windows");
    let preview_body = t("Always-on-top previews that stay out of your way");
    let overlay_title = t("Quick access overlay");
    let overlay_body = t("Post-capture actions without hunting through menus");
    let rec_title = t("Recording status indicator");
    let rec_body = t("Shell-managed controls while a recording is live");
    let features = feature_card_list(&[
        (
            custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            preview_title.as_str(),
            preview_body.as_str(),
        ),
        (
            custom::SELECT_MODE_SYMBOLIC,
            overlay_title.as_str(),
            overlay_body.as_str(),
        ),
        (
            custom::RECORD_SCREEN_SYMBOLIC,
            rec_title.as_str(),
            rec_body.as_str(),
        ),
    ]);
    features.set_margin_top(18);
    content.append(&features);

    // Check if extension is already installed
    let is_enabled = is_extension_enabled();
    let is_installed = gnome_extension::is_installed();

    // Install button
    let installed_label = t("Extension Enabled ✓");
    let install_label = t("Install GNOME Extension");
    let enable_label = t("Enable GNOME Extension");
    let download_label = t("Download GNOME Extension");
    let install_btn = Button::with_label(if is_enabled {
        installed_label.as_str()
    } else if crate::app_identity::portal_only() {
        download_label.as_str()
    } else if is_installed {
        enable_label.as_str()
    } else {
        install_label.as_str()
    });
    install_btn.add_css_class("settings-primary-btn");
    install_btn.set_halign(Align::Center);
    install_btn.set_margin_top(32);

    if !is_enabled && crate::app_identity::portal_only() {
        install_btn.connect_clicked(|_| {
            open_url(GNOME_EXTENSION_RELEASES_URL);
        });
    } else if !is_enabled {
        let install_btn_weak = gtk4::glib::SendWeakRef::from(install_btn.downgrade());

        install_btn.connect_clicked(move |btn| {
            btn.set_label(&t(if is_installed {
                "Enabling..."
            } else {
                "Installing..."
            }));
            btn.set_sensitive(false);
            install_extension(install_btn_weak.clone(), is_installed);
        });
    } else {
        install_btn.set_sensitive(false);
    }
    content.append(&install_btn);

    // Note about logout
    let note_text = if crate::app_identity::portal_only() {
        t("This Flatpak cannot install extensions into your host desktop. Open the release page and download the GNOME extension archive, then install and enable it on the host in GNOME Extensions or run `gnome-extensions install --force ~/Downloads/apexshot-gnome-integration.zip` followed by `gnome-extensions enable apexshot-gnome-integration@apexshot.github.io` in a host terminal. ApexShot will detect it when the extension is enabled.")
    } else if is_installed && !is_enabled {
        t("The GNOME extension is installed but not active. Use Enable GNOME Extension or enable it in GNOME Extensions.")
    } else {
        t("You may need to log out and back in for the extension to appear in GNOME.")
    };
    let note = Label::new(Some(&note_text));
    note.set_halign(Align::Center);
    note.set_wrap(true);
    note.set_width_request(500);
    note.set_margin_top(16);
    note.add_css_class("settings-sub-option");
    content.append(&note);

    // Manual download link
    let manual_link = Button::with_label(&t("Download extension archive"));
    manual_link.add_css_class("secondary-settings-button");
    manual_link.set_halign(Align::Center);
    manual_link.set_margin_top(16);
    manual_link.connect_clicked(|_| {
        open_url(GNOME_EXTENSION_RELEASES_URL);
    });
    content.append(&manual_link);
}

pub(super) fn build_chrome(body: &gtk4::Box, actions: &gtk4::Box, nav: &super::Nav) {
    body.append(&heading(
        &t("Browser Extension"),
        &t("Optional Chrome/Chromium add-on for full-page web captures that open straight in ApexShot."),
    ));

    let scroll_title = t("Full-page scroll capture");
    let scroll_body = t("Stitch long pages that a normal screenshot can't fit");
    let send_title = t("Sends directly to ApexShot");
    let send_body = t("Opens in the editor so you can annotate and share immediately");
    let desktop_title = t("Works with your desktop app");
    let desktop_body = if crate::app_identity::portal_only() {
        t("Host-installed browsers need scripts/install-flatpak-browser-host.py run on the host. This bridge does not support browsers installed as Flatpaks.")
    } else {
        t("Native messaging keeps the browser and ApexShot in sync")
    };
    let features = feature_card_list(&[
        (
            custom::SCREENSHOOTER_SYMBOLIC,
            scroll_title.as_str(),
            scroll_body.as_str(),
        ),
        (
            custom::ARROW2_TOP_RIGHT_SYMBOLIC,
            send_title.as_str(),
            send_body.as_str(),
        ),
        (
            custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            desktop_title.as_str(),
            desktop_body.as_str(),
        ),
    ]);
    features.set_margin_top(super::SECTION_GAP);
    body.append(&features);

    // Shown only when the browser could not be opened.
    let browser_status = Label::new(None);
    browser_status.add_css_class("settings-sub-option");
    browser_status.set_halign(Align::Center);
    browser_status.set_xalign(0.5);
    browser_status.set_justify(gtk4::Justification::Center);
    browser_status.set_wrap(true);
    browser_status.set_max_width_chars(38);
    browser_status.set_margin_top(10);
    browser_status.set_visible(false);
    body.append(&browser_status);

    let install_btn = Button::with_label(&t("Get Chrome Extension"));
    install_btn.add_css_class("settings-primary-btn");
    install_btn.connect_clicked({
        let browser_status = browser_status.clone();
        move |_| {
            open_link(CHROME_EXTENSION_URL, &browser_status);
        }
    });
    actions.append(&install_btn);

    let copy_link_btn = Button::with_label(&t("Copy link"));
    copy_link_btn.add_css_class("secondary-settings-button");
    copy_link_btn.connect_clicked(|button| {
        copy_with_feedback(button, CHROME_EXTENSION_URL, &t("Copy link"));
    });
    actions.append(&copy_link_btn);

    let skip_btn = Button::with_label(&t("Skip for now"));
    skip_btn.add_css_class("onboarding-text-button");
    let advance = nav.advance.clone();
    skip_btn.connect_clicked(move |_| advance());
    actions.append(&skip_btn);
}

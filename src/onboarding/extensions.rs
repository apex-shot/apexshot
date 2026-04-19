use gtk4::{prelude::*, Align, Button, Label, show_uri};
use std::process::Command;

// TODO: Update these URLs when extensions are published
const GNOME_EXTENSION_URL: &str = "https://github.com/apex-shot/apexshot/releases/tag/gnome-extension-v2";
const CHROME_EXTENSION_URL: &str = "https://chromewebstore.google.com/detail/apexshot/XXXXX";
const EXTENSION_UUID: &str = "apexshot-gnome-integration@apexshot.github.io";
const OLD_EXTENSION_UUID: &str = "apexshot-preview-helper@apexshot.github.io";
const EXTENSION_ZIP_URL: &str = "https://github.com/apex-shot/apexshot/releases/download/gnome-extension-v2/apexshot-gnome-integration.zip";

fn open_url(url: &str) {
    let _ = show_uri(None::<&gtk4::Window>, url, 0);
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("gnome")
}

fn is_extension_installed() -> bool {
    Command::new("gnome-extensions")
        .args(["list"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .contains(EXTENSION_UUID)
        })
        .unwrap_or(false)
}

fn is_old_extension_installed() -> bool {
    Command::new("gnome-extensions")
        .args(["list"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .contains(OLD_EXTENSION_UUID)
        })
        .unwrap_or(false)
}

fn remove_old_extension() {
    let _ = Command::new("gnome-extensions")
        .args(["disable", OLD_EXTENSION_UUID])
        .output();

    let home = std::env::var("HOME").unwrap_or_default();
    let old_ext_dir = format!(
        "{}/.local/share/gnome-shell/extensions/{}",
        home, OLD_EXTENSION_UUID
    );
    let _ = Command::new("rm")
        .args(["-rf", &old_ext_dir])
        .output();
}

fn install_extension() {
    // Download and install the extension
    let home = std::env::var("HOME").unwrap_or_default();
    let ext_dir = format!(
        "{}/.local/share/gnome-shell/extensions/{}",
        home, EXTENSION_UUID
    );

    // Create directory
    let _ = Command::new("mkdir")
        .args(["-p", &ext_dir])
        .output();

    // Download zip
    let _ = Command::new("wget")
        .args(["-O", "/tmp/apexshot-extension.zip", EXTENSION_ZIP_URL])
        .output();

    // Extract to extension directory
    let _ = Command::new("unzip")
        .args(["-o", "/tmp/apexshot-extension.zip", "-d", &ext_dir])
        .output();

    // Enable extension
    let _ = Command::new("gnome-extensions")
        .args(["enable", EXTENSION_UUID])
        .output();

    // Clean up
    let _ = Command::new("rm")
        .args(["-f", "/tmp/apexshot-extension.zip"])
        .output();
}

pub fn build_gnome(content: &gtk4::Box) {
    // Check if running GNOME
    if !is_gnome() {
        let title = Label::new(None);
        title.set_markup("<span size='x-large' weight='bold'>GNOME Shell Extension Required</span>");
        title.set_halign(Align::Center);
        title.set_margin_bottom(8);
        content.append(&title);

        let desc = Label::new(Some(
            "ApexShot requires GNOME Shell. Please install on a GNOME desktop to continue.",
        ));
        desc.set_halign(Align::Center);
        desc.set_wrap(true);
        desc.set_width_request(500);
        desc.add_css_class("settings-sub-option");
        content.append(&desc);

        let exit_btn = Button::with_label("Exit");
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
    let has_old_extension = is_old_extension_installed();
    if has_old_extension {
        let title = Label::new(None);
        title.set_markup("<span size='x-large' weight='bold'>Update GNOME Extension</span>");
        title.set_halign(Align::Center);
        title.set_margin_bottom(8);
        content.append(&title);

        let desc = Label::new(Some(
            "An old version of the ApexShot extension is installed. It needs to be removed before installing the new version.",
        ));
        desc.set_halign(Align::Center);
        desc.set_wrap(true);
        desc.set_width_request(500);
        desc.add_css_class("settings-sub-option");
        content.append(&desc);

        let remove_btn = Button::with_label("Remove Old Extension");
        remove_btn.add_css_class("settings-primary-btn");
        remove_btn.set_halign(Align::Center);
        remove_btn.set_margin_top(32);
        remove_btn.connect_clicked(|_| {
            remove_old_extension();
        });
        content.append(&remove_btn);
        return;
    }

    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>GNOME Shell Extension</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description
    let desc = Label::new(Some(
        "ApexShot requires the GNOME Shell extension for full functionality:",
    ));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    // Features
    let features_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    features_box.set_margin_top(32);
    features_box.set_halign(Align::Center);

    let features = [
        "Floating preview windows",
        "Quick access overlay",
        "Recording status indicator",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        label.set_margin_start(40);
        label.set_margin_end(40);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Check if extension is already installed
    let is_installed = is_extension_installed();

    // Install button
    let install_btn = Button::with_label(if is_installed {
        "Extension Installed ✓"
    } else {
        "Install GNOME Extension"
    });
    install_btn.add_css_class("settings-primary-btn");
    install_btn.set_halign(Align::Center);
    install_btn.set_margin_top(32);

    if !is_installed {
        install_btn.connect_clicked(|_| {
            install_extension();
        });
    } else {
        install_btn.set_sensitive(false);
    }
    content.append(&install_btn);

    // Manual download link
    let manual_link = Button::with_label("Or download manually from GitHub");
    manual_link.add_css_class("secondary-settings-button");
    manual_link.set_halign(Align::Center);
    manual_link.set_margin_top(8);
    manual_link.connect_clicked(|_| {
        open_url(GNOME_EXTENSION_URL);
    });
    content.append(&manual_link);
}

pub fn build_chrome(content: &gtk4::Box) {
    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>Browser Extension</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description
    let desc = Label::new(Some(
        "Capture full-page screenshots from any website with our Chrome/Chromium extension:",
    ));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    // Features
    let features_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    features_box.set_margin_top(32);
    features_box.set_halign(Align::Center);

    let features = [
        "Full-page scroll capture",
        "Sends directly to ApexShot",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        label.set_margin_start(40);
        label.set_margin_end(40);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Install button
    let install_btn = Button::with_label("Get Chrome Extension");
    install_btn.add_css_class("settings-primary-btn");
    install_btn.set_halign(Align::Center);
    install_btn.set_margin_top(32);
    install_btn.connect_clicked(|_| {
        open_url(CHROME_EXTENSION_URL);
    });
    content.append(&install_btn);
}

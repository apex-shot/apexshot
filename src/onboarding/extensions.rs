use gtk4::{prelude::*, Align, Button, Label};
use std::process::Command;

// TODO: Update these URLs when extensions are published
const GNOME_EXTENSION_URL: &str =
    "https://github.com/apex-shot/apexshot/releases/tag/gnome-extension-v2";
pub const CHROME_EXTENSION_URL: &str =
    "https://chromewebstore.google.com/detail/apexshot/kaejmfabajnakpodjffipckmcpfpdenj";
const EXTENSION_UUID: &str = "apexshot-gnome-integration@apexshot.github.io";
const OLD_EXTENSION_UUID: &str = "apexshot-preview-helper@apexshot.github.io";
fn open_url(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(&url).spawn();
    });
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
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(EXTENSION_UUID))
        .unwrap_or(false)
}

fn is_old_extension_installed() -> bool {
    Command::new("gnome-extensions")
        .args(["list"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(OLD_EXTENSION_UUID))
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
    let _ = Command::new("rm").args(["-rf", &old_ext_dir]).output();
}

fn install_extension(button: gtk4::glib::SendWeakRef<Button>) {
    std::thread::spawn(move || {
        // Dynamically find the latest release that actually contains the zip file
        // This handles cases where recent releases (e.g., .deb only) don't have the zip
        let get_url_cmd = r#"curl -s https://api.github.com/repos/apex-shot/apexshot/releases | grep -o '"browser_download_url": *"[^"]*apexshot-gnome-integration.zip"' | head -n 1 | cut -d '"' -f 4"#;

        if let Ok(output) = Command::new("sh").arg("-c").arg(get_url_cmd).output() {
            let zip_url = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if !zip_url.is_empty() {
                // Download zip
                let _ = Command::new("wget")
                    .args(["-O", "/tmp/apexshot-extension.zip", &zip_url])
                    .output();

                // Install using the official gnome-extensions tool
                // This ensures GNOME Shell registers it immediately without a Wayland restart
                let _ = Command::new("gnome-extensions")
                    .args(["install", "--force", "/tmp/apexshot-extension.zip"])
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
        }

        gtk4::glib::MainContext::default().invoke(move || {
            if let Some(button) = button.upgrade() {
                button.set_label("Extension Installed ✓");
                button.set_sensitive(false);
            }
        });
    });
}

pub fn build_gnome(content: &gtk4::Box) {
    // Check if running GNOME
    if !is_gnome() {
        let title = Label::new(None);
        title
            .set_markup("<span size='x-large' weight='bold'>GNOME Shell Extension Required</span>");
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
        let install_btn_weak = gtk4::glib::SendWeakRef::from(install_btn.downgrade());

        install_btn.connect_clicked(move |btn| {
            btn.set_label("Installing...");
            btn.set_sensitive(false);
            install_extension(install_btn_weak.clone());
        });
    } else {
        install_btn.set_sensitive(false);
    }
    content.append(&install_btn);

    // Note about logout
    let note = Label::new(Some(
        "Note: You may need to log out and back in for the extension to appear in GNOME.",
    ));
    note.set_halign(Align::Center);
    note.set_wrap(true);
    note.set_width_request(500);
    note.set_margin_top(16);
    note.add_css_class("settings-sub-option");
    content.append(&note);

    // Manual download link
    let manual_link = Button::with_label("Or download manually from GitHub");
    manual_link.add_css_class("secondary-settings-button");
    manual_link.set_halign(Align::Center);
    manual_link.set_margin_top(16);
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

    let features = ["Full-page scroll capture", "Sends directly to ApexShot"];

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

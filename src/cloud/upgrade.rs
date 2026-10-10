use gtk4::{gio, prelude::*, Window};

use crate::i18n::t;

pub const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";

pub fn show_prompt(window: &impl IsA<Window>, message: &str) {
    let dialog = gtk4::AlertDialog::builder()
        .modal(true)
        .message(t("Your capture is saved"))
        .detail(message)
        .buttons([t("Keep working"), t("Go Pro")])
        .cancel_button(0)
        .default_button(0)
        .build();
    dialog.choose(Some(window), None::<&gio::Cancellable>, |response| {
        if matches!(response, Ok(1)) {
            std::thread::spawn(|| {
                if let Err(error) = crate::utils::open::open_url(BILLING_URL) {
                    crate::utils::notify::desktop_notification(
                        &t("Could not open billing"),
                        &error,
                    );
                }
            });
        }
    });
}

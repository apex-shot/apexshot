//! Layout pieces shared by the update window's pages.
//!
//! Everything here is authored at 100% scale, matching the onboarding sheet:
//! the brand mark over a centred headline, a quiet fact card, and one action
//! row whose quiet choice sits on the left.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label, Orientation,
    ProgressBar, Stack,
};

use crate::i18n::{t, tfmt};

use super::install::{self, InstallEvent, InstallMethod, InstallRequest};

/// Inset of a page from the pane's edges, at 100% scale.
const PAGE_INSET: i32 = 36;

/// The widgets the install pages share, so both the "Install now" button and
/// the Flatpak shortcut can start the same flow.
pub(super) struct InstallUi {
    pub stack: Stack,
    pub status: Label,
    pub progress: ProgressBar,
    pub pulse: Rc<Cell<bool>>,
    pub failure: Label,
    pub done_copy: Label,
    pub done_note: Label,
    pub ready_note: Label,
    pub restartable: bool,
    pub version: String,
    /// Set when the install closed open ApexShot windows, so the done page can
    /// tell the user how to get them back.
    pub closed_windows: Rc<Cell<bool>>,
    /// The public commands that reopen the windows this install closed. The
    /// install pages reopen them when the install does not go through; the
    /// done page's own actions take the list after a successful install.
    pub reopen: Rc<RefCell<Vec<Vec<String>>>>,
}

impl InstallUi {
    pub fn start(
        &self,
        method: InstallMethod,
        package: Option<PathBuf>,
        extension_zip: Option<PathBuf>,
    ) {
        self.failure.set_visible(false);
        self.ready_note.set_visible(false);
        self.status.set_text(&t("Installing..."));
        self.progress.set_fraction(0.0);
        self.pulse.set(true);
        self.stack.set_visible_child_name("installing");

        let events = install::start(InstallRequest {
            method,
            package,
            extension_zip,
        });

        let stack = self.stack.clone();
        let status = self.status.clone();
        let progress = self.progress.clone();
        let pulse = self.pulse.clone();
        let failure = self.failure.clone();
        let done_copy = self.done_copy.clone();
        let done_note = self.done_note.clone();
        let ready_note = self.ready_note.clone();
        let version = self.version.clone();
        let restartable = self.restartable;
        let closed_windows = self.closed_windows.clone();
        let reopen = self.reopen.clone();

        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                match event {
                    InstallEvent::Status(text) => status.set_text(&text),
                    InstallEvent::Progress(Some(fraction)) => {
                        pulse.set(false);
                        progress.set_fraction(fraction.clamp(0.0, 1.0));
                    }
                    InstallEvent::Progress(None) => pulse.set(true),
                    InstallEvent::Finished(installed) => {
                        pulse.set(false);
                        progress.set_fraction(1.0);
                        done_copy.set_text(&tfmt(
                            "ApexShot v{version} is installed.",
                            &[("version", &version)],
                        ));
                        let mut notes = Vec::new();
                        match installed.extension {
                            Some(crate::gnome_extension::ExtensionRefresh::UpdatedNeedsRelogin) => {
                                notes.push(t(
                                    "Log out and back in to finish updating the GNOME extension.",
                                ));
                            }
                            Some(crate::gnome_extension::ExtensionRefresh::Failed(message)) => {
                                notes.push(message);
                            }
                            _ => {}
                        }
                        if closed_windows.get() {
                            notes.push(t("Your windows will reopen with your edits."));
                        }
                        if !notes.is_empty() {
                            done_note.set_text(&notes.join("\n"));
                            done_note.set_visible(true);
                        } else if restartable {
                            done_note.set_visible(false);
                        } else {
                            done_note.set_text(&t("Restart ApexShot to use the new version."));
                            done_note.set_visible(true);
                        }
                        stack.set_visible_child_name("done");
                    }
                    InstallEvent::Cancelled => {
                        pulse.set(false);
                        // The authentication dialog was dismissed, so the
                        // package is still on disk and ready to install again.
                        // The old binary is still the running one, so the
                        // windows that were closed can come straight back.
                        super::reopen_windows(std::mem::take(&mut *reopen.borrow_mut()));
                        ready_note.set_text(&t("Installation was cancelled."));
                        ready_note.set_visible(true);
                        stack.set_visible_child_name("ready");
                    }
                    InstallEvent::Failed(message) => {
                        pulse.set(false);
                        super::reopen_windows(std::mem::take(&mut *reopen.borrow_mut()));
                        failure.set_text(&message);
                        failure.set_visible(true);
                        stack.set_visible_child_name("failed");
                    }
                }
            }
        });
    }
}

/// The brand mark above a centred headline, shared by every state.
pub(crate) fn header(dark: bool, title: &str, copy: &str) -> GtkBox {
    header_parts(dark, title, copy).0
}

/// The same block with its supporting copy exposed for a later text change.
pub(crate) fn header_parts(dark: bool, title: &str, copy: &str) -> (GtkBox, Label) {
    let block = GtkBox::new(Orientation::Vertical, 0);
    let mark = crate::onboarding::showcase::brand_mark(dark);
    mark.set_margin_bottom(18);
    block.append(&mark);
    let parts = crate::onboarding::ui::heading_parts(title, copy);
    block.append(&parts.block);
    (block, parts.copy)
}

pub(crate) fn page() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("update-page");
    page.set_halign(Align::Fill);
    // The actions sit on the page's bottom edge instead of floating under the
    // reading column, which leaves the window's lower inset to the row itself.
    page.set_valign(Align::Fill);
    page.set_vexpand(true);
    page.set_margin_start(PAGE_INSET);
    page.set_margin_end(PAGE_INSET);
    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page
}

/// The reading column above the actions.
pub(crate) fn body() -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, 0);
    body.set_halign(Align::Fill);
    body
}

/// Stack the reading column and the bottom action row, centring the column in
/// the space the actions leave.
pub(crate) fn page_body(page: &GtkBox, body: &GtkBox, actions: &GtkBox) {
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
pub(crate) fn action_row(left: Option<&Button>, right: &Button) -> GtkBox {
    let actions = GtkBox::new(Orientation::Horizontal, 12);
    actions.add_css_class("onboarding-actions");
    actions.add_css_class("update-actions");
    actions.set_halign(Align::Fill);
    actions.set_margin_top(28);

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

pub(crate) fn card() -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("update-card");
    card.set_margin_top(22);
    card.set_halign(Align::Fill);
    card.set_hexpand(true);
    card
}

pub(super) fn row(key: &str, value: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, 16);
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

/// One line of a fact card that has no value column, for simple lists.
pub(super) fn fact_line(text: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("update-row");
    row.set_hexpand(true);

    let label = Label::new(Some(text));
    label.add_css_class("update-row-value");
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    row.append(&label);

    row
}

pub(crate) fn primary_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("settings-primary-btn");
    button.set_halign(Align::Fill);
    button
}

pub(crate) fn quiet_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("onboarding-text-button");
    button.set_halign(Align::Center);
    button
}

/// The same minimize and close controls as onboarding, so the two windows of
/// the first-run family match.
pub(crate) fn window_controls(window: &ApplicationWindow) -> GtkBox {
    crate::onboarding::window_controls(window)
}

pub(super) fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub(super) fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Load the update-specific surfaces after the onboarding sheet.
pub(crate) fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("update.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
}

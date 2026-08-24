use crate::recording::editor::model::format_size;
use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Popover,
};
use std::path::PathBuf;

fn popup_on_editor(parent: &ApplicationWindow, card: &impl IsA<gtk4::Widget>) -> Popover {
    let popover = Popover::new();
    popover.set_parent(parent);
    popover.add_css_class("recording-editor-dialog");
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.set_child(Some(card));
    let width = parent.allocated_width().max(1);
    let height = parent.allocated_height().max(1);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
        width / 2,
        height / 2,
        1,
        1,
    )));
    popover.popup();
    popover
}

fn close_editor_popup(popover: &Popover) {
    popover.popdown();
    popover.unparent();
}

fn dialog_card() -> (GtkBox, GtkBox) {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("recording-editor-dialog-root");
    root.set_margin_top(24);
    root.set_margin_bottom(18);
    root.set_margin_start(24);
    root.set_margin_end(24);
    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("recording-editor-dialog-bg");
    wrapper.set_size_request(380, -1);
    wrapper.append(&root);
    (wrapper, root)
}

pub(super) fn show_success(parent: &ApplicationWindow, path: PathBuf) {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let (wrapper, root) = dialog_card();

    let title = Label::new(Some("Export complete"));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);

    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("file");
    let body = Label::new(Some(&format!(
        "Saved {} ({})",
        file_name,
        format_size(size)
    )));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);

    let button_row = GtkBox::new(Orientation::Horizontal, 0);
    button_row.set_hexpand(true);
    button_row.set_margin_top(8);

    let open_folder = Button::with_label("Open Folder");
    open_folder.set_has_frame(false);
    open_folder.add_css_class("recording-editor-secondary-button");

    let close = Button::with_label("Close");
    close.set_has_frame(false);
    close.add_css_class("recording-editor-primary-button");

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    button_row.append(&open_folder);
    button_row.append(&spacer);
    button_row.append(&close);
    root.append(&title);
    root.append(&body);
    root.append(&button_row);

    let dialog = popup_on_editor(parent, &wrapper);
    close.connect_clicked({
        let dialog = dialog.clone();
        move |_| close_editor_popup(&dialog)
    });
    open_folder.connect_clicked(move |_| {
        if let Some(parent_dir) = path.parent() {
            let _ = crate::utils::open::open_path(parent_dir);
        }
        close_editor_popup(&dialog);
    });
}

pub(super) fn show_manual_zoom_notice(parent: &ApplicationWindow) {
    let (wrapper, root) = dialog_card();

    let title = Label::new(Some("Automatic zoom unavailable"));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);
    let body = Label::new(Some(
        "Mouse movement could not be detected for this recording. Use Manual mode to place zooms yourself.",
    ));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    body.set_wrap(true);
    let close = Button::with_label("Got it");
    close.set_has_frame(false);
    close.add_css_class("recording-editor-primary-button");
    close.set_halign(Align::End);

    root.append(&title);
    root.append(&body);
    root.append(&close);

    let dialog = popup_on_editor(parent, &wrapper);
    close.connect_clicked(move |_| close_editor_popup(&dialog));
}

pub(super) fn show_error(
    parent: &ApplicationWindow,
    title: &str,
    message: &str,
    detail: Option<&str>,
) {
    let (wrapper, root) = dialog_card();

    let title_label = Label::new(Some(title));
    title_label.add_css_class("recording-editor-dialog-title");
    title_label.set_xalign(0.0);

    let body_text = match detail {
        Some(d) if !d.is_empty() => format!("{message}\n\n{d}"),
        _ => message.to_string(),
    };
    let body = Label::new(Some(&body_text));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);

    let button_row = GtkBox::new(Orientation::Horizontal, 12);
    button_row.set_halign(Align::End);
    button_row.set_margin_top(8);

    let close = Button::with_label("Close");
    close.set_has_frame(false);
    close.add_css_class("recording-editor-primary-button");
    button_row.append(&close);

    root.append(&title_label);
    root.append(&body);
    root.append(&button_row);

    let dialog = popup_on_editor(parent, &wrapper);
    close.connect_clicked(move |_| close_editor_popup(&dialog));
}

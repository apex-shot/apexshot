use crate::recording::editor::model::format_size;
use gtk4::{prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Window};
use std::path::PathBuf;

pub(super) fn show_success(parent: &ApplicationWindow, path: PathBuf) {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let dialog = Window::builder()
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .default_width(380)
        .default_height(-1)
        .resizable(false)
        .build();
    dialog.add_css_class("recording-editor-dialog");

    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("recording-editor-dialog-root");
    root.set_margin_top(24);
    root.set_margin_bottom(18);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("recording-editor-dialog-bg");
    wrapper.append(&root);

    let title = Label::new(Some("Export complete"));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);

    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    let body = Label::new(Some(&format!("Saved {} ({})", file_name, format_size(size))));
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

    let dialog_close = dialog.clone();
    close.connect_clicked(move |_| dialog_close.close());

    let dialog_open = dialog.clone();
    let path_open = path.clone();
    open_folder.connect_clicked(move |_| {
        if let Some(parent_dir) = path_open.parent() {
            let _ = std::process::Command::new("xdg-open")
                .arg(parent_dir)
                .spawn();
        }
        dialog_open.close();
    });

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    button_row.append(&open_folder);
    button_row.append(&spacer);
    button_row.append(&close);

    root.append(&title);
    root.append(&body);
    root.append(&button_row);
    dialog.set_child(Some(&wrapper));
    dialog.present();
}

pub(super) fn show_error(
    parent: &ApplicationWindow,
    title: &str,
    message: &str,
    detail: Option<&str>,
) {
    let dialog = Window::builder()
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .default_width(380)
        .default_height(-1)
        .resizable(false)
        .build();
    dialog.add_css_class("recording-editor-dialog");

    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("recording-editor-dialog-root");
    root.set_margin_top(24);
    root.set_margin_bottom(18);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("recording-editor-dialog-bg");
    wrapper.append(&root);

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

    let dialog_close = dialog.clone();
    close.connect_clicked(move |_| dialog_close.close());

    button_row.append(&close);

    root.append(&title_label);
    root.append(&body);
    root.append(&button_row);
    dialog.set_child(Some(&wrapper));
    dialog.present();
}

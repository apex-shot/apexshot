use crate::i18n::{t, tfmt};
use crate::recording::editor::model::format_size;
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Popover,
};
use std::path::PathBuf;

fn popup_on_editor(parent: &ApplicationWindow, card: &impl IsA<gtk4::Widget>) -> Popover {
    let popover = Popover::new();
    popover.set_parent(parent);
    popover.add_css_class("recording-editor-dialog");
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_child(Some(card));
    set_editor_modal_blur(parent, true);
    center_editor_popup(&popover, parent);
    popover.popup();
    glib::idle_add_local_once({
        let popover = popover.clone();
        let parent = parent.clone();
        move || center_editor_popup(&popover, &parent)
    });
    // GTK takes the dialog down without any of its own buttons too: the
    // editor pops every popover down the moment its window loses
    // activation — opening another window does exactly that — and the
    // compositor dismisses a popover whose grab lost its surface the
    // same way. Neither path runs the Close handler, so the modal blur
    // and the input-eating scrim were left up after the dialog itself
    // was gone, and the editor stayed blurred and dead until it was
    // restarted. Every dismissal funnels through the popover's `closed`
    // signal, so that is where the teardown belongs.
    popover.connect_closed(close_editor_popup);
    popover
}

fn center_editor_popup(popover: &Popover, parent: &ApplicationWindow) {
    let width = parent.allocated_width().max(1);
    let height = parent.allocated_height().max(1);
    let card_h = popover.allocated_height().max(1);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
        width / 2,
        (height - card_h).max(0) / 2,
        1,
        1,
    )));
}

fn close_editor_popup(popover: &Popover) {
    if let Some(parent) = popover
        .parent()
        .and_then(|widget| widget.downcast::<ApplicationWindow>().ok())
    {
        set_editor_modal_blur(&parent, false);
    }
    popover.popdown();
    popover.unparent();
}

fn set_editor_modal_blur(parent: &ApplicationWindow, on: bool) {
    let Some(shell) = parent.child() else {
        return;
    };
    let mut child = shell.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.has_css_class("recording-editor-modal-scrim") {
            widget.set_visible(on);
            continue;
        }
        if on {
            widget.add_css_class("recording-editor-modal-blur");
        } else {
            widget.remove_css_class("recording-editor-modal-blur");
        }
    }
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
    wrapper.set_hexpand(false);
    wrapper.set_halign(Align::Center);
    wrapper.append(&root);
    (wrapper, root)
}

fn constrain_dialog_body(body: &Label) {
    body.set_wrap(true);
    body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    body.set_max_width_chars(44);
    body.set_hexpand(false);
}

pub(super) fn show_success(parent: &ApplicationWindow, path: PathBuf) {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let (wrapper, root) = dialog_card();

    let title = Label::new(Some(&t("Export complete")));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);

    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("file");
    let size_text = format_size(size);
    let body = Label::new(Some(&tfmt(
        "Saved {file} ({size})",
        &[("file", file_name), ("size", &size_text)],
    )));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    constrain_dialog_body(&body);

    let button_row = GtkBox::new(Orientation::Horizontal, 0);
    button_row.set_hexpand(true);
    button_row.set_margin_top(8);

    let open_folder = Button::with_label(&t("Open Folder"));
    open_folder.set_has_frame(false);
    open_folder.add_css_class("recording-editor-secondary-button");

    let close = Button::with_label(&t("Close"));
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

    let title = Label::new(Some(&t("No pointer data found")));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);
    let body = Label::new(Some(&t(
        "Open Zoom and choose Detect to place zooms from the recorded clicks, or use Manual mode to place them yourself.",
    )));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    constrain_dialog_body(&body);
    let close = Button::with_label(&t("Got it"));
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
    constrain_dialog_body(&body);

    let button_row = GtkBox::new(Orientation::Horizontal, 12);
    button_row.set_halign(Align::End);
    button_row.set_margin_top(8);

    let close = Button::with_label(&t("Close"));
    close.set_has_frame(false);
    close.add_css_class("recording-editor-primary-button");
    button_row.append(&close);

    root.append(&title_label);
    root.append(&body);
    root.append(&button_row);

    let dialog = popup_on_editor(parent, &wrapper);
    close.connect_clicked(move |_| close_editor_popup(&dialog));
}

pub(super) fn show_export_in_progress_close(
    parent: &ApplicationWindow,
    on_close_anyway: impl Fn() + 'static,
) {
    let (wrapper, root) = dialog_card();

    let title = Label::new(Some(&t("Export is still running.")));
    title.add_css_class("recording-editor-dialog-title");
    title.set_xalign(0.0);
    let body = Label::new(Some(&t(
        "Closing now stops the encode. Your edit session is kept.",
    )));
    body.add_css_class("recording-editor-dialog-body");
    body.set_xalign(0.0);
    constrain_dialog_body(&body);

    let button_row = GtkBox::new(Orientation::Horizontal, 12);
    button_row.set_halign(Align::End);
    button_row.set_margin_top(8);

    let keep = Button::with_label(&t("Keep editing"));
    keep.set_has_frame(false);
    keep.add_css_class("recording-editor-secondary-button");
    let close_anyway = Button::with_label(&t("Close anyway"));
    close_anyway.set_has_frame(false);
    close_anyway.add_css_class("recording-editor-primary-button");
    button_row.append(&keep);
    button_row.append(&close_anyway);

    root.append(&title);
    root.append(&body);
    root.append(&button_row);

    let dialog = popup_on_editor(parent, &wrapper);
    keep.connect_clicked({
        let dialog = dialog.clone();
        move |_| close_editor_popup(&dialog)
    });
    close_anyway.connect_clicked({
        let dialog = dialog.clone();
        move |_| {
            close_editor_popup(&dialog);
            on_close_anyway();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_dismissal_tears_the_modal_blur_down() {
        // The regression this pins: exporting a video raised the success
        // dialog, and opening another window made the editor's
        // deactivation sweep pop that dialog down on its own. That path
        // never ran the dialog's Close handler, so the modal blur and
        // the scrim stayed up — the editor stayed blurred, dimmed and
        // pointer-dead until it was restarted. `popup_on_editor` owns
        // the modal state, so it has to tear it down through the one
        // signal every dismissal funnels through.
        let source = include_str!("dialogs.rs");
        let popup_start = source.find("fn popup_on_editor").expect("popup_on_editor");
        let popup_end = source
            .find("fn center_editor_popup")
            .expect("center_editor_popup");
        let popup = &source[popup_start..popup_end];
        assert!(
            popup.contains("connect_closed") && popup.contains("close_editor_popup"),
            "the modal blur and scrim must come down on the popover's `closed` signal, not only on the dialog's Close button"
        );
    }

    #[test]
    fn the_editor_comes_back_when_the_sweep_takes_the_dialog_down() {
        // Built through the real popup and the real shell: the sweep's
        // plain `popdown` — the whole of what focus-out does to a
        // popover — has to leave the editor unblurred, the scrim down
        // and the dialog unparented.
        let Some(result) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            use gtk4::{
                glib as gtk_glib, Application, ApplicationWindow, Box as GtkBox, Orientation,
                Overlay,
            };

            let app = Application::builder()
                .application_id(crate::app_identity::app_id())
                .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
                .build();
            // `startup` has to fire before any window is added, or GTK
            // refuses to present the window and the dialog never maps.
            app.register(None::<&gtk4::gio::Cancellable>)
                .expect("the test application to register");
            let window = ApplicationWindow::new(&app);
            let shell = Overlay::new();
            let root = GtkBox::new(Orientation::Vertical, 0);
            root.add_css_class("recording-editor-root");
            let scrim = GtkBox::new(Orientation::Vertical, 0);
            scrim.add_css_class("recording-editor-modal-scrim");
            scrim.set_visible(false);
            shell.set_child(Some(&root));
            shell.add_overlay(&scrim);
            window.set_child(Some(&shell));

            // Surfaces are positioned by the compositor, so the loop
            // has to run in real time before visibility means anything.
            // The editor is already up when a dialog opens, so the
            // window presents before the dialog does.
            window.present();
            let pump = || {
                let ctx = gtk_glib::MainContext::default();
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
                while std::time::Instant::now() < deadline {
                    while ctx.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            pump();

            let card = GtkBox::new(Orientation::Vertical, 0);
            let dialog = super::popup_on_editor(&window, &card);
            pump();

            let blurred = root.has_css_class("recording-editor-modal-blur");
            let scrim_up = scrim.is_visible();

            // What `popdown_popovers` does to every popover when the
            // window loses activation to another window.
            dialog.popdown();
            pump();

            let blurred_after = root.has_css_class("recording-editor-modal-blur");
            let scrim_after = scrim.is_visible();
            let parented = dialog.parent().is_some();

            dialog.unparent();
            window.destroy();
            (blurred, scrim_up, blurred_after, scrim_after, parented)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert!(result.0, "the dialog must blur the editor while it is up");
        assert!(result.1, "the dialog must raise the scrim while it is up");
        assert!(
            !result.2,
            "the sweep's popdown must take the modal blur off the editor"
        );
        assert!(
            !result.3,
            "the sweep's popdown must take the scrim back down"
        );
        assert!(
            !result.4,
            "the sweep's popdown must hand the dialog back from its parent"
        );
    }
}

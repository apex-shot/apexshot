use crate::i18n::{t, tfmt};
use crate::recording::editor::model::format_size;
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Popover,
};
use std::path::PathBuf;

/// The width of the image editor's Motion confirm card, which every dialog in
/// this editor is sized to.
const DIALOG_CARD_WIDTH: i32 = 360;

/// The body width that card wraps its own body to.
const DIALOG_BODY_CHARS: i32 = 44;

pub(super) fn popup_on_editor(
    parent: &ApplicationWindow,
    card: &impl IsA<gtk4::Widget>,
) -> Popover {
    let popover = prepare_editor_popup(parent, card);
    present_editor_popup(&popover, parent);
    popover
}

pub(super) fn prepare_editor_popup(
    parent: &ApplicationWindow,
    card: &impl IsA<gtk4::Widget>,
) -> Popover {
    let popover = Popover::new();
    popover.set_parent(parent);
    popover.add_css_class("recording-editor-dialog");
    if let Some(shell) = parent.child() {
        for class in [
            "editor-theme-light",
            "editor-theme-dark",
            "editor-reduced-transparency",
        ] {
            if find_widget_with_class(&shell, class).is_some() {
                popover.add_css_class(class);
            }
        }
    }
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_child(Some(card));
    set_editor_modal_blur(parent, true);
    center_editor_popup(&popover, parent);
    let parent_weak = parent.downgrade();
    popover
        .connect_closed(move |popover| close_editor_popup_with_parent(popover, Some(&parent_weak)));
    popover
}

pub(super) fn present_editor_popup(popover: &Popover, parent: &ApplicationWindow) {
    center_editor_popup(popover, parent);
    popover.popup();
    glib::idle_add_local_once({
        let popover = popover.downgrade();
        let parent = parent.downgrade();
        move || {
            let (Some(popover), Some(parent)) = (popover.upgrade(), parent.upgrade()) else {
                return;
            };
            if popover.parent().is_some() && popover.is_visible() && parent.is_visible() {
                center_editor_popup(&popover, &parent);
            }
        }
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

pub(super) fn close_editor_popup(popover: &Popover) {
    let parent = popover
        .parent()
        .and_then(|widget| widget.downcast::<ApplicationWindow>().ok())
        .map(|parent| parent.downgrade());
    close_editor_popup_with_parent(popover, parent.as_ref());
}

pub(super) fn close_editor_popup_with_parent(
    popover: &Popover,
    parent: Option<&glib::WeakRef<ApplicationWindow>>,
) {
    if let Some(parent) = parent.and_then(|parent| parent.upgrade()) {
        set_editor_modal_blur(&parent, false);
    }
    if popover.is_visible() {
        popover.popdown();
    }
    if popover.parent().is_some() {
        popover.unparent();
    }
}

fn set_editor_modal_blur(parent: &ApplicationWindow, on: bool) {
    let Some(shell) = parent.child() else {
        return;
    };
    if let Some(root) = find_widget_with_class(&shell, "recording-editor-root")
        .or_else(|| find_widget_with_class(&shell, "editor-root"))
    {
        if on {
            root.add_css_class("recording-editor-modal-blur");
        } else {
            root.remove_css_class("recording-editor-modal-blur");
        }
    }
    let Ok(overlay) = shell.clone().downcast::<gtk4::Overlay>() else {
        return;
    };
    let overlay_widget: gtk4::Widget = overlay.clone().upcast();
    let scrim = find_widget_with_class(&overlay_widget, "recording-editor-modal-scrim")
        .and_then(|widget| widget.downcast::<GtkBox>().ok())
        .or_else(|| {
            if !on {
                return None;
            }
            let scrim = GtkBox::new(Orientation::Vertical, 0);
            scrim.add_css_class("recording-editor-modal-scrim");
            scrim.set_halign(Align::Fill);
            scrim.set_valign(Align::Fill);
            scrim.set_hexpand(true);
            scrim.set_vexpand(true);
            scrim.set_can_target(true);
            overlay.add_overlay(&scrim);
            overlay.set_clip_overlay(&scrim, true);
            Some(scrim)
        });
    if let Some(scrim) = scrim {
        if find_widget_with_class(&shell, "editor-theme-light").is_some() {
            scrim.add_css_class("editor-theme-light");
        }
        scrim.set_visible(on);
    }
}

fn find_widget_with_class(root: &impl IsA<gtk4::Widget>, class: &str) -> Option<gtk4::Widget> {
    let mut stack = vec![root.clone().upcast::<gtk4::Widget>()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class(class) {
            return Some(widget);
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            stack.push(current.clone());
            child = current.next_sibling();
        }
    }
    None
}

pub(super) fn dialog_card() -> (GtkBox, GtkBox) {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("recording-editor-dialog-root");
    // The image editor's Motion confirm card pads 20px 22px 16px; matching
    // those insets is what makes this card read as the same dialog.
    root.set_margin_top(20);
    root.set_margin_bottom(16);
    root.set_margin_start(22);
    root.set_margin_end(22);
    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("recording-editor-dialog-bg");
    // That card is 360px wide. Keep that as this card's floor, not as content
    // inside its insets: a floor wide enough for the longer card plus one line
    // of body text is what strung this dialog out into a shallow strip.
    wrapper.set_size_request(DIALOG_CARD_WIDTH, -1);
    wrapper.set_hexpand(false);
    wrapper.set_halign(Align::Center);
    wrapper.append(&root);
    (wrapper, root)
}

/// Keep a dialog body to the width of the image editor's Motion confirm card.
///
/// The 44-character cap only holds while the label keeps GTK's default word
/// wrap: asking for `WrapMode::WordChar` makes the label request its whole line
/// instead, so a body holding a file name stretched the card to twice the
/// width of the card this dialog is modelled on. The Motion card's body relies
/// on the default mode the same way.
pub(super) fn constrain_dialog_body(body: &Label) {
    body.set_wrap(true);
    body.set_max_width_chars(DIALOG_BODY_CHARS);
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
    open_folder.add_css_class("recording-editor-ghost-button");

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
    /// Export complete pairs a bare-text secondary with the filled orange
    /// primary — Open Folder alone on the left, Close at the far right — the
    /// way the image editor's confirm dialogs read, instead of two filled
    /// pills huddling together.
    #[test]
    fn export_complete_pairs_a_ghost_secondary_with_the_primary() {
        let source = include_str!("dialogs.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let start = source
            .find("pub(super) fn show_success")
            .expect("show_success");
        let end = source
            .find("pub(super) fn show_manual_zoom_notice")
            .expect("show_manual_zoom_notice");
        let success = &source[start..end];
        assert!(
            success.contains("open_folder.add_css_class(\"recording-editor-ghost-button\")")
                && success.contains("close.add_css_class(\"recording-editor-primary-button\")")
                && success.contains("spacer.set_hexpand(true)")
                && success.contains("button_row.append(&open_folder)")
                && success.contains("button_row.append(&spacer)")
                && success.contains("button_row.append(&close)"),
            "Export complete should put a bare-text Open Folder on the left and the orange Close on the right"
        );
    }

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

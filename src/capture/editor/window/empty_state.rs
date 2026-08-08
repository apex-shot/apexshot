//! Empty-editor drop zone and same-window image load path (PR 10.14).
//!
//! Owns the empty-state overlay, open dialog, drop target, supported-file
//! checks, loading banner, and reload-into-same-window flow. Session reuse
//! controller cleanup stays in setup.

use gtk4::gdk;
use gtk4::gio;
use gtk4::{
    glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button,
    FileChooserAction, FileChooserNative, FileFilter, Image, Label, Orientation, Overlay,
    ResponseType, Revealer, Spinner,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

fn is_supported_image_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp")
    )
}

/// Validate + decode the image on a background thread (showing the loading
/// banner, like the video editor's async video load), then rebuild the
/// editor in the SAME window with the file loaded.
#[allow(clippy::too_many_arguments)]
fn load_image_into_editor(
    app: &Application,
    window: &ApplicationWindow,
    path: PathBuf,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
    loading: Rc<Cell<bool>>,
    open_button: &Button,
) {
    if loading.get() {
        return;
    }
    loading.set(true);
    loading_revealer.set_reveal_child(true);
    loading_spinner.set_visible(true);
    loading_spinner.start();
    open_button.set_sensitive(false);

    let (sender, receiver) = mpsc::channel::<Result<(), String>>();
    let path_thread = path.clone();
    std::thread::spawn(move || {
        let result = image::open(&path_thread)
            .map(|_| ())
            .map_err(|e| e.to_string());
        let _ = sender.send(result);
    });

    let app = app.clone();
    let window = window.clone();
    let loading_revealer = loading_revealer.clone();
    let loading_spinner = loading_spinner.clone();
    let open_button = open_button.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let stop_loading = || {
            loading.set(false);
            loading_revealer.set_reveal_child(false);
            loading_spinner.stop();
            loading_spinner.set_visible(false);
            open_button.set_sensitive(true);
        };
        match receiver.try_recv() {
            Ok(Ok(())) => {
                stop_loading();
                // Rebuild the editor UI with the image, reusing this window.
                super::setup_editor_window_full(&app, path.clone(), false, Some(window.clone()));
                glib::ControlFlow::Break
            }
            Ok(Err(err)) => {
                stop_loading();
                eprintln!("[editor] Failed to load image: {err}");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                stop_loading();
                glib::ControlFlow::Break
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn show_open_image_dialog(
    app: &Application,
    parent: &ApplicationWindow,
    loading_revealer: &Revealer,
    loading_spinner: &Spinner,
    loading: Rc<Cell<bool>>,
    open_button: &Button,
) {
    let chooser = FileChooserNative::new(
        Some("Open image"),
        Some(parent),
        FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );

    let filter = FileFilter::new();
    filter.set_name(Some("Image files"));
    filter.add_mime_type("image/png");
    filter.add_mime_type("image/jpeg");
    filter.add_mime_type("image/webp");
    filter.add_pattern("*.png");
    filter.add_pattern("*.jpg");
    filter.add_pattern("*.jpeg");
    filter.add_pattern("*.webp");
    chooser.add_filter(&filter);

    let app_ref = app.clone();
    let parent_ref = parent.clone();
    let revealer_ref = loading_revealer.clone();
    let spinner_ref = loading_spinner.clone();
    let open_button_ref = open_button.clone();
    chooser.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(file) = dialog.file() {
                if let Some(path) = file.path() {
                    if is_supported_image_path(&path) {
                        // Load into the same window, like the video editor.
                        load_image_into_editor(
                            &app_ref,
                            &parent_ref,
                            path.to_path_buf(),
                            &revealer_ref,
                            &spinner_ref,
                            loading.clone(),
                            &open_button_ref,
                        );
                    }
                }
            }
        }
        dialog.hide();
    });
    chooser.show();
}

/// Install empty-editor drop overlay, open button, loading banner, and drop target.
pub(super) fn install_empty_drop_zone(
    app: &Application,
    window: &ApplicationWindow,
    canvas_with_toolbar: &Overlay,
    root_overlay: &Overlay,
) {
    // Reuse the video editor's button / banner styles so the empty
    // states look identical.
    crate::recording::editor::ui_support::install_recording_editor_css();

    let drop_center = GtkBox::new(Orientation::Vertical, 14);
    drop_center.set_halign(Align::Center);
    drop_center.set_valign(Align::Center);
    drop_center.set_can_target(true);

    let drop_icon = Image::from_icon_name("image-x-generic-symbolic");
    drop_icon.add_css_class("recording-editor-empty-icon");
    drop_icon.set_pixel_size(42);
    drop_icon.set_halign(Align::Center);

    let drop_title = Label::new(Some("Drop an image here"));
    drop_title.add_css_class("recording-editor-empty-title");
    drop_title.set_halign(Align::Center);

    let drop_hint = Label::new(Some("PNG, JPEG, or WebP"));
    drop_hint.add_css_class("recording-editor-empty-hint");
    drop_hint.set_halign(Align::Center);

    let open_btn = Button::with_label("Open Folder");
    open_btn.set_has_frame(false);
    open_btn.add_css_class("recording-editor-primary-button");
    open_btn.add_css_class("recording-editor-empty-open-button");
    open_btn.set_halign(Align::Center);

    drop_center.append(&drop_icon);
    drop_center.append(&drop_title);
    drop_center.append(&drop_hint);
    drop_center.append(&open_btn);

    // Keep the empty state centered in the canvas pane, excluding the inspector.
    canvas_with_toolbar.add_overlay(&drop_center);

    // Loading banner (same as the video editor's "Loading video…").
    let loading_box = GtkBox::new(Orientation::Horizontal, 8);
    loading_box.add_css_class("recording-editor-drop-banner");
    loading_box.set_can_target(false);
    loading_box.set_halign(Align::Center);
    let loading_spinner = Spinner::new();
    loading_spinner.set_size_request(16, 16);
    loading_spinner.set_can_target(false);
    let loading_label = Label::new(Some("Loading image…"));
    loading_label.add_css_class("recording-editor-drop-label");
    loading_label.set_can_target(false);
    loading_box.append(&loading_spinner);
    loading_box.append(&loading_label);
    let loading_revealer = Revealer::new();
    loading_revealer.set_can_target(false);
    loading_revealer.set_halign(Align::Center);
    loading_revealer.set_valign(Align::Start);
    loading_revealer.set_child(Some(&loading_box));
    loading_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    loading_revealer.set_reveal_child(false);
    root_overlay.add_overlay(&loading_revealer);

    let loading = Rc::new(Cell::new(false));

    let app_open = app.clone();
    let window_open = window.clone();
    let revealer_open = loading_revealer.clone();
    let spinner_open = loading_spinner.clone();
    let loading_open = loading.clone();
    let open_btn_ref = open_btn.clone();
    open_btn.connect_clicked(move |_| {
        show_open_image_dialog(
            &app_open,
            &window_open,
            &revealer_open,
            &spinner_open,
            loading_open.clone(),
            &open_btn_ref,
        );
    });

    let drop_target = gtk4::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    let app_drop = app.clone();
    let window_drop = window.clone();
    let revealer_drop = loading_revealer.clone();
    let spinner_drop = loading_spinner.clone();
    let loading_drop = loading.clone();
    let open_btn_drop = open_btn.clone();
    drop_target.connect_drop(move |_, value, _x, _y| {
        if loading_drop.get() {
            return false;
        }
        let Ok(file) = value.get::<gio::File>() else {
            return false;
        };
        let Some(path) = file.path() else {
            return false;
        };
        if !is_supported_image_path(&path) {
            return false;
        }
        // Load into the SAME window (no close/reopen), like the video
        // editor does.
        load_image_into_editor(
            &app_drop,
            &window_drop,
            path.to_path_buf(),
            &revealer_drop,
            &spinner_drop,
            loading_drop.clone(),
            &open_btn_drop,
        );
        true
    });
    root_overlay.add_controller(drop_target);
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_drop_zone_checks_supported_files_and_reuses_window() {
        let source = include_str!("empty_state.rs");
        assert!(
            source.contains("fn is_supported_image_path")
                && source
                    .contains("Some(\"png\") | Some(\"jpg\") | Some(\"jpeg\") | Some(\"webp\")")
                && source.contains(
                    "setup_editor_window_full(&app, path.clone(), false, Some(window.clone()))"
                )
                && source.contains("Drop an image here")
                && source.contains("Loading image…")
                && source.contains("root_overlay.add_controller(drop_target)"),
            "empty state must validate image types and reload into the same window"
        );
    }
}

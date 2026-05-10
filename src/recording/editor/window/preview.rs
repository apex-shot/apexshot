use super::timeline;
use crate::recording::editor::model::VideoEditState;
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Label, MediaFile, Orientation, Overlay, Picture,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) fn build_preview(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    thumbnails: Vec<PathBuf>,
) -> GtkBox {
    let path = {
        let state = state.lock().unwrap();
        state.metadata.path.clone()
    };

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-preview-frame");
    root.set_hexpand(true);
    root.set_vexpand(true);

    let workspace = GtkBox::new(Orientation::Vertical, 0);
    workspace.add_css_class("recording-editor-preview-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.set_halign(Align::Fill);
    workspace.set_valign(Align::Fill);

    let media = MediaFile::for_filename(path);
    media.set_loop(true);

    let picture = Picture::for_paintable(&media);
    picture.add_css_class("recording-editor-video");
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Center);
    picture.set_valign(Align::Center);
    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);

    let overlay = Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_child(Some(&picture));

    let dim_badge = Label::new(None);
    dim_badge.add_css_class("recording-editor-dim-badge");
    dim_badge.set_halign(Align::Start);
    dim_badge.set_valign(Align::Start);
    dim_badge.set_margin_start(12);
    dim_badge.set_margin_top(12);
    dim_badge.set_can_target(false);
    overlay.add_overlay(&dim_badge);

    // Update badge live
    {
        let state = state.clone();
        let dim_badge = dim_badge.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let (w, h) = {
                let s = state.lock().unwrap();
                s.target_dimensions()
            };
            dim_badge.set_text(&format!("{w} × {h}"));
            glib::ControlFlow::Continue
        });
    }

    workspace.append(&overlay);
    root.append(&workspace);

    let timeline_widget = timeline::build_timeline(state, estimate_label, thumbnails, media);
    root.append(&timeline_widget);

    root
}


use crate::history::scan::{CaptureEntry, MediaKind};
use crate::history::thumbnails;
use crate::recording::editor::model::{ProjectMedia, ProjectMediaKind, VideoEditState};
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, FlowBox, Label, Orientation,
    Image, Picture, Revealer, RevealerTransitionType, ScrolledWindow,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryFilter {
    All,
    Videos,
    Audio,
    Images,
}

pub(super) fn build_media_library(state: Option<Arc<Mutex<VideoEditState>>>) -> Revealer {
    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideRight);
    revealer.set_reveal_child(false);
    revealer.set_hexpand(false);
    revealer.set_vexpand(true);
    revealer.set_valign(Align::Fill);

    let shell = GtkBox::new(Orientation::Vertical, 0);
    shell.add_css_class("recording-editor-media-shell");
    shell.add_css_class("editor-right-inspector");
    shell.add_css_class("recording-editor-inspector");
    shell.set_hexpand(false);
    shell.set_vexpand(true);

    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("recording-editor-media-library");
    root.set_width_request(280);
    root.set_hexpand(false);
    root.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some("Media"));
    title.add_css_class("recording-editor-media-title-heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let close = Button::new();
    close.set_has_frame(false);
    close.add_css_class("recording-editor-media-close");
    close.set_tooltip_text(Some("Close"));
    close.set_halign(Align::End);
    close.set_valign(Align::Center);
    let close_icon = Image::from_icon_name("go-previous-symbolic");
    close_icon.set_pixel_size(16);
    close.set_child(Some(&close_icon));
    close.connect_clicked({
        let revealer = revealer.clone();
        move |_| revealer.set_reveal_child(false)
    });
    header.append(&close);
    root.append(&header);

    let tabs = GtkBox::new(Orientation::Horizontal, 4);
    tabs.add_css_class("recording-editor-media-tabs");
    let all = tab_button("All", true);
    let videos = tab_button("Videos", false);
    let audio = tab_button("Audio", false);
    let images = tab_button("Images", false);
    videos.set_group(Some(&all));
    audio.set_group(Some(&all));
    images.set_group(Some(&all));
    tabs.append(&all);
    tabs.append(&videos);
    tabs.append(&audio);
    tabs.append(&images);
    root.append(&tabs);

    let search = Entry::new();
    search.add_css_class("recording-editor-media-search");
    search.set_placeholder_text(Some("Search Media..."));
    search.set_hexpand(true);
    root.append(&search);

    let upload = Button::with_label("Upload Media");
    upload.set_has_frame(false);
    upload.add_css_class("recording-editor-media-upload");
    upload.set_hexpand(true);
    upload.set_sensitive(state.is_some());
    root.append(&upload);

    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    let grid = FlowBox::new();
    grid.add_css_class("recording-editor-media-grid");
    grid.set_selection_mode(gtk4::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_max_children_per_line(2);
    grid.set_min_children_per_line(2);
    grid.set_valign(Align::Start);
    scroller.set_child(Some(&grid));
    root.append(&scroller);

    let empty = Label::new(Some("No media in this project"));
    empty.add_css_class("recording-editor-time-label");
    empty.set_halign(Align::Center);
    empty.set_margin_top(24);
    empty.set_visible(false);
    root.append(&empty);

    let filter = Rc::new(Cell::new(LibraryFilter::All));
    let query = Rc::new(RefCell::new(String::new()));

    let reload = {
        let grid = grid.clone();
        let empty = empty.clone();
        let filter = filter.clone();
        let query = query.clone();
        let state = state.clone();
        Rc::new(move || {
            let items = state
                .as_ref()
                .map(|state| state.lock().unwrap().project_media.clone())
                .unwrap_or_default();
            populate_grid(&grid, &empty, &items, filter.get(), &query.borrow());
        })
    };
    reload();

    all.connect_toggled({
        let filter = filter.clone();
        let reload = reload.clone();
        move |btn| {
            if btn.is_active() {
                filter.set(LibraryFilter::All);
                reload();
            }
        }
    });
    videos.connect_toggled({
        let filter = filter.clone();
        let reload = reload.clone();
        move |btn| {
            if btn.is_active() {
                filter.set(LibraryFilter::Videos);
                reload();
            }
        }
    });
    audio.connect_toggled({
        let filter = filter.clone();
        let reload = reload.clone();
        move |btn| {
            if btn.is_active() {
                filter.set(LibraryFilter::Audio);
                reload();
            }
        }
    });
    images.connect_toggled({
        let filter = filter.clone();
        let reload = reload.clone();
        move |btn| {
            if btn.is_active() {
                filter.set(LibraryFilter::Images);
                reload();
            }
        }
    });
    search.connect_changed({
        let query = query.clone();
        let reload = reload.clone();
        move |entry| {
            *query.borrow_mut() = entry.text().to_ascii_lowercase();
            reload();
        }
    });

    if let Some(state) = state {
        upload.connect_clicked({
            let reload = reload.clone();
            move |button| import_into_project(button, state.clone(), reload.clone())
        });
    }

    shell.append(&root);
    revealer.set_child(Some(&shell));
    revealer
}

fn tab_button(label: &str, active: bool) -> CheckButton {
    let button = CheckButton::with_label(label);
    button.add_css_class("recording-editor-media-tab");
    button.set_active(active);
    button
}

fn populate_grid(
    grid: &FlowBox,
    empty: &Label,
    items: &[ProjectMedia],
    filter: LibraryFilter,
    query: &str,
) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    let filtered: Vec<&ProjectMedia> = items
        .iter()
        .filter(|item| match filter {
            LibraryFilter::All => true,
            LibraryFilter::Videos => item.kind == ProjectMediaKind::Video,
            LibraryFilter::Audio => item.kind == ProjectMediaKind::Audio,
            LibraryFilter::Images => item.kind == ProjectMediaKind::Image,
        })
        .filter(|item| query.is_empty() || item.display_name.to_ascii_lowercase().contains(query))
        .collect();
    empty.set_visible(filtered.is_empty());
    empty.set_text(if items.is_empty() {
        "No media in this project"
    } else {
        "Nothing in this tab"
    });
    for item in filtered {
        grid.insert(&build_card(item), -1);
    }
}

fn build_card(item: &ProjectMedia) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.add_css_class("recording-editor-media-card");
    card.set_hexpand(true);

    let thumb_wrap = GtkBox::new(Orientation::Vertical, 0);
    thumb_wrap.add_css_class("recording-editor-media-thumb");
    thumb_wrap.set_size_request(-1, 86);
    let picture = Picture::new();
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_size_request(-1, 86);
    thumb_wrap.append(&picture);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&thumb_wrap));
    if let Some(duration) = item.duration_seconds {
        let badge = Label::new(Some(&format_duration(duration)));
        badge.add_css_class("recording-editor-media-badge");
        badge.set_halign(Align::End);
        badge.set_valign(Align::End);
        badge.set_margin_end(8);
        badge.set_margin_bottom(8);
        overlay.add_overlay(&badge);
    } else if item.kind == ProjectMediaKind::Audio {
        let badge = Label::new(Some("Audio"));
        badge.add_css_class("recording-editor-media-badge");
        badge.set_halign(Align::Start);
        badge.set_valign(Align::End);
        badge.set_margin_start(8);
        badge.set_margin_bottom(8);
        overlay.add_overlay(&badge);
    }
    card.append(&overlay);

    let title = Label::new(Some(&item.display_name));
    title.add_css_class("recording-editor-media-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    card.append(&title);

    if item.kind != ProjectMediaKind::Audio {
        load_thumbnail(&picture, item);
    }
    card
}

fn load_thumbnail(picture: &Picture, item: &ProjectMedia) {
    let (tx, rx) = mpsc::channel();
    let entry = CaptureEntry {
        path: item.path.clone(),
        display_name: item.display_name.clone(),
        modified: None,
        size_bytes: 0,
        kind: match item.kind {
            ProjectMediaKind::Image => MediaKind::Image,
            _ => MediaKind::Video,
        },
    };
    std::thread::spawn(move || {
        let _ = tx.send(thumbnails::thumbnail_for_entry(&entry).ok());
    });
    let picture = picture.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        match rx.try_recv() {
            Ok(Some(thumb)) => {
                picture.set_filename(Some(&thumb));
                glib::ControlFlow::Break
            }
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        }
    });
}

fn import_into_project(
    button: &Button,
    state: Arc<Mutex<VideoEditState>>,
    reload: Rc<dyn Fn()>,
) {
    let Some(root) = button.root() else {
        return;
    };
    let window = root.downcast::<gtk4::Window>().ok();
    let chooser = gtk4::FileChooserNative::new(
        Some("Add to project"),
        window.as_ref(),
        gtk4::FileChooserAction::Open,
        Some("Add"),
        Some("Cancel"),
    );
    chooser.set_select_multiple(true);
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("Media"));
    for mime in ["video/mp4", "image/png", "image/jpeg", "image/webp"] {
        filter.add_mime_type(mime);
    }
    chooser.add_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response != gtk4::ResponseType::Accept {
            return;
        }
        let files = chooser.files();
        let n = files.n_items();
        let mut state = state.lock().unwrap();
        for index in 0..n {
            let Some(obj) = files.item(index) else {
                continue;
            };
            let Ok(file) = obj.downcast::<gtk4::gio::File>() else {
                continue;
            };
            let Some(path) = file.path() else {
                continue;
            };
            if let Some(item) = project_item_from_path(path) {
                state.add_project_media(item);
            }
        }
        drop(state);
        reload();
    });
    chooser.show();
}

fn project_item_from_path(path: PathBuf) -> Option<ProjectMedia> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let display_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Media")
        .to_string();
    let kind = match ext.as_str() {
        "mp4" | "webm" | "gif" => ProjectMediaKind::Video,
        "png" | "jpg" | "jpeg" | "webp" => ProjectMediaKind::Image,
        _ => return None,
    };
    Some(ProjectMedia {
        path,
        display_name,
        kind,
        duration_seconds: None,
    })
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let secs = (seconds - minutes as f64 * 60.0).floor() as u64;
    format!("{minutes:02}:{secs:02}")
}

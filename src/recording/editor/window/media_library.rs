use crate::history::scan::{CaptureEntry, MediaKind};
use crate::history::thumbnails;
use crate::recording::editor::model::{ProjectMedia, ProjectMediaKind, VideoEditState, ZoomClip};
use gtk4::gdk;
use gtk4::gio;
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, DropTarget, Image, Label, Orientation, Picture,
    ScrolledWindow,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone)]
pub(super) struct EmptyOpenHooks {
    pub on_click: Rc<dyn Fn()>,
    pub on_drop_video: Rc<dyn Fn(PathBuf)>,
    pub open_button_slot: Rc<RefCell<Option<Button>>>,
}

const LIBRARY_WIDTH: i32 = 240;
const THUMB_WIDTH: i32 = 56;
const THUMB_HEIGHT: i32 = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryFilter {
    Video,
    Audio,
    Image,
    Zoom,
}

pub(super) fn build_media_library(
    state: Option<Arc<Mutex<VideoEditState>>>,
    empty_open: Option<EmptyOpenHooks>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.add_css_class("recording-editor-media-library");
    root.set_width_request(LIBRARY_WIDTH);
    root.set_hexpand(false);
    root.set_vexpand(true);

    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-media-tabs");
    let video = kind_tab("camera-video-symbolic", "Video");
    let audio = kind_tab("audio-x-generic-symbolic", "Audio");
    let image = kind_tab("image-x-generic-symbolic", "Image");
    let zoom = kind_tab("zoom-in-symbolic", "Zoom");
    video.add_css_class("recording-editor-media-tab-active");
    tabs.append(&video);
    tabs.append(&audio);
    tabs.append(&image);
    tabs.append(&zoom);
    root.append(&tabs);
    let tab_buttons = [video.clone(), audio.clone(), image.clone(), zoom.clone()];

    let upload = build_upload_dropzone();
    root.append(&upload);

    let scroller = ScrolledWindow::new();
    scroller.add_css_class("recording-editor-media-scroll");
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("recording-editor-media-list");
    list.set_hexpand(true);
    list.set_valign(Align::Start);

    let empty = Label::new(Some("No media"));
    empty.add_css_class("recording-editor-media-empty");
    empty.set_halign(Align::Start);
    empty.set_xalign(0.0);
    empty.set_margin_top(8);
    empty.set_visible(false);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.append(&list);
    body.append(&empty);
    scroller.set_child(Some(&body));
    root.append(&scroller);

    let filter = Rc::new(Cell::new(LibraryFilter::Video));

    let reload: Rc<dyn Fn()> = {
        let list = list.clone();
        let empty = empty.clone();
        let filter = filter.clone();
        let state = state.clone();
        Rc::new(move || {
            let (items, zooms) = state
                .as_ref()
                .map(|state| {
                    let guard = state.lock().unwrap();
                    (guard.project_media.clone(), guard.zoom_clips.clone())
                })
                .unwrap_or_default();
            populate_list(&list, &empty, &items, &zooms, filter.get());
        })
    };
    reload();

    bind_tab(
        &video,
        LibraryFilter::Video,
        &filter,
        &reload,
        &tab_buttons,
        &upload,
    );
    bind_tab(
        &audio,
        LibraryFilter::Audio,
        &filter,
        &reload,
        &tab_buttons,
        &upload,
    );
    bind_tab(
        &image,
        LibraryFilter::Image,
        &filter,
        &reload,
        &tab_buttons,
        &upload,
    );
    bind_tab(
        &zoom,
        LibraryFilter::Zoom,
        &filter,
        &reload,
        &tab_buttons,
        &upload,
    );

    wire_upload_dropzone(
        &upload,
        state.clone(),
        empty_open,
        filter.clone(),
        reload.clone(),
    );

    let last_sig = Rc::new(Cell::new(library_signature(&state, filter.get())));
    glib::timeout_add_local(std::time::Duration::from_millis(400), {
        let state = state.clone();
        let filter = filter.clone();
        let reload = reload.clone();
        move || {
            let sig = library_signature(&state, filter.get());
            if sig != last_sig.get() {
                last_sig.set(sig);
                reload();
            }
            glib::ControlFlow::Continue
        }
    });

    root
}

fn library_signature(state: &Option<Arc<Mutex<VideoEditState>>>, tab: LibraryFilter) -> u64 {
    let Some(state) = state else {
        return 0;
    };
    let guard = state.lock().unwrap();
    match tab {
        LibraryFilter::Zoom => {
            let mut hash = guard.zoom_clips.len() as u64;
            for clip in &guard.zoom_clips {
                hash = hash
                    .wrapping_mul(33)
                    .wrapping_add((clip.start * 1000.0) as u64);
                hash = hash
                    .wrapping_mul(33)
                    .wrapping_add((clip.end * 1000.0) as u64);
            }
            hash
        }
        _ => {
            let mut hash = guard.project_media.len() as u64;
            for item in &guard.project_media {
                hash = hash
                    .wrapping_mul(33)
                    .wrapping_add(item.path.as_os_str().len() as u64);
            }
            hash
        }
    }
}

fn bind_tab(
    button: &Button,
    value: LibraryFilter,
    filter: &Rc<Cell<LibraryFilter>>,
    reload: &Rc<dyn Fn()>,
    tabs: &[Button; 4],
    upload: &Button,
) {
    let filter = filter.clone();
    let reload = reload.clone();
    let tabs = tabs.clone();
    let upload = upload.clone();
    button.connect_clicked(move |_| {
        if filter.get() == value {
            return;
        }
        filter.set(value);
        for tab in &tabs {
            tab.remove_css_class("recording-editor-media-tab-active");
        }
        match value {
            LibraryFilter::Video => tabs[0].add_css_class("recording-editor-media-tab-active"),
            LibraryFilter::Audio => tabs[1].add_css_class("recording-editor-media-tab-active"),
            LibraryFilter::Image => tabs[2].add_css_class("recording-editor-media-tab-active"),
            LibraryFilter::Zoom => tabs[3].add_css_class("recording-editor-media-tab-active"),
        }
        upload.set_visible(!matches!(value, LibraryFilter::Zoom));
        reload();
    });
}

fn build_upload_dropzone() -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-media-upload");
    button.set_hexpand(true);
    button.set_tooltip_text(Some("Drag here or click to upload"));

    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);

    let icon = Image::from_icon_name("go-up-symbolic");
    icon.add_css_class("recording-editor-media-upload-icon");
    icon.set_pixel_size(18);

    let title = Label::new(Some("Drag here"));
    title.add_css_class("recording-editor-media-upload-title");

    let hint = Label::new(Some("Or click to upload"));
    hint.add_css_class("recording-editor-media-upload-hint");

    content.append(&icon);
    content.append(&title);
    content.append(&hint);
    button.set_child(Some(&content));
    button
}

fn wire_upload_dropzone(
    upload: &Button,
    state: Option<Arc<Mutex<VideoEditState>>>,
    empty_open: Option<EmptyOpenHooks>,
    filter: Rc<Cell<LibraryFilter>>,
    reload: Rc<dyn Fn()>,
) {
    if let Some(empty_open) = &empty_open {
        *empty_open.open_button_slot.borrow_mut() = Some(upload.clone());
    }

    upload.connect_clicked({
        let state = state.clone();
        let empty_open = empty_open.clone();
        let filter = filter.clone();
        let reload = reload.clone();
        move |button| match filter.get() {
            LibraryFilter::Zoom => {}
            kind => {
                if let Some(state) = &state {
                    import_into_project(button, state.clone(), reload.clone(), kind);
                } else if let Some(empty_open) = &empty_open {
                    if kind == LibraryFilter::Video {
                        (empty_open.on_click)();
                    } else {
                        import_into_library(button, kind);
                    }
                }
            }
        }
    });

    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop({
        let state = state.clone();
        let empty_open = empty_open.clone();
        let filter = filter.clone();
        let reload = reload.clone();
        move |_, value, _x, _y| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            match filter.get() {
                LibraryFilter::Zoom => false,
                kind => {
                    if let Some(state) = &state {
                        if let Some(item) = project_item_from_path(path, kind) {
                            state.lock().unwrap().add_project_media(item);
                            reload();
                            true
                        } else {
                            false
                        }
                    } else if kind == LibraryFilter::Video {
                        if is_video_path(&path) {
                            if let Some(empty_open) = &empty_open {
                                (empty_open.on_drop_video)(path);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
        }
    });
    upload.add_controller(drop_target);
}

fn kind_tab(icon_name: &str, label: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-media-tab");
    button.set_hexpand(true);
    button.set_tooltip_text(Some(label));
    let col = GtkBox::new(Orientation::Vertical, 2);
    col.set_halign(Align::Center);
    col.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    let text = Label::new(Some(label));
    col.append(&icon);
    col.append(&text);
    button.set_child(Some(&col));
    button
}

fn populate_list(
    list: &GtkBox,
    empty: &Label,
    items: &[ProjectMedia],
    zooms: &[ZoomClip],
    filter: LibraryFilter,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if filter == LibraryFilter::Zoom {
        empty.set_visible(zooms.is_empty());
        empty.set_text("No zooms");
        for (index, clip) in zooms.iter().enumerate() {
            list.append(&build_zoom_row(index, clip));
        }
        return;
    }

    empty.set_visible(false);
    let filtered: Vec<&ProjectMedia> = items
        .iter()
        .filter(|item| match filter {
            LibraryFilter::Video => item.kind == ProjectMediaKind::Video,
            LibraryFilter::Audio => item.kind == ProjectMediaKind::Audio,
            LibraryFilter::Image => item.kind == ProjectMediaKind::Image,
            LibraryFilter::Zoom => false,
        })
        .collect();
    for item in filtered {
        list.append(&build_row(item));
    }
}

fn build_zoom_row(index: usize, clip: &ZoomClip) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-media-row");
    row.set_hexpand(true);

    let thumb = GtkBox::new(Orientation::Vertical, 0);
    thumb.add_css_class("recording-editor-media-thumb");
    thumb.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
    thumb.set_halign(Align::Start);
    thumb.set_valign(Align::Center);
    let icon = Image::from_icon_name("zoom-in-symbolic");
    icon.add_css_class("recording-editor-media-kind-icon");
    icon.set_pixel_size(14);
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    icon.set_hexpand(true);
    icon.set_vexpand(true);
    thumb.append(&icon);
    row.append(&thumb);

    let name = Label::new(Some(&format!("Zoom {} · {:.1}×", index + 1, clip.scale)));
    name.add_css_class("recording-editor-media-title");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row.append(&name);

    let meta = Label::new(Some(&format!(
        "{}–{}",
        format_duration(clip.start),
        format_duration(clip.end)
    )));
    meta.add_css_class("recording-editor-media-meta");
    meta.set_halign(Align::End);
    meta.set_valign(Align::Center);
    row.append(&meta);
    row
}

fn build_row(item: &ProjectMedia) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-media-row");
    row.set_hexpand(true);

    let thumb = GtkBox::new(Orientation::Vertical, 0);
    thumb.add_css_class("recording-editor-media-thumb");
    thumb.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
    thumb.set_halign(Align::Start);
    thumb.set_valign(Align::Center);
    thumb.set_hexpand(false);
    thumb.set_overflow(gtk4::Overflow::Hidden);

    match item.kind {
        ProjectMediaKind::Audio => {
            let icon = Image::from_icon_name("audio-x-generic-symbolic");
            icon.add_css_class("recording-editor-media-kind-icon");
            icon.set_pixel_size(14);
            icon.set_halign(Align::Center);
            icon.set_valign(Align::Center);
            icon.set_hexpand(true);
            icon.set_vexpand(true);
            thumb.append(&icon);
        }
        ProjectMediaKind::Video | ProjectMediaKind::Image => {
            let picture = Picture::new();
            picture.set_can_shrink(true);
            picture.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
            picture.set_hexpand(true);
            picture.set_vexpand(true);
            thumb.append(&picture);
            load_thumbnail(&picture, item);
        }
    }
    row.append(&thumb);

    let name = Label::new(Some(&item.display_name));
    name.add_css_class("recording-editor-media-title");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row.append(&name);

    if let Some(duration) = item.duration_seconds {
        let meta = Label::new(Some(&format_duration(duration)));
        meta.add_css_class("recording-editor-media-meta");
        meta.set_halign(Align::End);
        meta.set_valign(Align::Center);
        row.append(&meta);
    }

    row
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
    kind: LibraryFilter,
) {
    let Some(chooser) = media_chooser(button, kind, "Add to project", "Add") else {
        return;
    };
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
            if let Some(item) = project_item_from_path(path, kind) {
                state.add_project_media(item);
            }
        }
        drop(state);
        reload();
    });
    chooser.show();
}

fn import_into_library(button: &Button, kind: LibraryFilter) {
    let Some(chooser) = media_chooser(button, kind, "Upload media", "Open") else {
        return;
    };
    chooser.show();
}

fn media_chooser(
    button: &Button,
    kind: LibraryFilter,
    title: &str,
    accept: &str,
) -> Option<gtk4::FileChooserNative> {
    let root = button.root()?;
    let window = root.downcast::<gtk4::Window>().ok();
    let chooser = gtk4::FileChooserNative::new(
        Some(title),
        window.as_ref(),
        gtk4::FileChooserAction::Open,
        Some(accept),
        Some("Cancel"),
    );
    chooser.set_select_multiple(true);
    chooser.add_filter(&media_filter(kind));
    Some(chooser)
}

fn media_filter(kind: LibraryFilter) -> gtk4::FileFilter {
    let filter = gtk4::FileFilter::new();
    match kind {
        LibraryFilter::Video => {
            filter.set_name(Some("Video"));
            filter.add_mime_type("video/mp4");
            filter.add_pattern("*.mp4");
        }
        LibraryFilter::Audio => {
            filter.set_name(Some("Audio"));
            for mime in [
                "audio/mpeg",
                "audio/wav",
                "audio/x-wav",
                "audio/ogg",
                "audio/flac",
            ] {
                filter.add_mime_type(mime);
            }
            for pattern in ["*.mp3", "*.wav", "*.ogg", "*.flac", "*.m4a"] {
                filter.add_pattern(pattern);
            }
        }
        LibraryFilter::Image => {
            filter.set_name(Some("Images"));
            for mime in ["image/png", "image/jpeg", "image/webp"] {
                filter.add_mime_type(mime);
            }
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
        }
        LibraryFilter::Zoom => {
            filter.set_name(Some("Media"));
        }
    }
    filter
}

fn project_item_from_path(path: PathBuf, filter: LibraryFilter) -> Option<ProjectMedia> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let display_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Media")
        .to_string();
    let kind = match filter {
        LibraryFilter::Video if matches!(ext.as_str(), "mp4" | "webm" | "gif") => {
            ProjectMediaKind::Video
        }
        LibraryFilter::Audio if matches!(ext.as_str(), "mp3" | "wav" | "ogg" | "flac" | "m4a") => {
            ProjectMediaKind::Audio
        }
        LibraryFilter::Image if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") => {
            ProjectMediaKind::Image
        }
        _ => return None,
    };
    let duration_seconds = if kind == ProjectMediaKind::Video {
        crate::recording::editor::ffmpeg::probe_metadata(&path)
            .ok()
            .map(|metadata| metadata.duration_seconds)
    } else {
        None
    };
    Some(ProjectMedia {
        path,
        display_name,
        kind,
        duration_seconds,
    })
}

fn is_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("mp4"))
        .unwrap_or(false)
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let secs = (seconds - minutes as f64 * 60.0).floor() as u64;
    format!("{minutes:02}:{secs:02}")
}

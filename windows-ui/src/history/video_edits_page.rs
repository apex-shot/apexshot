//! The History window's Video Edits page.
//!
//! One card per saved edit list: the recording is the thumbnail and the title,
//! the project behind it is what the card opens. The page exists in both
//! histories, and on Windows it is what the video editor's autosave feeds —
//! see [`crate::video_editor::project`].
//!
//! The shell, the grid, the card metrics and the empty state follow the
//! Screenshots and Recordings pages, because a page in this window has one
//! visual contract.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, FlowBox, Image, Label, Orientation, Overlay, Picture,
    ScrolledWindow, SelectionMode,
};

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::video_editor::SavedEdit;

use super::local_page::{CARD_THUMB_HEIGHT, CARD_THUMB_WIDTH};
use super::scan::{self, MediaKind};
use super::thumbnails;
use super::window::{HistoryToast, ToastKind};
use super::HistoryPage;

/// Everything a card needs, gathered off the main thread.
struct CardData {
    name: String,
    source: PathBuf,
    tooltip: String,
    thumbnail: Option<PathBuf>,
    search_key: String,
}

impl CardData {
    fn new(edit: SavedEdit, now: SystemTime) -> Self {
        let entry = capture_entry(&edit.source_path);
        let thumbnail = entry
            .as_ref()
            .and_then(|entry| thumbnails::thumbnail_for_entry(entry).ok());
        let modified = entry.as_ref().and_then(|entry| entry.modified);
        let size_bytes = entry.as_ref().map(|entry| entry.size_bytes).unwrap_or(0);
        Self {
            tooltip: format!(
                "{}\n{} · {}",
                edit.name,
                scan::format_relative_time(modified, now),
                scan::format_size(size_bytes)
            ),
            search_key: edit.name.to_ascii_lowercase(),
            name: edit.name,
            source: edit.source_path,
            thumbnail,
        }
    }
}

/// The video behind a saved edit as a capture entry, which is the shape the
/// shared thumbnail cache speaks.
fn capture_entry(path: &Path) -> Option<scan::CaptureEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    let display_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(scan::CaptureEntry {
        path: path.to_path_buf(),
        display_name,
        modified: metadata.modified().ok(),
        size_bytes: metadata.len(),
        kind: MediaKind::Video,
    })
}

struct PageState {
    grid: FlowBox,
    empty_state: GtkBox,
    search: Entry,
    toast: HistoryToast,
    cards: RefCell<Vec<(Button, String)>>,
    generation: Cell<u64>,
}

pub fn build_video_edits_page(toast: HistoryToast, search: &Entry) -> HistoryPage {
    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let column = GtkBox::new(Orientation::Vertical, 0);
    let scale = super::window_scale();
    column.set_margin_top((2.0 * scale).round() as i32);
    column.set_margin_bottom((24.0 * scale).round() as i32);
    let side = (28.0 * scale).round() as i32;
    column.set_margin_start(side);
    column.set_margin_end(side);

    let grid = FlowBox::new();
    grid.add_css_class("recent-captures-grid");
    grid.set_selection_mode(SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_row_spacing(14);
    grid.set_column_spacing(14);
    grid.set_max_children_per_line(8);
    grid.set_min_children_per_line(1);
    grid.set_valign(Align::Start);
    column.append(&grid);

    let empty_state = build_empty_state();
    empty_state.set_visible(false);
    column.append(&empty_state);

    scroller.set_child(Some(&column));

    let state = Rc::new(PageState {
        grid,
        empty_state,
        search: search.clone(),
        toast,
        cards: RefCell::new(Vec::new()),
        generation: Cell::new(0),
    });

    {
        let state = Rc::clone(&state);
        search.connect_changed(move |entry| apply_filter(&state, &entry.text()));
    }

    let refresh = {
        let state = Rc::clone(&state);
        Rc::new(move || reload(&state)) as Rc<dyn Fn()>
    };

    // The list is read when the page is first shown, so opening the window
    // never pays for pages that stay unvisited.
    {
        let state = Rc::clone(&state);
        let done = Cell::new(false);
        scroller.connect_map(move |_| {
            if done.replace(true) {
                return;
            }
            reload(&state);
        });
    }

    HistoryPage {
        widget: scroller.upcast(),
        refresh,
        search_placeholder: t("Search video edits"),
        searchable: true,
    }
}

fn build_empty_state() -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Center);
    empty.set_vexpand(true);

    let icon = Image::from_icon_name(icon_names::custom::RECORD_SCREEN_SYMBOLIC);
    icon.set_pixel_size(48);
    icon.add_css_class("history-empty-icon");
    icon.set_halign(Align::Center);
    empty.append(&icon);

    let detail = Label::new(Some(&t(
        "Edits you save in the video editor will show up here.",
    )));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_halign(Align::Center);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    detail.set_justify(gtk4::Justification::Center);
    empty.append(&detail);

    empty
}

/// Read the saved edit lists and rebuild the grid. Listing and thumbnailing
/// touch the disk, so both run off the main thread and hand back finished cards.
fn reload(state: &Rc<PageState>) {
    let generation = state.generation.get().wrapping_add(1);
    state.generation.set(generation);

    while let Some(child) = state.grid.first_child() {
        state.grid.remove(&child);
    }
    state.cards.borrow_mut().clear();
    state.empty_state.set_visible(false);
    state.grid.set_visible(true);

    let (sender, receiver) = mpsc::channel::<Vec<CardData>>();
    std::thread::spawn(move || {
        let now = SystemTime::now();
        let cards = crate::video_editor::saved_edits()
            .into_iter()
            .map(|edit| CardData::new(edit, now))
            .collect();
        let _ = sender.send(cards);
    });

    let receiver = Rc::new(receiver);
    let state = Rc::clone(state);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(cards) => {
                if state.generation.get() == generation {
                    populate(&state, cards);
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn populate(state: &Rc<PageState>, cards: Vec<CardData>) {
    if cards.is_empty() {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        return;
    }

    let mut built = state.cards.borrow_mut();
    for card in cards {
        let button = build_card(state, &card);
        state.grid.insert(&button, -1);
        built.push((button, card.search_key));
    }
    drop(built);

    apply_filter(state, &state.search.text());
}

fn build_card(state: &Rc<PageState>, card: &CardData) -> Button {
    let clickable = Button::new();
    clickable.add_css_class("recent-captures-card");
    clickable.set_halign(Align::Fill);
    clickable.set_tooltip_text(Some(&card.tooltip));

    let content = GtkBox::new(Orientation::Vertical, 0);

    let image_wrap = Overlay::new();
    image_wrap.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
    image_wrap.set_halign(Align::Center);

    let placeholder = GtkBox::new(Orientation::Vertical, 0);
    placeholder.add_css_class("recent-captures-card-image");
    placeholder.add_css_class("recent-captures-picture-missing");
    placeholder.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);

    let placeholder_icon = Image::from_icon_name(icon_names::custom::RECORD_SCREEN_SYMBOLIC);
    placeholder_icon.set_pixel_size(32);
    placeholder_icon.add_css_class("history-card-placeholder");
    placeholder_icon.set_vexpand(true);
    placeholder_icon.set_valign(Align::Center);
    placeholder.append(&placeholder_icon);
    image_wrap.set_child(Some(&placeholder));

    if let Some(thumbnail) = &card.thumbnail {
        let picture = Picture::for_filename(thumbnail);
        picture.add_css_class("recent-captures-card-image");
        picture.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
        image_wrap.add_overlay(&picture);
    }

    content.append(&image_wrap);

    let title = Label::new(Some(&card.name));
    title.add_css_class("recent-captures-card-title");
    title.set_halign(Align::Fill);
    title.set_justify(gtk4::Justification::Center);
    title.set_wrap(true);
    title.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    title.set_lines(2);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(18);
    content.append(&title);

    clickable.set_child(Some(&content));

    {
        let state = Rc::clone(state);
        let source = card.source.clone();
        clickable.connect_clicked(move |_| {
            report(&state, super::actions::launch_video_editor(&source));
        });
    }

    clickable
}

fn apply_filter(state: &Rc<PageState>, query: &str) {
    let query = query.trim().to_ascii_lowercase();
    let cards = state.cards.borrow();
    for (button, search_key) in cards.iter() {
        button.set_visible(query.is_empty() || search_key.contains(&query));
    }
    // The empty state explains a page with nothing saved in it. A search that
    // matched nothing is a filtered grid, as on the other pages, not an
    // invitation to go and make an edit.
    let none_saved = cards.is_empty();
    drop(cards);
    state.grid.set_visible(!none_saved);
    state.empty_state.set_visible(none_saved);
}

/// Route an action outcome to the shared window toast.
fn report(state: &Rc<PageState>, result: Result<String, String>) {
    match result {
        Ok(message) => {
            state
                .toast
                .show(&message, ToastKind::Success, Some(Duration::from_secs(2)));
        }
        Err(message) => {
            state
                .toast
                .show(&message, ToastKind::Error, Some(Duration::from_secs(4)));
        }
    }
}

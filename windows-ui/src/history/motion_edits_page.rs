//! The History window's Motion Edits page on Windows.
//!
//! Two kinds of card share the grid: a saved Motion project, which opens its
//! image in the editor with the Motion edit restored, and a Motion video the
//! editor exported, which plays in the default app. Exported Motion videos
//! list here instead of under Recordings — see [`super::motion_exports`].
//!
//! The shell, the grid, the card metrics and the empty state follow the
//! Video Edits page.

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

use super::local_page::{CARD_THUMB_HEIGHT, CARD_THUMB_WIDTH};
use super::scan::{self, CaptureEntry, MediaKind};
use super::thumbnails;
use super::window::{HistoryToast, ToastKind};
use super::HistoryPage;

/// What a card opens.
enum CardTarget {
    /// A saved Motion project: reopen its image in the editor.
    Project(PathBuf),
    /// An exported Motion video: play it.
    Export(CaptureEntry),
}

/// Everything a card needs, gathered off the main thread.
struct CardData {
    name: String,
    target: CardTarget,
    tooltip: String,
    thumbnail: Option<PathBuf>,
    search_key: String,
    modified: Option<SystemTime>,
}

fn image_entry(path: &Path) -> Option<CaptureEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    let display_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(CaptureEntry {
        path: path.to_path_buf(),
        display_name,
        modified: metadata.modified().ok(),
        size_bytes: metadata.len(),
        kind: MediaKind::Image,
    })
}

/// Saved projects and exported videos, newest first.
fn gather(now: SystemTime) -> Vec<CardData> {
    let mut cards = Vec::new();
    for project in crate::capture::editor::motion_project::list_projects() {
        let entry = image_entry(&project.source_path);
        let modified = entry.as_ref().and_then(|entry| entry.modified);
        let thumbnail = entry
            .as_ref()
            .and_then(|entry| thumbnails::thumbnail_for_entry(entry).ok());
        let name = project.display_name();
        cards.push(CardData {
            tooltip: format!(
                "{name}\n{} · {}",
                t("Motion edit"),
                scan::format_relative_time(modified, now)
            ),
            search_key: name.to_lowercase(),
            name,
            target: CardTarget::Project(project.source_path.clone()),
            thumbnail,
            modified,
        });
    }
    for entry in super::motion_exports::list_exports() {
        let thumbnail = thumbnails::thumbnail_for_entry(&entry).ok();
        cards.push(CardData {
            tooltip: format!(
                "{}\n{} · {} · {}",
                entry.display_name,
                t("Exported Motion video"),
                scan::format_relative_time(entry.modified, now),
                scan::format_size(entry.size_bytes)
            ),
            search_key: entry.display_name.to_lowercase(),
            name: entry.display_name.clone(),
            modified: entry.modified,
            target: CardTarget::Export(entry),
            thumbnail,
        });
    }
    cards.sort_by(|a, b| b.modified.cmp(&a.modified));
    cards
}

struct PageState {
    grid: FlowBox,
    skeleton: FlowBox,
    empty_state: GtkBox,
    search: Entry,
    toast: HistoryToast,
    cards: RefCell<Vec<(Button, String)>>,
    generation: Cell<u64>,
}

pub fn build_motion_edits_page(toast: HistoryToast, search: &Entry) -> HistoryPage {
    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let column = GtkBox::new(Orientation::Vertical, 0);
    let scale = super::window_scale().min(1.0);
    column.set_margin_top((12.0 * scale).round() as i32);
    column.set_margin_bottom((32.0 * scale).round() as i32);
    column.set_margin_start((20.0 * scale).round() as i32);
    column.set_margin_end((28.0 * scale).round() as i32);

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

    let skeleton = super::local_page::build_loading_skeleton();
    skeleton.set_visible(false);
    column.append(&skeleton);

    let empty_state = build_empty_state();
    empty_state.set_visible(false);
    column.append(&empty_state);

    scroller.set_child(Some(&column));

    let state = Rc::new(PageState {
        grid,
        skeleton,
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
        search_placeholder: t("Search motion edits"),
        searchable: true,
    }
}

fn build_empty_state() -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Center);
    empty.set_vexpand(true);

    let icon = Image::from_icon_name(icon_names::custom::SPLINE_SYMBOLIC);
    icon.set_pixel_size(48);
    icon.add_css_class("history-empty-icon");
    icon.set_halign(Align::Center);
    empty.append(&icon);

    let detail = Label::new(Some(&t(
        "Motion edits you make and Motion videos you export will show up here.",
    )));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_halign(Align::Center);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    detail.set_justify(gtk4::Justification::Center);
    empty.append(&detail);

    empty
}

/// Read the projects and exports and rebuild the grid. Listing and
/// thumbnailing touch the disk, so both run off the main thread.
fn reload(state: &Rc<PageState>) {
    let generation = state.generation.get().wrapping_add(1);
    state.generation.set(generation);

    while let Some(child) = state.grid.first_child() {
        state.grid.remove(&child);
    }
    state.cards.borrow_mut().clear();
    state.empty_state.set_visible(false);
    state.grid.set_visible(false);
    state.skeleton.set_visible(true);

    let (sender, receiver) = mpsc::channel::<Vec<CardData>>();
    std::thread::spawn(move || {
        let _ = sender.send(gather(SystemTime::now()));
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
    state.skeleton.set_visible(false);
    if cards.is_empty() {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        return;
    }
    state.grid.set_visible(true);

    let mut built = state.cards.borrow_mut();
    for card in cards {
        let search_key = card.search_key.clone();
        let button = build_card(state, card);
        state.grid.insert(&button, -1);
        built.push((button, search_key));
    }
    drop(built);

    apply_filter(state, &state.search.text());
}

fn build_card(state: &Rc<PageState>, card: CardData) -> Button {
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

    let placeholder_icon = Image::from_icon_name(icon_names::custom::SPLINE_SYMBOLIC);
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

    // The same badge the Recordings page puts on a video, so an exported
    // Motion video reads apart from a saved edit at a glance.
    if matches!(card.target, CardTarget::Export(_)) {
        let badge = Image::from_icon_name(icon_names::custom::RECORD_SCREEN_SYMBOLIC);
        badge.add_css_class("history-media-badge");
        badge.set_pixel_size(12);
        badge.set_halign(Align::End);
        badge.set_valign(Align::End);
        badge.set_margin_end(8);
        badge.set_margin_bottom(8);
        image_wrap.add_overlay(&badge);
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
        clickable.connect_clicked(move |_| {
            let result = match &card.target {
                CardTarget::Project(source) => super::actions::launch_image_editor(source),
                CardTarget::Export(entry) => super::actions::open_in_default_app(entry),
            };
            report(&state, result);
        });
    }

    clickable
}

fn apply_filter(state: &Rc<PageState>, query: &str) {
    let query = query.trim().to_lowercase();
    let cards = state.cards.borrow();
    for (button, search_key) in cards.iter() {
        button.set_visible(query.is_empty() || search_key.contains(&query));
    }
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

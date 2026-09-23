//! Edit pages for the History window: saved Motion edits and saved video edits.
//!
//! Both pages are the same listing over a different project directory — the
//! Motion sidecar written by [`crate::capture::editor::motion_project`] and the
//! recording editor's `video-projects` files — so one builder covers both.
//!
//! Unlike the Screenshots/Recordings pages, which list files on disk, these
//! list *edits*: a card stands for a saved project and opens it back up in its
//! editor. The source file is what gets thumbnailed and revealed; the project
//! is what gets deleted, so removing an edit never touches the capture itself.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, Entry, FlowBox, Image, Label, Orientation,
    Picture, Popover, ScrolledWindow, SelectionMode,
};

use super::scan::{self, CaptureEntry, MediaKind};
use super::thumbnails::{self, ThumbnailReady, ThumbnailRequest, ThumbnailSource};
use super::window::{HistoryToast, ToastKind};
use crate::i18n::{t, tfmt};

use super::local_page::{CARD_THUMB_HEIGHT, CARD_THUMB_WIDTH};

/// Which editor's saved edits a page lists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditsKind {
    Motion,
    Video,
}

impl EditsKind {
    /// Page title, also used as the sidebar label.
    fn title(self) -> String {
        match self {
            Self::Motion => t("Motion Edits"),
            Self::Video => t("Video Edits"),
        }
    }

    fn search_placeholder(self) -> String {
        match self {
            Self::Motion => t("Search motion edits"),
            Self::Video => t("Search video edits"),
        }
    }

    fn empty_title(self) -> String {
        match self {
            Self::Motion => t("No motion edits yet"),
            Self::Video => t("No video edits yet"),
        }
    }

    fn empty_detail(self) -> String {
        match self {
            Self::Motion => t("Motion edits you make will show up here."),
            Self::Video => t("Video edits you make will show up here."),
        }
    }

    /// Subtitle count line; the noun phrase is per kind so the string
    /// translates as a unit rather than being assembled from fragments.
    fn count_line(self, count: usize) -> String {
        let count = count.to_string();
        match self {
            Self::Motion => tfmt("{count} motion edits", &[("count", &count)]),
            Self::Video => tfmt("{count} video edits", &[("count", &count)]),
        }
    }

    /// Every saved project of this kind, newest source first.
    fn list(self) -> Vec<EditEntry> {
        match self {
            Self::Motion => crate::capture::editor::motion_project::list_projects()
                .into_iter()
                .map(|project| EditEntry {
                    source_path: project.source_path.clone(),
                    display_name: project.display_name(),
                    detail: t("Motion edit"),
                })
                .collect(),
            Self::Video => crate::recording::editor::project::list_projects()
                .into_iter()
                .map(|project| EditEntry {
                    source_path: project.source_path.clone(),
                    display_name: project.display_name(),
                    detail: t("Video edit"),
                })
                .collect(),
        }
    }

    /// Remove this project's sidecar, keeping the source file. Returns the
    /// path to the project file so the card can be matched after deletion.
    fn delete_project(self, source_path: &std::path::Path) {
        match self {
            Self::Motion => crate::capture::editor::motion_project::delete_project(source_path),
            Self::Video => crate::recording::editor::project::delete_project(source_path),
        }
    }
}

/// One saved edit, flattened from whichever project type backs the page.
#[derive(Clone)]
pub struct EditEntry {
    /// The capture the edit was authored against.
    pub source_path: PathBuf,
    /// Title for the card, from the project's reserved `title` field.
    pub display_name: String,
    /// Short description of what kind of edit this is, for the tooltip.
    pub detail: String,
}

impl EditEntry {
    /// Lowercase name used by the search box.
    fn search_key(&self) -> String {
        self.display_name.to_ascii_lowercase()
    }

    /// The source as a `CaptureEntry`, so the existing thumbnail pool and
    /// action plumbing can be reused unchanged.
    fn as_capture(&self) -> CaptureEntry {
        let metadata = std::fs::metadata(&self.source_path).ok();
        let display_name = self
            .source_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        CaptureEntry {
            path: self.source_path.clone(),
            display_name,
            modified: metadata.as_ref().and_then(|meta| meta.modified().ok()),
            size_bytes: metadata.as_ref().map(|meta| meta.len()).unwrap_or(0),
            kind: self.source_kind_hint(),
        }
    }

    /// Filled in by the page builder, which knows the page's kind.
    fn source_kind_hint(&self) -> MediaKind {
        // A video source is an mp4/webm/gif; everything else is a still.
        let extension = self
            .source_path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if scan::VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Video
        } else {
            MediaKind::Image
        }
    }
}

/// One card plus the metadata needed to filter it.
struct Card {
    id: u64,
    root: gtk4::FlowBoxChild,
    entry: EditEntry,
    search_key: String,
    picture: Picture,
    placeholder: GtkBox,
}

/// Shared per-page state, kept alive by the closures wired below.
struct PageState {
    kind: EditsKind,
    grid: FlowBox,
    empty_state: GtkBox,
    scroller: ScrolledWindow,
    subtitle: Label,
    search: Entry,
    cards: RefCell<Vec<Rc<Card>>>,
    /// Current listing batch. Bumped on refresh so stale deliveries drop.
    generation: Cell<u64>,
    next_card_id: Cell<u64>,
    toast: HistoryToast,
}

/// Build a Motion Edits or Video Edits page.
pub fn build_edits_page(
    kind: EditsKind,
    toast: HistoryToast,
    search: &Entry,
) -> super::HistoryPage {
    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let column = GtkBox::new(Orientation::Vertical, 0);
    column.set_margin_top(20);
    column.set_margin_bottom(32);
    column.set_margin_start(28);
    column.set_margin_end(28);

    let header = GtkBox::new(Orientation::Vertical, 0);

    let title = Label::new(Some(&kind.title()));
    title.add_css_class("recent-captures-title");
    title.set_halign(Align::Start);

    let subtitle = Label::new(Some(&t("Loading…")));
    subtitle.add_css_class("history-page-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_margin_bottom(18);

    header.append(&title);
    header.append(&subtitle);
    column.append(&header);

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

    let empty_state = build_empty_state(kind);
    empty_state.set_visible(false);
    column.append(&empty_state);

    scroller.set_child(Some(&column));

    let state = Rc::new(PageState {
        kind,
        grid,
        empty_state,
        scroller: scroller.clone(),
        subtitle,
        search: search.clone(),
        cards: RefCell::new(Vec::new()),
        generation: Cell::new(0),
        next_card_id: Cell::new(0),
        toast,
    });

    {
        let state = Rc::clone(&state);
        search.connect_changed(move |entry| {
            apply_filter(&state, &entry.text());
        });
    }

    let refresh = {
        let state = Rc::clone(&state);
        Rc::new(move || reload(&state)) as Rc<dyn Fn()>
    };

    // Populate once the page is actually shown, so opening the window does
    // not pay for pages the user never visits.
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

    super::HistoryPage {
        widget: scroller.upcast(),
        refresh,
        search_placeholder: kind.search_placeholder(),
        searchable: true,
    }
}

fn build_empty_state(kind: EditsKind) -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 0);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Start);
    empty.set_margin_top(24);

    let icon = Image::from_icon_name(match kind {
        EditsKind::Motion => {
            crate::capture::editor::window::icon_names::custom::SCREENSHOOTER_SYMBOLIC
        }
        EditsKind::Video => {
            crate::capture::editor::window::icon_names::custom::RECORD_SCREEN_SYMBOLIC
        }
    });
    icon.set_pixel_size(48);
    icon.add_css_class("history-empty-icon");
    icon.set_halign(Align::Center);
    empty.append(&icon);

    let title = Label::new(Some(&kind.empty_title()));
    title.add_css_class("recent-captures-empty-title");

    let detail = Label::new(Some(&kind.empty_detail()));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_halign(Align::Center);

    empty.append(&title);
    empty.append(&detail);
    empty
}

/// Clear the grid, bump the generation, and re-list on a worker thread.
fn reload(state: &Rc<PageState>) {
    let previous = state.generation.get();
    if previous != 0 {
        thumbnails::cancel_generation(previous);
    }
    let generation = thumbnails::next_generation();
    state.generation.set(generation);

    for card in state.cards.borrow().iter() {
        state.grid.remove(&card.root);
    }
    state.cards.borrow_mut().clear();
    state.empty_state.set_visible(false);
    state.grid.set_visible(true);
    state.subtitle.set_visible(true);
    state.subtitle.set_text(&t("Loading…"));

    let kind = state.kind;
    let (scan_tx, scan_rx) = mpsc::channel::<Vec<EditEntry>>();
    std::thread::spawn(move || {
        let _ = scan_tx.send(kind.list());
    });

    let (thumb_tx, thumb_rx) = mpsc::channel::<ThumbnailReady>();
    let thumb_rx = Rc::new(thumb_rx);

    {
        let state = Rc::clone(state);
        let thumb_tx = thumb_tx.clone();
        let thumb_rx = Rc::clone(&thumb_rx);
        glib::source::idle_add_local(move || match scan_rx.try_recv() {
            Ok(entries) => {
                if state.generation.get() != generation {
                    return glib::ControlFlow::Break;
                }
                populate(&state, generation, entries, &thumb_tx);
                start_thumbnail_drain(&state, generation, Rc::clone(&thumb_rx));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }
}

/// Build the cards for `entries` and submit a thumbnail job per card.
fn populate(
    state: &Rc<PageState>,
    generation: u64,
    entries: Vec<EditEntry>,
    thumb_tx: &mpsc::Sender<ThumbnailReady>,
) {
    if entries.is_empty() {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        state.subtitle.set_visible(false);
        return;
    }

    state
        .subtitle
        .set_text(&state.kind.count_line(entries.len()));
    state.subtitle.set_visible(true);

    let now = SystemTime::now();
    let mut cards = state.cards.borrow_mut();
    for entry in entries {
        let id = state.next_card_id.get();
        state.next_card_id.set(id + 1);

        let card = build_card(state, id, &entry, now);
        state.grid.insert(&card.root, -1);

        thumbnails::submit(ThumbnailRequest {
            id,
            generation,
            source: ThumbnailSource::Local(entry.as_capture()),
            reply: thumb_tx.clone(),
        });

        cards.push(Rc::new(card));
    }
    drop(cards);

    apply_filter(state, &state.search.text());
}

/// Build a single card. `id` is the thumbnail-request id echoed back on
/// delivery so the finished image lands on the right card.
fn build_card(state: &Rc<PageState>, id: u64, entry: &EditEntry, now: SystemTime) -> Card {
    let source = entry.as_capture();

    let card_box = GtkBox::new(Orientation::Vertical, 0);
    card_box.set_halign(Align::Fill);

    let clickable = Button::new();
    clickable.add_css_class("recent-captures-card");
    clickable.set_halign(Align::Fill);

    let content = GtkBox::new(Orientation::Vertical, 0);

    let image_wrap = gtk4::Overlay::new();
    image_wrap.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
    image_wrap.set_halign(Align::Center);

    let placeholder = GtkBox::new(Orientation::Vertical, 0);
    placeholder.add_css_class("recent-captures-card-image");
    placeholder.add_css_class("recent-captures-picture-missing");
    placeholder.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);

    let placeholder_icon = Image::from_icon_name(match source.kind {
        MediaKind::Image => {
            crate::capture::editor::window::icon_names::custom::SCREENSHOOTER_SYMBOLIC
        }
        MediaKind::Video => {
            crate::capture::editor::window::icon_names::custom::RECORD_SCREEN_SYMBOLIC
        }
    });
    placeholder_icon.set_pixel_size(32);
    placeholder_icon.add_css_class("history-card-placeholder");
    placeholder_icon.set_vexpand(true);
    placeholder_icon.set_valign(Align::Center);
    placeholder.append(&placeholder_icon);

    let picture = Picture::new();
    picture.add_css_class("recent-captures-card-image");
    picture.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
    picture.set_visible(false);

    image_wrap.set_child(Some(&placeholder));
    image_wrap.add_overlay(&picture);

    // An "Edited" badge marks these as saved projects rather than plain
    // captures, so the page reads differently from Screenshots/Recordings.
    let badge = Label::new(Some(&t("Edited")));
    badge.add_css_class("history-edited-badge");
    badge.set_halign(Align::End);
    badge.set_valign(Align::End);
    badge.set_margin_end(6);
    badge.set_margin_bottom(6);
    image_wrap.add_overlay(&badge);

    content.append(&image_wrap);

    let title = Label::new(Some(&entry.display_name));
    title.add_css_class("recent-captures-card-title");
    title.set_halign(Align::Fill);
    title.set_justify(gtk4::Justification::Center);
    title.set_wrap(true);
    title.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    title.set_lines(2);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(18);
    content.append(&title);

    clickable.set_tooltip_text(Some(&format!(
        "{}\n{} · {}",
        entry.display_name,
        entry.detail,
        scan::format_relative_time(source.modified, now),
    )));

    clickable.set_child(Some(&content));
    card_box.append(&clickable);

    let child = gtk4::FlowBoxChild::new();
    child.set_child(Some(&card_box));

    // Double-click resumes the edit in its editor.
    {
        let state = Rc::clone(state);
        let source = source.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(gtk4::gdk::BUTTON_PRIMARY);
        gesture.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                report(&state, super::actions::open_in_apexshot_editor(&source));
            }
        });
        clickable.add_controller(gesture);
    }
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let source = source.clone();
        let anchor = clickable.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
        gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
        gesture.connect_pressed(move |_, _, _, _| {
            show_action_popover(&state, &entry, &source, &anchor);
        });
        clickable.add_controller(gesture);
    }

    Card {
        id,
        root: child,
        entry: entry.clone(),
        search_key: entry.search_key(),
        picture,
        placeholder,
    }
}

/// Drain finished thumbnails onto their cards until the generation changes.
fn start_thumbnail_drain(
    state: &Rc<PageState>,
    generation: u64,
    thumb_rx: Rc<mpsc::Receiver<ThumbnailReady>>,
) {
    let state = Rc::clone(state);
    glib::source::idle_add_local(move || {
        if state.generation.get() != generation {
            return glib::ControlFlow::Break;
        }
        loop {
            match thumb_rx.try_recv() {
                Ok(ready) => apply_thumbnail(&state, ready),
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
    });
}

/// Swap a card's placeholder for its finished thumbnail.
fn apply_thumbnail(state: &Rc<PageState>, ready: ThumbnailReady) {
    let Ok(path) = ready.result else {
        return;
    };
    let cards = state.cards.borrow();
    if let Some(card) = cards.iter().find(|card| card.id == ready.id) {
        set_card_image(card, &path);
    }
}

fn set_card_image(card: &Card, path: &PathBuf) {
    card.picture.set_filename(Some(path));
    card.picture.set_visible(true);
    card.placeholder.set_visible(false);
}

/// Show or hide cards against the search query.
fn apply_filter(state: &Rc<PageState>, needle: &str) {
    let needle = needle.trim().to_ascii_lowercase();
    let mut visible = 0usize;
    for card in state.cards.borrow().iter() {
        let matches = needle.is_empty() || card.search_key.contains(&needle);
        card.root.set_visible(matches);
        if matches {
            visible += 1;
        }
    }
    // The empty state reflects the *filtered* result, so a query that matches
    // nothing explains itself instead of showing a bare grid.
    if visible == 0 && !state.cards.borrow().is_empty() {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
    } else {
        state.grid.set_visible(true);
        state.empty_state.set_visible(false);
    }
}

fn show_action_popover(
    state: &Rc<PageState>,
    entry: &EditEntry,
    source: &CaptureEntry,
    anchor: &Button,
) {
    let popover = Popover::new();
    popover.add_css_class("history-action-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(anchor);

    let menu = GtkBox::new(Orientation::Vertical, 2);

    let add_action = |label_text: &str, destructive: bool| {
        let btn = Button::new();
        btn.add_css_class("history-action-btn");
        btn.set_focus_on_click(false);
        if destructive {
            btn.add_css_class("history-action-btn-destructive");
        }
        let label = Label::new(Some(label_text));
        label.set_halign(Align::Start);
        label.set_xalign(0.0);
        btn.set_child(Some(&label));
        menu.append(&btn);
        btn
    };

    let editor_btn = add_action(&t("Open in editor"), false);
    let reveal_btn = add_action(&t("Show in files"), false);

    let separator = gtk4::Separator::new(Orientation::Horizontal);
    separator.add_css_class("history-action-separator");
    menu.append(&separator);

    let delete_btn = add_action(&t("Delete edit"), true);

    popover.set_child(Some(&menu));
    popover.set_size_request(anchor.width(), -1);

    {
        let state = Rc::clone(state);
        let source = source.clone();
        let popover = popover.clone();
        editor_btn.connect_clicked(move |_| {
            report(&state, super::actions::open_in_apexshot_editor(&source));
            popover.popdown();
        });
    }
    {
        let state = Rc::clone(state);
        let source = source.clone();
        let popover = popover.clone();
        reveal_btn.connect_clicked(move |_| {
            report(&state, super::actions::reveal_in_file_manager(&source));
            popover.popdown();
        });
    }
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let popover = popover.clone();
        delete_btn.connect_clicked(move |_| {
            popover.popdown();
            confirm_delete(&state, &entry);
        });
    }

    popover.popup();
}

/// Confirm before discarding a saved edit. The source capture is untouched,
/// which the dialog says explicitly so "delete" is not read as data loss.
fn confirm_delete(state: &Rc<PageState>, entry: &EditEntry) {
    let root = state.scroller.root().and_downcast::<gtk4::Window>();
    let dialog = gtk4::MessageDialog::builder()
        .modal(true)
        .message_type(gtk4::MessageType::Warning)
        .buttons(gtk4::ButtonsType::None)
        .text(tfmt("Delete {name}?", &[("name", &entry.display_name)]))
        .secondary_text(t(
            "This removes the saved edit and keeps the original file.",
        ))
        .build();
    if let Some(window) = root {
        dialog.set_transient_for(Some(&window));
    }
    dialog.add_button(&t("Cancel"), gtk4::ResponseType::Cancel);
    let delete_response = dialog.add_button(&t("Delete"), gtk4::ResponseType::Accept);
    delete_response.add_css_class("recent-captures-primary-button");

    let state = Rc::clone(state);
    let entry = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Accept {
            state.kind.delete_project(&entry.source_path);
            remove_card(&state, &entry.source_path);
            state.toast.show(
                &t("Removed the saved edit"),
                ToastKind::Success,
                Some(Duration::from_secs(2)),
            );
        }
        dialog.close();
    });
    dialog.show();
}

/// Drop the card for `source_path` from the grid and the card list.
fn remove_card(state: &Rc<PageState>, source_path: &std::path::Path) {
    let mut cards = state.cards.borrow_mut();
    if let Some(pos) = cards
        .iter()
        .position(|card| card.entry.source_path == source_path)
    {
        let card = cards.remove(pos);
        state.grid.remove(&card.root);
    }
    let remaining = cards.len();
    drop(cards);
    if remaining == 0 {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        state.subtitle.set_visible(false);
    } else {
        state.subtitle.set_text(&state.kind.count_line(remaining));
        state.subtitle.set_visible(true);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, display_name: &str) -> EditEntry {
        EditEntry {
            source_path: PathBuf::from(source),
            display_name: display_name.to_string(),
            detail: "Motion edit".to_string(),
        }
    }

    #[test]
    fn search_key_is_lowercased() {
        assert_eq!(entry("/a/B.png", "My Zoom").search_key(), "my zoom");
    }

    #[test]
    fn a_video_source_is_detected_from_its_extension() {
        for name in ["clip.mp4", "clip.webm", "clip.gif", "CLIP.MP4"] {
            assert_eq!(
                entry(name, "x").source_kind_hint(),
                MediaKind::Video,
                "{name} should read as a video source"
            );
        }
    }

    #[test]
    fn an_image_source_is_detected_from_its_extension() {
        for name in ["shot.png", "shot.jpg", "shot.webp", "shot.unknown"] {
            assert_eq!(
                entry(name, "x").source_kind_hint(),
                MediaKind::Image,
                "{name} should read as an image source"
            );
        }
    }

    #[test]
    fn a_capture_entry_carries_the_source_file_name_and_size() {
        let path = std::env::temp_dir().join(format!(
            "apexshot-edits-page-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"some bytes").unwrap();

        let capture = entry(&path.to_string_lossy(), "Ignored title").as_capture();
        assert_eq!(
            capture.display_name,
            path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(capture.size_bytes, 10);
        assert!(capture.modified.is_some());
        assert_eq!(capture.kind, MediaKind::Image);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_source_still_yields_a_usable_entry() {
        let capture = entry("/nonexistent/gone.png", "Gone").as_capture();
        assert_eq!(capture.size_bytes, 0);
        assert!(capture.modified.is_none());
    }

    #[test]
    fn each_kind_reports_its_own_labels() {
        assert_ne!(EditsKind::Motion.title(), EditsKind::Video.title());
        assert_ne!(
            EditsKind::Motion.search_placeholder(),
            EditsKind::Video.search_placeholder()
        );
        assert_ne!(
            EditsKind::Motion.empty_title(),
            EditsKind::Video.empty_title()
        );
        assert_ne!(
            EditsKind::Motion.empty_detail(),
            EditsKind::Video.empty_detail()
        );
    }

    #[test]
    fn the_count_line_includes_the_number() {
        let line = EditsKind::Motion.count_line(3);
        assert!(line.contains('3'), "count should appear in {line:?}");
    }
}

//! Local capture gallery pages (Screenshots / Recordings) for the History window.
//!
//! A page scans its folder on a background thread, streams thumbnails in through
//! an `mpsc` channel drained on the GTK main loop, and lets the user search,
//! refresh, and act on individual captures. Everything reuses the existing
//! `scan`, `thumbnails`, and `actions` modules and the Settings visual
//! vocabulary (the `recent-captures-*` CSS classes).

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

/// On-card thumbnail display size — smaller than the baked 260×150 (same
/// aspect ratio) so the grid reads like a file manager's icon view. The
/// Picture scales the baked thumbnail down with no quality loss.
/// Shared with the Cloud grid so all three History pages match.
pub(super) const CARD_THUMB_WIDTH: i32 = 160;
pub(super) const CARD_THUMB_HEIGHT: i32 = 92;

/// One card widget plus the metadata needed to filter and act on it.
struct Card {
    /// Thumbnail-request id, echoed back on delivery to find this card.
    id: u64,
    root: gtk4::FlowBoxChild,
    entry: CaptureEntry,
    search_key: String,
    picture: Picture,
    placeholder: GtkBox,
}

/// Shared per-page state, kept alive by the closures wired below.
struct PageState {
    kind: MediaKind,
    grid: FlowBox,
    empty_state: GtkBox,
    scroller: ScrolledWindow,
    subtitle: Label,
    /// The window's shared header-bar search entry; re-read after every
    /// populate so freshly loaded cards honor an in-progress query.
    search: Entry,
    cards: RefCell<Vec<Rc<Card>>>,
    /// Current scan/thumbnail batch. Bumped on refresh so stale deliveries drop.
    generation: Cell<u64>,
    next_card_id: Cell<u64>,
    toast: HistoryToast,
}

/// Build a Screenshots or Recordings page as a scrolled, titled column.
///
/// The page is filtered by the window's shared header-bar search entry and
/// reports its refresh hook so the header-bar refresh button can reload it.
pub fn build_local_page(
    kind: MediaKind,
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

    // Header: gallery title + live count subtitle. Search and refresh live in
    // the window's header bar, GNOME-Settings-style, not on the page itself.
    let header = GtkBox::new(Orientation::Vertical, 0);

    let title_text = match kind {
        MediaKind::Image => t("Screenshots"),
        MediaKind::Video => t("Recordings"),
    };
    let title = Label::new(Some(&title_text));
    title.add_css_class("recent-captures-title");
    title.set_halign(Align::Start);

    let subtitle = Label::new(Some(&t("Loading…")));
    subtitle.add_css_class("history-page-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_margin_bottom(18);

    header.append(&title);
    header.append(&subtitle);

    column.append(&header);

    // The card grid.
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

    // Empty-state, hidden until a scan finishes with nothing to show.
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

    // Filter the grid on every keystroke.
    {
        let state = Rc::clone(&state);
        search.connect_changed(move |entry| {
            apply_filter(&state, &entry.text());
        });
    }

    // The header-bar refresh button cancels in-flight work and rescans.
    let refresh = {
        let state = Rc::clone(&state);
        Rc::new(move || reload(&state)) as Rc<dyn Fn()>
    };

    // First population happens once the page is actually shown, so opening the
    // window does not pay for pages the user never visits.
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
        search_placeholder: match kind {
            MediaKind::Image => t("Search screenshots"),
            MediaKind::Video => t("Search recordings"),
        },
        searchable: true,
    }
}

fn build_empty_state(kind: MediaKind) -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 0);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Start);
    empty.set_margin_top(24);

    let icon = Image::from_icon_name(match kind {
        MediaKind::Image => {
            crate::capture::editor::window::icon_names::custom::SCREENSHOOTER_SYMBOLIC
        }
        MediaKind::Video => {
            crate::capture::editor::window::icon_names::custom::RECORD_SCREEN_SYMBOLIC
        }
    });
    icon.set_pixel_size(48);
    icon.add_css_class("history-empty-icon");
    icon.set_halign(Align::Center);
    empty.append(&icon);

    let title_text = match kind {
        MediaKind::Image => t("No screenshots yet"),
        MediaKind::Video => t("No recordings yet"),
    };
    let title = Label::new(Some(&title_text));
    title.add_css_class("recent-captures-empty-title");

    let detail_text = match kind {
        MediaKind::Image => t("Captures you take will show up here."),
        MediaKind::Video => t("Screen recordings you make will show up here."),
    };
    let detail = Label::new(Some(&detail_text));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_halign(Align::Center);

    empty.append(&title);
    empty.append(&detail);
    empty
}

/// Clear the grid, bump the generation, rescan on a worker thread, and stream
/// the results (and their thumbnails) back onto the main loop.
fn reload(state: &Rc<PageState>) {
    // A new batch: any in-flight thumbnails for the old one become irrelevant.
    let previous = state.generation.get();
    if previous != 0 {
        thumbnails::cancel_generation(previous);
    }
    let generation = thumbnails::next_generation();
    state.generation.set(generation);

    // Remove existing cards.
    for card in state.cards.borrow().iter() {
        state.grid.remove(&card.root);
    }
    state.cards.borrow_mut().clear();
    state.empty_state.set_visible(false);
    state.grid.set_visible(true);
    state.subtitle.set_visible(true);
    state.subtitle.set_text(&t("Loading…"));

    let kind = state.kind;
    let (scan_tx, scan_rx) = mpsc::channel::<Vec<CaptureEntry>>();
    std::thread::spawn(move || {
        let config = crate::config::load_config();
        let folder = match kind {
            MediaKind::Image => scan::screenshot_folder(&config),
            MediaKind::Video => scan::recording_folder(&config),
        };
        let entries = scan::scan_folder(&folder, kind);
        let _ = scan_tx.send(entries);
    });

    // Channel for thumbnails; retained by the idle drain below.
    let (thumb_tx, thumb_rx) = mpsc::channel::<ThumbnailReady>();
    let thumb_rx = Rc::new(thumb_rx);

    // Wait for the scan result, then build cards and submit thumbnail jobs.
    {
        let state = Rc::clone(state);
        let thumb_tx = thumb_tx.clone();
        let thumb_rx = Rc::clone(&thumb_rx);
        glib::source::idle_add_local(move || {
            match scan_rx.try_recv() {
                Ok(entries) => {
                    // Stale result from a superseded reload.
                    if state.generation.get() != generation {
                        return glib::ControlFlow::Break;
                    }
                    populate(&state, generation, entries, &thumb_tx);
                    start_thumbnail_drain(&state, generation, Rc::clone(&thumb_rx));
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }
}

/// Build the cards for `entries` and submit a thumbnail job per card.
fn populate(
    state: &Rc<PageState>,
    generation: u64,
    entries: Vec<CaptureEntry>,
    thumb_tx: &mpsc::Sender<ThumbnailReady>,
) {
    if entries.is_empty() {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        state.subtitle.set_visible(false);
        return;
    }

    let noun = match state.kind {
        MediaKind::Image => "screenshots",
        MediaKind::Video => "recordings",
    };
    state
        .subtitle
        .set_text(&format!("{} {}", entries.len(), noun));
    state.subtitle.set_visible(true);

    let now = SystemTime::now();
    let mut cards = state.cards.borrow_mut();
    for entry in entries {
        let id = state.next_card_id.get();
        state.next_card_id.set(id + 1);

        let card = build_card(state, id, &entry, now);
        state.grid.insert(&card.root, -1);

        // Kick off the thumbnail for this card.
        thumbnails::submit(ThumbnailRequest {
            id,
            generation,
            source: ThumbnailSource::Local(entry),
            reply: thumb_tx.clone(),
        });

        cards.push(Rc::new(card));
    }
    drop(cards);

    // The shared search entry may already hold a query typed before this page
    // finished loading (or while another page was visible) — honor it now.
    apply_filter(state, &state.search.text());
}

/// Build a single card. `id` is the thumbnail-request id echoed back on
/// delivery so the finished image lands on the right card.
fn build_card(state: &Rc<PageState>, id: u64, entry: &CaptureEntry, now: SystemTime) -> Card {
    let card_box = GtkBox::new(Orientation::Vertical, 0);
    card_box.set_halign(Align::Fill);

    let clickable = Button::new();
    clickable.add_css_class("recent-captures-card");
    clickable.set_halign(Align::Fill);

    let content = GtkBox::new(Orientation::Vertical, 0);

    // Image area: a placeholder box until the thumbnail lands, then a Picture.
    let image_wrap = gtk4::Overlay::new();
    image_wrap.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
    image_wrap.set_halign(Align::Center);

    let placeholder = GtkBox::new(Orientation::Vertical, 0);
    placeholder.add_css_class("recent-captures-card-image");
    placeholder.add_css_class("recent-captures-picture-missing");
    placeholder.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);

    let placeholder_icon = Image::from_icon_name(match entry.kind {
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
    // Thumbnails are pre-baked to exactly THUMB_WIDTH×THUMB_HEIGHT, so no
    // content-fit is needed (and GTK 4.6 has none).
    picture.set_visible(false);

    image_wrap.set_child(Some(&placeholder));
    image_wrap.add_overlay(&picture);

    // A small badge for video captures, so recordings read at a glance.
    if entry.kind == MediaKind::Video {
        let badge = Image::from_icon_name(
            crate::capture::editor::window::icon_names::custom::RECORD_SCREEN_SYMBOLIC,
        );
        badge.add_css_class("history-media-badge");
        badge.set_pixel_size(12);
        badge.set_halign(Align::End);
        badge.set_valign(Align::End);
        badge.set_margin_end(8);
        badge.set_margin_bottom(8);
        image_wrap.add_overlay(&badge);
    }

    content.append(&image_wrap);

    // Filename only, centered and wrapping like a file-manager icon; the
    // size and timestamp move into the tooltip.
    let title = Label::new(Some(&entry.display_name));
    title.add_css_class("recent-captures-card-title");
    title.set_halign(Align::Fill);
    title.set_justify(gtk4::Justification::Center);
    title.set_wrap(true);
    // Cloud-style names are one unbreakable token; without char fallback Pango
    // reports the whole word as the minimum width and the homogeneous FlowBox
    // sizes every card to it, collapsing the grid to two columns.
    title.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    title.set_lines(2);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(18);
    content.append(&title);

    clickable.set_tooltip_text(Some(&format!(
        "{}\n{} · {}",
        entry.display_name,
        scan::format_relative_time(entry.modified, now),
        scan::format_size(entry.size_bytes),
    )));

    clickable.set_child(Some(&content));
    card_box.append(&clickable);

    let child = gtk4::FlowBoxChild::new();
    child.set_child(Some(&card_box));

    // Double-click opens like a file manager; right-click raises the action menu.
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(gtk4::gdk::BUTTON_PRIMARY);
        gesture.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                report(&state, super::actions::open_in_default_app(&entry));
            }
        });
        clickable.add_controller(gesture);
    }
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let anchor = clickable.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
        gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
        gesture.connect_pressed(move |_, _, x, y| {
            show_action_popover(&state, &entry, &anchor, x, y);
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
        // A newer batch means these deliveries are for cards that no longer exist.
        if state.generation.get() != generation {
            return glib::ControlFlow::Break;
        }
        loop {
            match thumb_rx.try_recv() {
                Ok(ready) => {
                    if ready.generation != generation {
                        continue;
                    }
                    apply_thumbnail(&state, ready);
                }
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
    });
}

/// Show a finished thumbnail on its card, matched by request id.
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

/// Show only cards whose filename contains `needle` (case-insensitive).
fn apply_filter(state: &Rc<PageState>, needle: &str) {
    let needle = needle.trim().to_ascii_lowercase();
    let cards = state.cards.borrow();
    let mut any_visible = false;
    for card in cards.iter() {
        let visible = needle.is_empty() || card.search_key.contains(&needle);
        card.root.set_visible(visible);
        any_visible |= visible;
    }
    // Only surface the empty state for a truly empty folder, not a no-match
    // search, so the search box does not look broken.
    if cards.is_empty() {
        state.empty_state.set_visible(true);
        state.grid.set_visible(false);
    } else {
        state.empty_state.set_visible(false);
        state.grid.set_visible(any_visible || !needle.is_empty());
    }
}

/// Present the per-item context menu at the right-click position on `anchor`.
fn show_action_popover(
    state: &Rc<PageState>,
    entry: &CaptureEntry,
    anchor: &Button,
    x: f64,
    y: f64,
) {
    let popover = Popover::new();
    popover.add_css_class("history-action-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    // File-manager geometry: below the pointer, body extending to the right.
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(anchor);

    let menu = GtkBox::new(Orientation::Vertical, 2);

    let add_action = |label_text: &str, destructive: bool| {
        let btn = Button::new();
        btn.add_css_class("history-action-btn");
        // Opening a pointer-driven popover may focus its first action. Keep
        // that mouse focus visually neutral; keyboard focus is styled with
        // `:focus-visible`.
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

    let open_btn = add_action(&t("Open"), false);
    let editor_btn = add_action(&t("Open in editor"), false);
    let copy_btn = add_action(&t("Copy"), false);
    let reveal_btn = add_action(&t("Show in files"), false);
    let upload_btn = add_action(&t("Upload to cloud"), false);

    let separator = gtk4::Separator::new(Orientation::Horizontal);
    separator.add_css_class("history-action-separator");
    menu.append(&separator);

    let delete_btn = add_action(&t("Delete"), true);

    popover.set_child(Some(&menu));

    // Keep the action menu flush with the card that opened it. This also
    // prevents the shared popover style from making small card menus wider
    // than their trigger.
    popover.set_size_request(anchor.width(), -1);

    // Simple, synchronous actions report their outcome and close the popover.
    let wire_simple =
        |btn: &Button, action: Rc<dyn Fn(&CaptureEntry) -> Result<String, String>>| {
            let state = Rc::clone(state);
            let entry = entry.clone();
            let popover = popover.clone();
            btn.connect_clicked(move |_| {
                report(&state, action(&entry));
                popover.popdown();
            });
        };

    wire_simple(&open_btn, Rc::new(super::actions::open_in_default_app));
    wire_simple(
        &editor_btn,
        Rc::new(super::actions::open_in_apexshot_editor),
    );
    wire_simple(&copy_btn, Rc::new(super::actions::copy_to_clipboard));
    wire_simple(&reveal_btn, Rc::new(super::actions::reveal_in_file_manager));

    // Upload hits the network, so it runs on a worker thread and delivers back.
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let popover = popover.clone();
        upload_btn.connect_clicked(move |_| {
            popover.popdown();
            state.toast.show(&t("Uploading…"), ToastKind::Neutral, None);
            let (tx, rx) = mpsc::channel::<Result<String, String>>();
            let entry_bg = entry.clone();
            std::thread::spawn(move || {
                let _ = tx.send(super::actions::upload_to_cloud(&entry_bg));
            });
            let state = Rc::clone(&state);
            glib::source::idle_add_local(move || match rx.try_recv() {
                Ok(result) => {
                    report(&state, result);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            });
        });
    }

    // Delete confirms first, then removes the file and its card.
    {
        let state = Rc::clone(state);
        let entry = entry.clone();
        let popover = popover.clone();
        delete_btn.connect_clicked(move |_| {
            popover.popdown();
            confirm_delete(&state, &entry);
        });
    }

    // Anchor a strip as wide as the menu, starting at the pointer: GTK centers
    // the popover on the strip, so the menu's top-left corner lands at the
    // cursor and the body extends down-right. The pointer sits on the menu's
    // corner, never over "Open" — which is what made that row look selected.
    let (_, natural_width, _, _) = popover.measure(gtk4::Orientation::Horizontal, -1);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
        x as i32,
        y as i32,
        natural_width.max(1),
        1,
    )));

    popover.popup();
}

/// Ask before deleting, then remove the capture and its card on confirmation.
fn confirm_delete(state: &Rc<PageState>, entry: &CaptureEntry) {
    let root = state.scroller.root().and_downcast::<gtk4::Window>();
    let dialog = gtk4::MessageDialog::builder()
        .modal(true)
        .message_type(gtk4::MessageType::Warning)
        .buttons(gtk4::ButtonsType::None)
        .text(tfmt("Delete {name}?", &[("name", &entry.display_name)]))
        .secondary_text(t("This permanently removes the file from disk."))
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
            let result = super::actions::delete_capture(&entry);
            if result.is_ok() {
                remove_card(&state, &entry.path);
            }
            report(&state, result);
        }
        dialog.close();
    });
    dialog.show();
}

/// Drop the card whose entry matches `path` from the grid and the card list.
fn remove_card(state: &Rc<PageState>, path: &PathBuf) {
    let mut cards = state.cards.borrow_mut();
    if let Some(pos) = cards.iter().position(|card| &card.entry.path == path) {
        let card = cards.remove(pos);
        state.grid.remove(&card.root);
    }
    // Keep the header count and empty state in sync with what the grid shows.
    let remaining = cards.len();
    drop(cards);
    if remaining == 0 {
        state.grid.set_visible(false);
        state.empty_state.set_visible(true);
        state.subtitle.set_visible(false);
    } else {
        let noun = match state.kind {
            MediaKind::Image => "screenshots",
            MediaKind::Video => "recordings",
        };
        state.subtitle.set_text(&format!("{remaining} {noun}"));
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

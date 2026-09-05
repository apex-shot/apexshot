//! Cloud page for the History window.
//!
//! This page renders the signed-in account's ApexShot Cloud uploads. It is a
//! small state machine driven entirely by config:
//!
//! | State            | What the user sees                                    |
//! |------------------|-------------------------------------------------------|
//! | Not signed in    | Explanation + a "Sign in" button, then login polling  |
//! | Signed in        | A paged grid of the account's uploads                |
//! | XBackBone chosen | A note that this page covers ApexShot Cloud only      |
//! | Error            | A readable message with a Retry button                |
//!
//! Networking (listing, thumbnails) always happens on background threads and
//! results are delivered to the main loop through `mpsc` channels drained by
//! `glib::idle_add_local`, mirroring `settings::cloud` and the local grids.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Box as GtkBox, Button, FlowBox, Image, Label, Orientation, Picture,
    ScrolledWindow, SelectionMode, Widget,
};

use crate::cloud::listing::{
    CloudReadError, CloudUpload, UploadsPage, UploadsPager, DEFAULT_PAGE_SIZE,
};
use crate::config::{is_cloud_logged_in, load_config, AppConfig};
use crate::history::thumbnails::{self, ThumbnailReady, ThumbnailRequest, ThumbnailSource};
use crate::i18n::t;

use super::actions;
use super::local_page::{CARD_THUMB_HEIGHT, CARD_THUMB_WIDTH};
use super::window::{HistoryToast, ToastKind};

/// How long an action-outcome toast stays up before fading.
const TOAST_SUCCESS: Duration = Duration::from_secs(2);
const TOAST_ERROR: Duration = Duration::from_secs(4);

/// How often to re-check config for a login that landed in a terminal.
const LOGIN_POLL_SECONDS: u32 = 2;

/// Build the History window's Cloud page.
///
/// Returns the page widget (a self-contained scroller with the same title
/// header and margins the local pages use) plus a refresh hook that re-renders
/// from current config. The page evaluates its state immediately and rebuilds
/// itself in place as the session or entitlement changes, so the caller never
/// has to rebuild it. Cloud uploads page in from the server, so the shared
/// header-bar search does not filter this page.
///
/// `toast` is the shared window toast (the same `HistoryToast` handed to
/// `build_local_page`), used to report per-card action outcomes.
pub fn build_cloud_page(toast: HistoryToast) -> super::HistoryPage {
    // Same page chrome the local pages build: an outer vertical scroller and a
    // margined column with a settings-style title header, so the three stack
    // pages line up pixel-for-pixel.
    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let column = GtkBox::new(Orientation::Vertical, 0);
    column.add_css_class("recent-captures-root");
    column.set_margin_top(20);
    column.set_margin_bottom(32);
    column.set_margin_start(28);
    column.set_margin_end(28);

    let title = Label::new(Some(&t("Cloud")));
    title.add_css_class("recent-captures-title");
    title.set_halign(Align::Start);

    let subtitle = Label::new(Some(&t("Everything you have uploaded to ApexShot Cloud")));
    subtitle.add_css_class("history-page-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_margin_bottom(18);

    column.append(&title);
    column.append(&subtitle);

    // Body holds whichever state is currently rendered; a rebuild only ever
    // clears and refills this box, leaving the title header untouched.
    let body = GtkBox::new(Orientation::Vertical, 0);
    body.set_hexpand(true);
    body.set_vexpand(true);
    column.append(&body);

    scroller.set_child(Some(&column));

    let page = Rc::new(CloudPage {
        body,
        scroller: scroller.clone(),
        toast,
        polling: Cell::new(false),
    });

    page.render_current_state();

    // The header-bar refresh button re-renders from the current config.
    let refresh = {
        let page = Rc::clone(&page);
        Rc::new(move || page.render_current_state()) as Rc<dyn Fn()>
    };

    super::HistoryPage {
        widget: scroller.upcast(),
        refresh,
        search_placeholder: t("Search isn't available for Cloud"),
        searchable: false,
    }
}

struct CloudPage {
    /// The refillable body of the page (everything below the title header).
    body: GtkBox,
    /// The page's outer scroller, whose vadjustment drives grid paging.
    scroller: ScrolledWindow,
    toast: HistoryToast,
    /// True while a login poll timer is running, so we never start a second.
    polling: Cell<bool>,
}

impl CloudPage {
    /// Clear the page and lay out whichever state config currently describes.
    fn render_current_state(self: &Rc<Self>) {
        clear_box(&self.body);
        let config = load_config();

        // XBackBone users have no ApexShot Cloud listing to show, whatever their
        // session state — check the destination before anything else.
        if config.cloud_destination == "xbackbone" {
            self.show_xbackbone_notice();
            return;
        }

        if !is_cloud_logged_in(&config) {
            self.show_signed_out();
            return;
        }

        self.show_uploads_grid(config);
    }

    // --- signed-out state ---

    fn show_signed_out(self: &Rc<Self>) {
        let state = empty_state(
            &t("Sign in to ApexShot Cloud"),
            &t("Connect your ApexShot Cloud account to browse everything you have uploaded, right here in History."),
        );

        let sign_in = Button::with_label(&t("Sign in"));
        sign_in.add_css_class("recent-captures-primary-button");
        sign_in.set_halign(Align::Center);
        sign_in.set_margin_top(20);

        {
            let page = Rc::clone(self);
            sign_in.connect_clicked(move |btn| {
                crate::settings::cloud::spawn_apexshot_login();
                btn.set_label(&t("Waiting for sign-in…"));
                btn.set_sensitive(false);
                page.start_login_polling();
            });
        }

        state.append(&sign_in);
        self.body.append(&state);

        // A terminal login may already be in flight from a previous click; keep
        // watching so the page flips to the grid the moment it lands.
        self.start_login_polling();
    }

    /// Watch config for a session to appear after `spawn_apexshot_login`, then
    /// re-render. Runs at most one timer at a time.
    fn start_login_polling(self: &Rc<Self>) {
        if self.polling.get() {
            return;
        }
        self.polling.set(true);

        let page = Rc::clone(self);
        glib::timeout_add_seconds_local(LOGIN_POLL_SECONDS, move || {
            // Stop if the page state moved on (e.g. the user is no longer on the
            // signed-out screen because a rebuild already happened).
            if is_cloud_logged_in(&load_config()) {
                page.polling.set(false);
                page.render_current_state();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    // --- XBackBone state ---

    fn show_xbackbone_notice(self: &Rc<Self>) {
        let state = empty_state(
            &t("This page covers ApexShot Cloud"),
            &t("Your uploads currently go to a self-hosted XBackBone instance. Switch your upload destination to ApexShot Cloud in Settings to browse those uploads here."),
        );
        self.body.append(&state);
    }

    // --- error state ---

    fn show_error(self: &Rc<Self>, message: &str) {
        // Replace whatever partial grid chrome was already in the body (the
        // "Loading…" status line, an empty grid) with a clean error state.
        clear_box(&self.body);
        let state = empty_state(&t("Could not load your cloud uploads"), message);

        let retry = Button::with_label(&t("Retry"));
        retry.add_css_class("recent-captures-secondary-button");
        retry.set_halign(Align::Center);
        retry.set_margin_top(20);
        {
            let page = Rc::clone(self);
            retry.connect_clicked(move |_| {
                page.render_current_state();
            });
        }
        state.append(&retry);

        self.body.append(&state);
    }

    // --- upload grid ---

    fn show_uploads_grid(self: &Rc<Self>, config: AppConfig) {
        let grid = FlowBox::new();
        grid.add_css_class("recent-captures-grid");
        grid.set_selection_mode(SelectionMode::None);
        grid.set_homogeneous(true);
        // Same grid geometry as the local pages so all three read alike.
        grid.set_max_children_per_line(8);
        grid.set_min_children_per_line(1);
        grid.set_row_spacing(14);
        grid.set_column_spacing(14);
        grid.set_halign(Align::Fill);
        grid.set_valign(Align::Start);
        grid.set_hexpand(true);

        // A status line doubles as the empty-state message once the first page
        // has come back with nothing.
        let status = Label::new(Some(&t("Loading your cloud uploads…")));
        status.add_css_class("recent-captures-empty-detail");
        status.set_halign(Align::Center);
        status.set_margin_top(16);
        status.set_margin_bottom(8);

        let load_more = Button::with_label(&t("Load more"));
        load_more.add_css_class("recent-captures-secondary-button");
        load_more.add_css_class("history-load-more");
        load_more.set_halign(Align::Center);
        load_more.set_margin_top(16);
        load_more.set_visible(false);

        // The grid, status line, and load-more button sit directly in the page
        // body — the page already lives inside the window's outer scroller, so
        // nesting a second scroller here would trap the wheel and split paging.
        self.body.append(&grid);
        self.body.append(&status);
        self.body.append(&load_more);

        // A fresh thumbnail batch id, so a page rebuild ignores late results
        // from a previous session's in-flight decodes.
        let generation = thumbnails::next_generation();

        // Own the pager behind an Rc<RefCell> so both the button and the scroll
        // handler drive the same cursor, and a `Cell<bool>` gate so overlapping
        // page requests can never be issued.
        let ctx = Rc::new(GridContext {
            page: Rc::clone(self),
            config,
            pager: RefCell::new(UploadsPager::new(DEFAULT_PAGE_SIZE)),
            loading_in_flight: Cell::new(false),
            first_page_loaded: Cell::new(false),
            next_card_id: Cell::new(0),
            generation,
            grid,
            status,
            load_more: load_more.clone(),
        });

        {
            let ctx = Rc::clone(&ctx);
            load_more.connect_clicked(move |_| {
                ctx.request_next_page();
            });
        }

        // Trigger the next page as the user nears the bottom of the page's own
        // outer scroller. The non-overlap gate in `request_next_page` keeps a
        // burst of scroll events from firing more than one fetch.
        {
            let ctx = Rc::clone(&ctx);
            let adjustment = self.scroller.vadjustment();
            adjustment.connect_value_changed(move |adj| {
                let remaining = adj.upper() - (adj.value() + adj.page_size());
                if remaining < adj.page_size() {
                    ctx.request_next_page();
                }
            });
        }

        ctx.request_next_page();
    }
}

/// Everything a live cloud grid needs to page and render.
struct GridContext {
    page: Rc<CloudPage>,
    config: AppConfig,
    pager: RefCell<UploadsPager>,
    /// Non-overlap gate: at most one page request may be outstanding.
    loading_in_flight: Cell<bool>,
    first_page_loaded: Cell<bool>,
    next_card_id: Cell<u64>,
    generation: u64,
    grid: FlowBox,
    status: Label,
    load_more: Button,
}

impl GridContext {
    /// Fetch the next page on a worker thread, unless the pager is exhausted or
    /// a request is already in flight.
    fn request_next_page(self: &Rc<Self>) {
        if self.loading_in_flight.get() {
            return;
        }
        if self.pager.borrow().is_exhausted() {
            self.load_more.set_visible(false);
            return;
        }
        self.loading_in_flight.set(true);
        self.load_more.set_sensitive(false);

        // The pager is stateful and !Send-friendly to keep on the UI thread, so
        // clone it into the worker and copy back the advanced cursor on return.
        let mut pager = self.pager.borrow().clone();
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel::<PageOutcome>();
        std::thread::spawn(move || {
            let result = pager.next_page(&config);
            let _ = tx.send(PageOutcome { pager, result });
        });

        let ctx = Rc::clone(self);
        glib::source::idle_add_local(move || match rx.try_recv() {
            Ok(outcome) => {
                ctx.on_page_result(outcome);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            // The worker vanished without sending: release the gate so a later
            // scroll or click can retry rather than wedging forever.
            Err(mpsc::TryRecvError::Disconnected) => {
                ctx.loading_in_flight.set(false);
                ctx.load_more.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });
    }

    fn on_page_result(self: &Rc<Self>, outcome: PageOutcome) {
        self.loading_in_flight.set(false);
        self.load_more.set_sensitive(true);
        // Adopt the worker's advanced cursor so the next request continues.
        *self.pager.borrow_mut() = outcome.pager;

        match outcome.result {
            Ok(Some(page)) => self.append_page(page),
            // The pager reports completion as Ok(None).
            Ok(None) => {
                self.load_more.set_visible(false);
                self.finish_if_empty();
            }
            Err(error) => self.on_page_error(error),
        }
    }

    fn append_page(self: &Rc<Self>, page: UploadsPage) {
        let is_first = !self.first_page_loaded.get();
        self.first_page_loaded.set(true);

        for upload in &page.items {
            let card = self.build_card(upload);
            self.grid.insert(&card, -1);
        }

        let exhausted = self.pager.borrow().is_exhausted() || !page.has_more;
        self.load_more.set_visible(!exhausted);

        if is_first {
            self.finish_if_empty();
        }
    }

    /// Update or hide the status line once we know whether anything loaded.
    fn finish_if_empty(&self) {
        if self.grid.first_child().is_none() {
            self.status.set_visible(true);
            self.status
                .set_text(&t("You have not uploaded anything to ApexShot Cloud yet."));
        } else {
            self.status.set_visible(false);
        }
    }

    fn on_page_error(self: &Rc<Self>, error: CloudReadError) {
        // If the very first page failed there is nothing to show, so replace the
        // whole page with the error state (which offers a clean retry). A later
        // page failing keeps the cards already on screen and reports via toast.
        if !self.first_page_loaded.get() {
            self.page.show_error(&error.to_string());
        } else {
            self.load_more.set_visible(true);
            self.page
                .toast
                .show(&error.to_string(), ToastKind::Error, Some(TOAST_ERROR));
        }
    }

    fn build_card(self: &Rc<Self>, upload: &CloudUpload) -> Widget {
        // Same geometry the local pages use: a fixed-size centred thumbnail with
        // a centred, wrapping filename underneath, wrapped in a clickable card.
        let card_box = GtkBox::new(Orientation::Vertical, 0);
        card_box.set_halign(Align::Fill);

        let clickable = Button::new();
        clickable.add_css_class("recent-captures-card");
        clickable.set_halign(Align::Fill);

        let content = GtkBox::new(Orientation::Vertical, 0);

        // Image area, built exactly like the local pages': a fixed-size overlay
        // whose measured child is the placeholder, with the Picture layered on
        // top. A Picture with a filename reports the image's intrinsic width as
        // its natural width, and an unmeasured overlay is what stops that from
        // inflating every card in the homogeneous grid.
        let image_wrap = gtk4::Overlay::new();
        image_wrap.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
        image_wrap.set_halign(Align::Center);
        image_wrap.set_overflow(gtk4::Overflow::Hidden);

        let placeholder = GtkBox::new(Orientation::Vertical, 0);
        placeholder.add_css_class("recent-captures-card-image");
        placeholder.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);

        let picture = Picture::new();
        picture.add_css_class("recent-captures-card-image");
        picture.set_size_request(CARD_THUMB_WIDTH, CARD_THUMB_HEIGHT);
        picture.set_can_shrink(true);
        picture.set_visible(false);

        image_wrap.set_child(Some(&placeholder));
        image_wrap.add_overlay(&picture);
        content.append(&image_wrap);

        if let Some(url) = upload
            .thumbnail_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            self.load_thumbnail(url, &picture, &placeholder);
        } else {
            add_missing_badge(&placeholder);
        }

        // Filename only, centred and wrapping; size and time go in the tooltip,
        // exactly as the local grids do.
        let title = Label::new(Some(&upload.display_name()));
        title.add_css_class("recent-captures-card-title");
        title.set_halign(Align::Fill);
        title.set_justify(gtk4::Justification::Center);
        title.set_wrap(true);
        // Upload names are usually one unbreakable token, so allow a mid-word
        // break: otherwise Pango's minimum width is the whole word and the
        // homogeneous FlowBox stretches every card to fit it.
        title.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        title.set_lines(2);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title.set_max_width_chars(18);
        content.append(&title);

        clickable.set_tooltip_text(Some(&card_tooltip(upload)));
        clickable.set_child(Some(&content));
        card_box.append(&clickable);

        // Left-click opens the share link; the actions also live in the
        // right-click popover, matching the local pages.
        let share_url = share_url_of(upload);
        {
            let toast = self.page.toast.clone();
            let url = share_url.clone();
            clickable.connect_clicked(move |_| match url.as_deref() {
                Some(url) => report(&toast, actions::open_in_browser(url)),
                None => toast.show(
                    &t("This upload has no share link"),
                    ToastKind::Error,
                    Some(TOAST_ERROR),
                ),
            });
        }

        {
            let page = Rc::clone(&self.page);
            let anchor = clickable.clone();
            let url = share_url.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
            gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
            gesture.connect_pressed(move |_, _, x, y| {
                show_cloud_popover(&page, url.as_deref(), &anchor, x, y);
            });
            clickable.add_controller(gesture);
        }

        card_box.upcast()
    }

    /// Decode a remote thumbnail on the shared pool and drop it into `picture`
    /// when it arrives, discarding results from a superseded page generation.
    /// Fetch and decode a remote thumbnail, then reveal it over `placeholder`.
    fn load_thumbnail(self: &Rc<Self>, url: &str, picture: &Picture, placeholder: &GtkBox) {
        let id = self.next_card_id.get();
        self.next_card_id.set(id + 1);

        let (tx, rx) = mpsc::channel::<ThumbnailReady>();
        thumbnails::submit(ThumbnailRequest {
            id,
            generation: self.generation,
            source: ThumbnailSource::Remote(url.to_string()),
            reply: tx,
        });

        let generation = self.generation;
        let picture = picture.clone();
        let placeholder = placeholder.clone();
        glib::source::idle_add_local(move || match rx.try_recv() {
            Ok(ready) => {
                if ready.generation == generation {
                    match ready.result {
                        Ok(path) => {
                            picture.set_filename(Some(&path));
                            picture.set_visible(true);
                        }
                        Err(_) => add_missing_badge(&placeholder),
                    }
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }
}

/// Result of a background page fetch, carrying the advanced pager back so the
/// UI thread can continue from where the worker left off.
struct PageOutcome {
    pager: UploadsPager,
    result: Result<Option<UploadsPage>, CloudReadError>,
}

// --- shared helpers ---

/// A centred block of plain title + detail text. Callers append their own
/// action button.
fn empty_state(title: &str, detail: &str) -> GtkBox {
    let state = GtkBox::new(Orientation::Vertical, 0);
    // Plain centred text: no card padding, background or border.
    state.set_halign(Align::Center);
    state.set_valign(Align::Center);
    state.set_hexpand(true);
    state.set_vexpand(true);
    // The page header (title + subtitle) sits above `body`, so centring inside
    // body alone lands low on the page. This bottom margin lifts the block back
    // to the optical centre of the page.
    // ponytail: fixed offset matched to the header height, recompute if the
    // header gains rows.
    state.set_margin_bottom(76);

    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("recent-captures-empty-title");
    title_lbl.set_halign(Align::Center);
    title_lbl.set_justify(gtk4::Justification::Center);
    state.append(&title_lbl);

    let detail_lbl = Label::new(Some(detail));
    detail_lbl.add_css_class("recent-captures-empty-detail");
    detail_lbl.set_halign(Align::Center);
    detail_lbl.set_justify(gtk4::Justification::Center);
    detail_lbl.set_wrap(true);
    detail_lbl.set_max_width_chars(52);
    state.append(&detail_lbl);

    state
}

/// Drop a "no preview" badge into a thumbnail placeholder that has no image.
///
/// Idempotent: adding the marker class more than once is harmless, and a frame
/// that already shows a badge simply gets a second identical one only if called
/// twice, which never happens (build-time xor thumbnail-failure, not both).
fn add_missing_badge(placeholder: &GtkBox) {
    placeholder.add_css_class("recent-captures-picture-missing");

    let badge = Image::from_icon_name("image-missing-symbolic");
    badge.add_css_class("history-media-badge");
    badge.set_pixel_size(16);
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    badge.set_hexpand(true);
    badge.set_vexpand(true);
    placeholder.append(&badge);
}

/// Human-readable timestamp for an upload, empty when the server sent nothing
/// usable. Uses the parsed UTC time formatted in the local zone.
fn format_upload_time(upload: &CloudUpload) -> String {
    match upload.created_at_utc() {
        Some(utc) => utc
            .with_timezone(&chrono::Local)
            .format("%b %-d, %Y")
            .to_string(),
        None => String::new(),
    }
}

/// The share link of an upload, or `None` when the server sent none.
fn share_url_of(upload: &CloudUpload) -> Option<String> {
    upload
        .share_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

/// Filename, date and size, matching the local pages' card tooltip.
fn card_tooltip(upload: &CloudUpload) -> String {
    let mut lines = upload.display_name();
    let when = format_upload_time(upload);
    let size = upload
        .size_bytes
        .filter(|bytes| *bytes > 0)
        .map(|bytes| super::scan::format_size(bytes as u64));

    let meta = match (when.is_empty(), size) {
        (false, Some(size)) => format!("{when} \u{b7} {size}"),
        (false, None) => when,
        (true, Some(size)) => size,
        (true, None) => String::new(),
    };
    if !meta.is_empty() {
        lines.push('\n');
        lines.push_str(&meta);
    }
    lines
}

/// Right-click menu for a cloud card, styled like the local pages' popover.
fn show_cloud_popover(
    page: &Rc<CloudPage>,
    share_url: Option<&str>,
    anchor: &Button,
    x: f64,
    y: f64,
) {
    let popover = gtk4::Popover::new();
    popover.add_css_class("history-action-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(anchor);

    let menu = GtkBox::new(Orientation::Vertical, 2);

    let add_action = |label_text: &str| {
        let btn = Button::new();
        btn.add_css_class("history-action-btn");
        btn.set_focus_on_click(false);
        let label = Label::new(Some(label_text));
        label.set_halign(Align::Start);
        label.set_xalign(0.0);
        btn.set_child(Some(&label));
        menu.append(&btn);
        btn
    };

    let open_btn = add_action(&t("Open in browser"));
    let copy_btn = add_action(&t("Copy link"));

    popover.set_child(Some(&menu));
    // Match the context-menu width to the card that opened it.
    popover.set_size_request(anchor.width(), -1);

    match share_url {
        Some(url) => {
            {
                let toast = page.toast.clone();
                let url = url.to_string();
                let popover = popover.clone();
                open_btn.connect_clicked(move |_| {
                    report(&toast, actions::open_in_browser(&url));
                    popover.popdown();
                });
            }
            {
                let toast = page.toast.clone();
                let url = url.to_string();
                let popover = popover.clone();
                copy_btn.connect_clicked(move |_| {
                    report(&toast, actions::copy_link_to_clipboard(&url));
                    popover.popdown();
                });
            }
        }
        None => {
            // Keep the menu shape stable; nothing to act on without a link.
            open_btn.set_sensitive(false);
            copy_btn.set_sensitive(false);
        }
    }

    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.popup();
}

/// Send an action outcome to the window's toast: the `Ok` message as success,
/// the `Err` message as an error.
fn report<T: AsRef<str>>(toast: &HistoryToast, outcome: Result<T, String>) {
    match outcome {
        Ok(message) => toast.show(message.as_ref(), ToastKind::Success, Some(TOAST_SUCCESS)),
        Err(message) => toast.show(&message, ToastKind::Error, Some(TOAST_ERROR)),
    }
}

/// Remove every child of a box, so a state transition starts from a clean slate.
fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(share: Option<&str>, size: Option<i64>) -> CloudUpload {
        CloudUpload {
            id: "up_1".to_string(),
            filename: "shot.png".to_string(),
            share_url: share.map(str::to_string),
            thumbnail_url: None,
            size_bytes: size,
            content_type: None,
            created_at: None,
        }
    }

    #[test]
    fn share_url_treats_blank_as_absent() {
        assert_eq!(
            share_url_of(&upload(Some(" https://a/b "), None)).as_deref(),
            Some("https://a/b")
        );
        assert!(share_url_of(&upload(Some("   "), None)).is_none());
        assert!(share_url_of(&upload(None, None)).is_none());
    }

    #[test]
    fn tooltip_keeps_the_name_and_omits_missing_metadata() {
        // No date and no usable size: the name stands alone with no stray separator.
        let bare = card_tooltip(&upload(None, Some(0)));
        assert_eq!(bare, "shot.png");

        // A size but still no date: one metadata line, no leading separator.
        let sized = card_tooltip(&upload(None, Some(2048)));
        assert!(sized.starts_with("shot.png\n"), "got {sized:?}");
        assert!(!sized.contains('\u{b7}'), "got {sized:?}");
    }
}

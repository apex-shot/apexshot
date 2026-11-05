//! Cloud usage in Settings. All values come from the authenticated account API.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation, ProgressBar, Separator,
    Spinner, Stack,
};

use crate::cloud::usage::{self, UsageError, UsageSnapshot};
use crate::config::{is_cloud_logged_in, load_config};
use crate::i18n::{t, tfmt};

const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";

/// Gap between the dashboard rows, matching the page's card rhythm.
const ROW_GAP: i32 = 12;

/// One dashboard reading: the hero gauge, the two secondary cards and the
/// facts strip all render from the same snapshot.
struct Readings {
    banner: Banner,
    storage: String,
    storage_limit: String,
    storage_floor: String,
    storage_percent: String,
    available: String,
    uploads: String,
    uploads_caption: String,
    uploads_percent: String,
    retention: String,
    checked: String,
}

/// What the status banner above the cards reports about the account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Banner {
    Healthy { remaining: i64 },
    NearFull { remaining: i64, limit: i64 },
    Full,
    UploadsCapped,
}

impl Banner {
    fn of(snapshot: &UsageSnapshot) -> Self {
        let used = snapshot.usage.storage_bytes;
        let limit = snapshot.limits.max_storage_bytes;
        let max_uploads = snapshot.limits.max_uploads;
        let remaining = (limit - used).max(0);
        if used >= limit {
            Banner::Full
        } else if max_uploads >= 0 && snapshot.usage.uploads >= max_uploads {
            Banner::UploadsCapped
        } else if used as f64 / limit as f64 >= 0.85 {
            Banner::NearFull { remaining, limit }
        } else {
            Banner::Healthy { remaining }
        }
    }

    fn class(self) -> &'static str {
        match self {
            Banner::Healthy { .. } => "usage-banner-ok",
            Banner::NearFull { .. } | Banner::UploadsCapped => "usage-banner-warn",
            Banner::Full => "usage-banner-full",
        }
    }

    fn title(self) -> String {
        match self {
            Banner::Healthy { .. } => t("Storage is within your allowance"),
            Banner::NearFull { .. } => t("Storage is nearly full"),
            Banner::Full => t("Storage is full"),
            Banner::UploadsCapped => t("This month's uploads are used up"),
        }
    }

    fn detail(self) -> String {
        match self {
            Banner::Healthy { remaining } => tfmt(
                "{remaining} remains before your allowance is reached.",
                &[("remaining", &format_bytes(remaining))],
            ),
            Banner::NearFull { remaining, limit } => tfmt(
                "{remaining} remains of your {limit} allowance.",
                &[
                    ("remaining", &format_bytes(remaining)),
                    ("limit", &format_bytes(limit)),
                ],
            ),
            Banner::Full => t("Free up space or raise your allowance to keep uploading."),
            Banner::UploadsCapped => t("They reset monthly in UTC."),
        }
    }
}

impl Readings {
    fn of(snapshot: &UsageSnapshot) -> Self {
        let used = snapshot.usage.storage_bytes;
        let limit = snapshot.limits.max_storage_bytes;
        let max_uploads = snapshot.limits.max_uploads;
        let uploads = snapshot.usage.uploads;
        let unlimited_uploads = max_uploads < 0;
        Self {
            banner: Banner::of(snapshot),
            storage: format_bytes(used),
            storage_limit: tfmt("of {limit}", &[("limit", &format_bytes(limit))]),
            storage_floor: format_bytes(0),
            storage_percent: percent_used(used, limit),
            available: format_bytes((limit - used).max(0)),
            uploads: uploads.to_string(),
            uploads_caption: if unlimited_uploads {
                t("No monthly limit")
            } else {
                tfmt(
                    "of {limit} this month",
                    &[("limit", &max_uploads.to_string())],
                )
            },
            uploads_percent: if unlimited_uploads {
                String::new()
            } else {
                percent_used(uploads, max_uploads)
            },
            retention: match snapshot.limits.retention_days {
                days if days > 0 => tfmt("{days} days", &[("days", &days.to_string())]),
                _ => t("Not set"),
            },
            checked: chrono::Local::now()
                .format("%b %-d, %Y · %-I:%M %p")
                .to_string(),
        }
    }
}

#[derive(Clone)]
struct View {
    root: glib::WeakRef<GtkBox>,
    states: Stack,
    banner: GtkBox,
    banner_title: Label,
    banner_detail: Label,
    account: Label,
    plan: Label,
    storage: Label,
    storage_caption: Label,
    storage_percent: Label,
    storage_bar: ProgressBar,
    storage_floor: Label,
    storage_ceiling: Label,
    available: Label,
    available_caption: Label,
    uploads: Label,
    uploads_caption: Label,
    uploads_percent: Label,
    uploads_bar: ProgressBar,
    retention: Label,
    checked: Label,
    loading_status: Label,
    spinner: Spinner,
    error: Label,
    generation: Rc<Cell<u64>>,
    session: Rc<RefCell<String>>,
    has_snapshot: Rc<Cell<bool>>,
}

pub fn build(go_cloud: impl Fn() + Clone + 'static) -> GtkBox {
    build_view(go_cloud).0
}

fn build_view(go_cloud: impl Fn() + Clone + 'static) -> (GtkBox, Rc<View>) {
    install_css();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_margin_top(12);
    root.set_margin_bottom(24);
    let states = Stack::new();
    states.set_transition_type(gtk4::StackTransitionType::Crossfade);
    states.set_vhomogeneous(false);
    states.set_vexpand(true);
    root.append(&states);

    let signed_out = GtkBox::new(Orientation::Vertical, 12);
    signed_out.add_css_class("usage-empty");
    signed_out.set_halign(Align::Center);
    signed_out.set_valign(Align::Center);
    let heading = label("Connect to see your usage", "usage-empty-title");
    heading.set_xalign(0.5);
    heading.set_wrap(true);
    heading.set_justify(gtk4::Justification::Center);
    let hint = label(
        "Sign in to view your uploads, storage and plan.",
        "usage-muted",
    );
    hint.set_xalign(0.5);
    hint.set_wrap(true);
    hint.set_max_width_chars(42);
    hint.set_justify(gtk4::Justification::Center);
    let connect = Button::with_label(&t("Go to Cloud settings"));
    connect.add_css_class("settings-primary-btn");
    connect.set_halign(Align::Center);
    connect.set_margin_top(8);
    connect.connect_clicked({
        let go_cloud = go_cloud.clone();
        move |_| go_cloud()
    });
    signed_out.append(&heading);
    signed_out.append(&hint);
    signed_out.append(&connect);
    states.add_named(&signed_out, Some("signed-out"));

    let failed = GtkBox::new(Orientation::Vertical, 12);
    failed.add_css_class("usage-empty");
    failed.append(&label("Usage is unavailable", "usage-empty-title"));
    let error = label("", "usage-muted");
    error.set_wrap(true);
    failed.append(&error);
    let retry = Button::with_label(&t("Try again"));
    retry.add_css_class("usage-secondary");
    retry.set_halign(Align::Start);
    failed.append(&retry);
    let sign_in = Button::with_label(&t("Go to Cloud settings"));
    sign_in.add_css_class("usage-link");
    sign_in.set_halign(Align::Start);
    sign_in.connect_clicked(move |_| go_cloud());
    failed.append(&sign_in);
    states.add_named(&failed, Some("error"));

    let dashboard = GtkBox::new(Orientation::Vertical, ROW_GAP);
    dashboard.set_hexpand(true);
    let overview = GtkBox::new(Orientation::Vertical, 8);
    overview.add_css_class("usage-overview");
    let top = GtkBox::new(Orientation::Horizontal, 8);
    let eyebrow = label("APEXSHOT CLOUD", "usage-eyebrow");
    eyebrow.set_hexpand(true);
    top.append(&eyebrow);
    let spinner = Spinner::new();
    spinner.set_visible(false);
    top.append(&spinner);
    let loading_status = label("", "usage-status");
    loading_status.set_visible(false);
    top.append(&loading_status);
    overview.append(&top);
    let account_row = GtkBox::new(Orientation::Horizontal, 12);
    let account = label("", "usage-account");
    account.set_hexpand(true);
    account.set_halign(Align::Start);
    account.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    account_row.append(&account);
    let plan = label("", "usage-plan");
    plan.set_valign(Align::Center);
    account_row.append(&plan);
    overview.append(&account_row);
    dashboard.append(&overview);

    let (banner, banner_title, banner_detail) = status_banner();
    dashboard.append(&banner);

    let metrics = GtkBox::new(Orientation::Horizontal, ROW_GAP);
    metrics.set_homogeneous(true);
    let (
        hero,
        storage,
        storage_caption,
        storage_bar,
        storage_floor,
        storage_ceiling,
        storage_percent,
    ) = hero_card();
    metrics.append(&hero);
    let column = GtkBox::new(Orientation::Vertical, ROW_GAP);
    let (available_card, available, available_caption) = stat_card("Available");
    available_caption.set_margin_top(3);
    column.append(&available_card);
    let (uploads_card, uploads, uploads_caption) = stat_card("Uploads");
    uploads_caption.set_margin_top(3);
    let uploads_bar = ProgressBar::new();
    uploads_bar.add_css_class("usage-progress");
    uploads_bar.set_margin_top(14);
    uploads_bar.set_hexpand(true);
    uploads_card.append(&uploads_bar);
    let uploads_percent = label("", "usage-note");
    uploads_percent.set_margin_top(7);
    uploads_card.append(&uploads_percent);
    column.append(&uploads_card);
    metrics.append(&column);
    dashboard.append(&metrics);

    let facts = GtkBox::new(Orientation::Horizontal, 0);
    facts.add_css_class("usage-facts");
    let (plan_cell, retention) = fact("Retention");
    let (resets_cell, resets) = fact("Resets");
    resets.set_text(&t("Monthly, UTC"));
    let (checked_cell, checked) = fact("Last checked");
    facts.append(&plan_cell);
    facts.append(&fact_divider());
    facts.append(&resets_cell);
    facts.append(&fact_divider());
    facts.append(&checked_cell);
    dashboard.append(&facts);

    states.add_named(&dashboard, Some("dashboard"));

    let view = Rc::new(View {
        root: root.downgrade(),
        states,
        banner,
        banner_title,
        banner_detail,
        account,
        plan,
        storage,
        storage_caption,
        storage_percent,
        storage_bar,
        storage_floor,
        storage_ceiling,
        available,
        available_caption,
        uploads,
        uploads_caption,
        uploads_percent,
        uploads_bar,
        retention,
        checked,
        loading_status,
        spinner,
        error,
        generation: Rc::new(Cell::new(0)),
        session: Rc::new(RefCell::new(String::new())),
        has_snapshot: Rc::new(Cell::new(false)),
    });
    retry.connect_clicked({
        let view = Rc::downgrade(&view);
        move |_| {
            if let Some(view) = view.upgrade() {
                view.refresh();
            }
        }
    });
    root.connect_map({
        let view = Rc::clone(&view);
        move |_| view.refresh()
    });
    root.connect_unmap({
        let generation = Rc::clone(&view.generation);
        move |_| generation.set(generation.get().wrapping_add(1))
    });
    let weak = Rc::downgrade(&view);
    glib::timeout_add_seconds_local(2, move || {
        let Some(view) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mapped = view.root.upgrade().is_some_and(|root| root.is_mapped());
        let changed = mapped && *view.session.borrow() != load_config().cloud_api_token;
        if changed {
            view.refresh();
        }
        glib::ControlFlow::Continue
    });
    (root, view)
}

fn label(text: &str, class: &str) -> Label {
    let label = Label::new(Some(&t(text)));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label
}

/// The uppercase micro-label that names a card or a fact.
fn micro(text: &str) -> Label {
    label(text, "usage-label")
}

fn card() -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("usage-card");
    card.set_hexpand(true);
    card
}

/// The allowance banner: a status dot and the sentence it stands for.
fn status_banner() -> (GtkBox, Label, Label) {
    let banner = GtkBox::new(Orientation::Horizontal, 10);
    banner.add_css_class("usage-banner");
    banner.add_css_class("usage-banner-pending");
    let dot = GtkBox::new(Orientation::Vertical, 0);
    dot.add_css_class("usage-banner-dot");
    dot.set_valign(Align::Start);
    banner.append(&dot);
    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = label("", "usage-banner-title");
    let detail = label("", "usage-banner-detail");
    detail.set_wrap(true);
    text.append(&title);
    text.append(&detail);
    banner.append(&text);
    (banner, title, detail)
}

/// The hero gauge: what is used, against what, with the scale beneath the bar.
fn hero_card() -> (GtkBox, Label, Label, ProgressBar, Label, Label, Label) {
    let card = card();
    card.add_css_class("usage-card-hero");
    card.append(&micro("Storage used"));
    let value_row = GtkBox::new(Orientation::Horizontal, 8);
    value_row.set_margin_top(10);
    let value = label("", "usage-value-hero");
    value_row.append(&value);
    let caption = label("", "usage-caption");
    caption.set_valign(Align::End);
    caption.set_margin_bottom(5);
    value_row.append(&caption);
    card.append(&value_row);
    let bar = ProgressBar::new();
    bar.add_css_class("usage-progress");
    bar.set_margin_top(16);
    bar.set_hexpand(true);
    card.append(&bar);
    let scale_row = GtkBox::new(Orientation::Horizontal, 8);
    scale_row.set_margin_top(8);
    let floor = label("", "usage-scale");
    floor.set_hexpand(true);
    scale_row.append(&floor);
    let ceiling = label("", "usage-scale");
    scale_row.append(&ceiling);
    card.append(&scale_row);
    let percent = label("", "usage-note");
    percent.set_margin_top(8);
    card.append(&percent);
    let billing = Button::with_label(&t("Manage billing"));
    billing.add_css_class("settings-primary-btn");
    billing.set_halign(Align::Start);
    billing.set_margin_top(28);
    billing.connect_clicked(|_| {
        std::thread::spawn(|| {
            if let Err(error) = crate::utils::open::open_url(BILLING_URL) {
                eprintln!("[usage] Could not open billing: {error}");
            }
        });
    });
    card.append(&billing);
    (card, value, caption, bar, floor, ceiling, percent)
}

fn stat_card(title: &str) -> (GtkBox, Label, Label) {
    let card = card();
    card.append(&micro(title));
    let value = label("", "usage-value");
    value.set_margin_top(8);
    card.append(&value);
    let caption = label("", "usage-caption");
    card.append(&caption);
    (card, value, caption)
}

fn fact(title: &str) -> (GtkBox, Label) {
    let cell = GtkBox::new(Orientation::Vertical, 0);
    cell.add_css_class("usage-fact");
    cell.set_hexpand(true);
    // The strip is one row of three cells: on a narrow window it truncates
    // rather than forcing the page wider than the settings content column.
    let name = micro(title);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    cell.append(&name);
    let value = label("", "usage-fact-value");
    value.set_margin_top(6);
    value.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    cell.append(&value);
    (cell, value)
}

fn fact_divider() -> Separator {
    let divider = Separator::new(Orientation::Vertical);
    divider.add_css_class("usage-fact-divider");
    divider
}

impl View {
    fn refresh(&self) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let mut config = load_config();
        if *self.session.borrow() != config.cloud_api_token {
            self.has_snapshot.set(false);
        }
        *self.session.borrow_mut() = config.cloud_api_token.clone();
        if !is_cloud_logged_in(&config) {
            self.has_snapshot.set(false);
            self.states.set_visible_child_name("signed-out");
            return;
        }
        if !self.has_snapshot.get() {
            self.show_skeleton();
        }
        self.loading_status.remove_css_class("usage-status-error");
        self.loading_status.set_text(&if self.has_snapshot.get() {
            t("Refreshing...")
        } else {
            t("Loading usage...")
        });
        self.loading_status.set_visible(true);
        self.spinner.set_visible(true);
        self.spinner.start();
        self.states.set_visible_child_name("dashboard");
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = usage::fetch(&mut config);
            let _ = tx.send_blocking((result, config.cloud_api_token));
        });
        let view = self.clone();
        glib::spawn_future_local(async move {
            let Ok((result, token)) = rx.recv().await else {
                return;
            };
            if view.generation.get() != generation
                || !view.root.upgrade().is_some_and(|root| root.is_mapped())
            {
                return;
            }
            let current = load_config();
            if !is_cloud_logged_in(&current) {
                view.has_snapshot.set(false);
                view.states.set_visible_child_name("signed-out");
                return;
            }
            if current.cloud_api_token != token {
                view.refresh();
                return;
            }
            *view.session.borrow_mut() = token;
            view.spinner.stop();
            view.spinner.set_visible(false);
            match result {
                Ok(snapshot) => {
                    view.render(&snapshot);
                    view.has_snapshot.set(true);
                    view.loading_status.set_visible(false);
                    view.states.set_visible_child_name("dashboard");
                }
                Err(error) => {
                    if view.has_snapshot.get() && matches!(error, UsageError::Unavailable) {
                        view.loading_status.add_css_class("usage-status-error");
                        view.loading_status
                            .set_text(&t("Could not refresh. Showing previous values."));
                        return;
                    }
                    view.has_snapshot.set(false);
                    view.error.set_text(&match error {
                        UsageError::SignedOut | UsageError::Expired => t("Your session has expired. Connect your account again in Cloud settings."),
                        UsageError::Unavailable => t("We couldn't load your usage. Check your connection and try again."),
                    });
                    view.states.set_visible_child_name("error");
                }
            }
        });
    }

    fn skeleton_labels(&self) -> [(&Label, i32, i32); 13] {
        [
            (&self.banner_title, 210, 13),
            (&self.banner_detail, 280, 12),
            (&self.account, 150, 20),
            (&self.plan, 72, 24),
            (&self.storage, 128, 34),
            (&self.storage_caption, 78, 15),
            (&self.storage_floor, 30, 12),
            (&self.storage_ceiling, 44, 12),
            (&self.storage_percent, 96, 13),
            (&self.available, 90, 24),
            (&self.available_caption, 140, 13),
            (&self.uploads, 46, 24),
            (&self.uploads_caption, 96, 13),
        ]
    }

    fn show_skeleton(&self) {
        self.banner.set_visible(false);
        self.set_banner_class(Some("usage-banner-pending"));
        for (label, width, height) in self.skeleton_labels() {
            label.set_text("");
            label.set_size_request(width, height);
            label.add_css_class("usage-skeleton");
        }
        for bar in [&self.uploads_bar, &self.storage_bar] {
            bar.set_fraction(0.0);
            bar.set_opacity(1.0);
            bar.add_css_class("usage-progress-loading");
        }
        self.retention.set_text("");
        self.checked.set_text("");
        self.retention.add_css_class("usage-skeleton");
        self.checked.add_css_class("usage-skeleton");
        self.retention.set_size_request(60, 14);
        self.checked.set_size_request(150, 14);
    }

    fn set_banner_class(&self, class: Option<&str>) {
        for known in [
            "usage-banner-pending",
            "usage-banner-ok",
            "usage-banner-warn",
            "usage-banner-full",
        ] {
            self.banner.remove_css_class(known);
        }
        if let Some(class) = class {
            self.banner.add_css_class(class);
        }
    }

    fn render(&self, snapshot: &UsageSnapshot) {
        let readings = Readings::of(snapshot);
        for label in [
            &self.banner_title,
            &self.banner_detail,
            &self.plan,
            &self.account,
            &self.storage,
            &self.storage_caption,
            &self.storage_floor,
            &self.storage_ceiling,
            &self.storage_percent,
            &self.available,
            &self.available_caption,
            &self.uploads,
            &self.uploads_caption,
            &self.retention,
            &self.checked,
        ] {
            label.remove_css_class("usage-skeleton");
            label.set_size_request(-1, -1);
        }
        self.storage_bar.remove_css_class("usage-progress-loading");
        self.uploads_bar.remove_css_class("usage-progress-loading");

        self.set_banner_class(Some(readings.banner.class()));
        self.banner_title.set_text(&readings.banner.title());
        self.banner_detail.set_text(&readings.banner.detail());
        self.banner
            .set_visible(!matches!(readings.banner, Banner::Healthy { .. }));

        let tier = if snapshot.tier.trim().is_empty() {
            t("Plan unavailable")
        } else {
            tfmt("{plan} plan", &[("plan", &capitalize(&snapshot.tier))])
        };
        self.plan.set_text(&tier);
        let connected = t("Connected");
        self.account.set_text(if snapshot.email.trim().is_empty() {
            &connected
        } else {
            &snapshot.email
        });

        self.storage.set_text(&readings.storage);
        self.storage_caption.set_text(&readings.storage_limit);
        self.storage_floor.set_text(&readings.storage_floor);
        self.storage_ceiling
            .set_text(&format_bytes(snapshot.limits.max_storage_bytes));
        self.storage_percent.set_text(&readings.storage_percent);
        self.storage_bar.set_fraction(ratio(
            snapshot.usage.storage_bytes,
            snapshot.limits.max_storage_bytes,
        ));

        self.available.set_text(&readings.available);
        self.available_caption.set_text(&tfmt(
            "of {limit} allowance",
            &[("limit", &format_bytes(snapshot.limits.max_storage_bytes))],
        ));

        self.uploads.set_text(&readings.uploads);
        self.uploads_caption.set_text(&readings.uploads_caption);
        self.uploads_percent.set_text(&readings.uploads_percent);
        if snapshot.limits.max_uploads < 0 {
            self.uploads_bar.set_opacity(0.0);
        } else {
            self.uploads_bar
                .set_fraction(ratio(snapshot.usage.uploads, snapshot.limits.max_uploads));
            self.uploads_bar.set_opacity(1.0);
        }

        self.retention.set_text(&readings.retention);
        self.checked.set_text(&readings.checked);
    }
}

fn percent_used(value: i64, limit: i64) -> String {
    if limit <= 0 {
        return t("Allowance unavailable");
    }
    let percent = value.max(0) as f64 / limit as f64 * 100.0;
    if percent > 0.0 && percent < 0.1 {
        t("Less than 0.1% used")
    } else if percent < 1.0 && percent > 0.0 {
        tfmt("{percent}% used", &[("percent", &format!("{percent:.1}"))])
    } else {
        tfmt("{percent}% used", &[("percent", &format!("{percent:.0}"))])
    }
}

fn ratio(value: i64, limit: i64) -> f64 {
    if limit <= 0 {
        0.0
    } else {
        (value as f64 / limit as f64).clamp(0.0, 1.0)
    }
}

fn format_bytes(bytes: i64) -> String {
    let mut value = bytes.max(0) as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB", "TB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    if unit == "B" || value >= 100.0 || value.fract() == 0.0 {
        format!("{value:.0} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

pub(crate) fn capitalize(value: &str) -> String {
    let mut chars = value.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        include_str!("usage.css"),
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn snapshot(used: i64, limit: i64, uploads: i64, max_uploads: i64) -> UsageSnapshot {
        UsageSnapshot {
            email: "user@example.com".into(),
            tier: "pro".into(),
            usage: usage::Usage {
                uploads,
                storage_bytes: used,
            },
            limits: usage::Limits {
                max_uploads,
                max_storage_bytes: limit,
                retention_days: 90,
            },
        }
    }

    fn empty_view() -> View {
        let root = GtkBox::new(Orientation::Vertical, 0);
        View {
            root: root.downgrade(),
            states: Stack::new(),
            banner: GtkBox::new(Orientation::Vertical, 0),
            banner_title: Label::new(None),
            banner_detail: Label::new(None),
            account: Label::new(None),
            plan: Label::new(None),
            storage: Label::new(None),
            storage_caption: Label::new(None),
            storage_percent: Label::new(None),
            storage_bar: ProgressBar::new(),
            storage_floor: Label::new(None),
            storage_ceiling: Label::new(None),
            available: Label::new(None),
            available_caption: Label::new(None),
            uploads: Label::new(None),
            uploads_caption: Label::new(None),
            uploads_percent: Label::new(None),
            uploads_bar: ProgressBar::new(),
            retention: Label::new(None),
            checked: Label::new(None),
            loading_status: Label::new(None),
            spinner: Spinner::new(),
            error: Label::new(None),
            generation: Rc::new(Cell::new(0)),
            session: Rc::new(RefCell::new(String::new())),
            has_snapshot: Rc::new(Cell::new(false)),
        }
    }

    #[test]
    fn signed_out_content_is_centered_in_the_scrolled_page() {
        let Some(result) = crate::test_support::with_gtk(|| {
            let root = build(|| {});
            let states = root.first_child().unwrap().downcast::<Stack>().unwrap();
            let signed_out = states.child_by_name("signed-out").unwrap();
            let heading = signed_out
                .first_child()
                .unwrap()
                .downcast::<Label>()
                .unwrap();
            let hint = heading.next_sibling().unwrap().downcast::<Label>().unwrap();
            let button = hint.next_sibling().unwrap().downcast::<Button>().unwrap();

            let column = GtkBox::new(Orientation::Vertical, 0);
            column.set_margin_top(4);
            column.set_margin_bottom(24);
            column.set_margin_start(20);
            column.set_margin_end(20);
            column.append(&root);
            let scroller = gtk4::ScrolledWindow::new();
            scroller.set_child(Some(&column));
            scroller.measure(Orientation::Horizontal, -1);
            scroller.measure(Orientation::Vertical, 800);
            scroller.allocate(800, 600, -1, None);
            let bounds = signed_out.compute_bounds(&scroller).unwrap();
            let center = (
                bounds.x() + bounds.width() / 2.0,
                bounds.y() + bounds.height() / 2.0,
            );
            scroller.measure(Orientation::Vertical, 360);
            scroller.allocate(360, 400, -1, None);
            let small_bounds = signed_out.compute_bounds(&scroller).unwrap();
            (
                root.vexpands(),
                states.vexpands(),
                heading.xalign(),
                hint.xalign(),
                button.halign(),
                center,
                (small_bounds.x(), small_bounds.width()),
            )
        }) else {
            return;
        };
        let (root_expands, states_expand, heading_x, hint_x, alignment, center, small) = result;
        assert!(root_expands && states_expand);
        assert_eq!((heading_x, hint_x, alignment), (0.5, 0.5, Align::Center));
        assert!((center.0 - 400.0).abs() < 2.0);
        assert!((center.1 - 300.0).abs() < 20.0);
        assert!(small.0 >= 0.0 && small.0 + small.1 <= 360.0);
    }

    #[test]
    fn usage_formatting_is_bounded_and_handles_zero() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024 * 1024 * 100), "100 MB");
        assert_eq!(format_bytes(1024 * 1024 * 95 + 512 * 1024), "95.5 MB");
        assert_eq!(ratio(20, 10), 1.0);
        assert_eq!(ratio(1, 0), 0.0);
        assert_eq!(capitalize(" pro "), "Pro");
        assert_eq!(percent_used(0, 1000), "0% used");
        assert_eq!(percent_used(1, 100_000), "Less than 0.1% used");
        assert_eq!(percent_used(95_500, 100_000_000), "Less than 0.1% used");
        assert_eq!(percent_used(160, 100_000), "0.2% used");
        assert_eq!(percent_used(125, 100), "125% used");
        assert_eq!(percent_used(0, 0), "Allowance unavailable");
    }

    #[test]
    fn banner_reports_the_worst_allowance_state() {
        assert_eq!(
            Banner::of(&snapshot(130_000_000, 10_000_000_000, 0, -1)),
            Banner::Healthy {
                remaining: 9_870_000_000
            }
        );
        assert_eq!(
            Banner::of(&snapshot(9_000_000_000, 10_000_000_000, 0, -1)),
            Banner::NearFull {
                remaining: 1_000_000_000,
                limit: 10_000_000_000
            }
        );
        assert_eq!(
            Banner::of(&snapshot(12_000_000_000, 10_000_000_000, 0, -1)),
            Banner::Full
        );
        assert_eq!(
            Banner::of(&snapshot(1_000, 10_000_000_000, 20, 20)),
            Banner::UploadsCapped
        );
        assert_eq!(
            Banner::of(&snapshot(1_000, 10_000_000_000, 5, 20)),
            Banner::Healthy {
                remaining: 9_999_999_000
            }
        );
        assert_eq!(
            Banner::of(&snapshot(12_000_000_000, 10_000_000_000, 20, 20)),
            Banner::Full
        );
    }

    #[test]
    fn readings_describe_the_account_without_repeating_it() {
        const MB: i64 = 1024 * 1024;
        const GB: i64 = 1024 * MB;
        let readings = Readings::of(&snapshot(95 * MB + MB / 2, 100 * GB, 0, -1));
        assert_eq!(readings.storage, "95.5 MB");
        assert_eq!(readings.storage_limit, "of 100 GB");
        assert_eq!(readings.storage_percent, "Less than 0.1% used");
        assert_eq!(readings.available, "99.9 GB");
        assert_eq!(readings.uploads_caption, "No monthly limit");
        assert_eq!(readings.uploads_percent, "");
        assert_eq!(readings.retention, "90 days");
        assert!(!readings.checked.is_empty());

        let capped = Readings::of(&snapshot(1_000, 1_000_000, 4, 20));
        assert_eq!(capped.uploads_caption, "of 20 this month");
        assert_eq!(capped.uploads_percent, "20% used");
    }

    #[test]
    fn skeleton_is_replaced_by_actual_usage() {
        let Some(result) = crate::test_support::with_gtk(|| {
            let view = empty_view();
            view.show_skeleton();
            let loading = (
                view.storage.has_css_class("usage-skeleton"),
                view.uploads_bar.has_css_class("usage-progress-loading"),
                view.banner.has_css_class("usage-banner-pending"),
            );
            view.render(&UsageSnapshot {
                email: "user@example.com".into(),
                tier: "pro".into(),
                usage: usage::Usage {
                    uploads: 7,
                    storage_bytes: 95_500_000,
                },
                limits: usage::Limits {
                    max_uploads: -1,
                    max_storage_bytes: 100_000_000_000,
                    retention_days: 90,
                },
            });
            (
                loading,
                view.storage.has_css_class("usage-skeleton"),
                view.uploads_bar.has_css_class("usage-progress-loading"),
                view.uploads.text().to_string(),
                view.uploads_caption.text().to_string(),
                view.storage_percent.text().to_string(),
                view.uploads_bar.opacity(),
                view.banner.has_css_class("usage-banner-ok"),
                view.banner.has_css_class("usage-banner-pending"),
                view.retention.has_css_class("usage-skeleton"),
                view.retention.text().to_string(),
            )
        }) else {
            return;
        };
        assert_eq!(result.0, (true, true, true));
        assert!(!result.1);
        assert!(!result.2);
        assert_eq!(result.3, "7");
        assert_eq!(result.4, "No monthly limit");
        assert_eq!(result.5, "Less than 0.1% used");
        assert_eq!(result.6, 0.0);
        assert!(result.7 && !result.8);
        assert!(!result.9);
        assert_eq!(result.10, "90 days");
    }

    #[test]
    fn usage_styles_parse() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(RefCell::new(Vec::new()));
            let captured = Rc::clone(&errors);
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(include_str!("usage.css"));
            let result = errors.borrow().clone();
            result
        }) else {
            return;
        };
        assert!(errors.is_empty(), "CSS parsing errors: {errors:?}");
    }

    #[test]
    fn the_hero_card_never_outgrows_the_stats_column() {
        let Some(heights) = crate::test_support::with_gtk(|| {
            let (root, view) = build_view(|| {});
            let states = root.first_child().unwrap().downcast::<Stack>().unwrap();
            states.set_visible_child_name("dashboard");
            let data = snapshot(95 * 1024 * 1024, 100 * 1024 * 1024 * 1024, 0, -1);
            view.render(&data);
            let dashboard = states
                .child_by_name("dashboard")
                .unwrap()
                .downcast::<GtkBox>()
                .unwrap();
            let overview = dashboard.first_child().unwrap();
            let metrics = overview.next_sibling().unwrap().next_sibling().unwrap();
            let hero = metrics.first_child().unwrap();
            let column = hero.next_sibling().unwrap();
            (
                hero.measure(Orientation::Vertical, 310).1,
                column.measure(Orientation::Vertical, 310).1,
            )
        }) else {
            return;
        };
        let (hero, column) = heights;
        assert!(
            hero <= column,
            "the hero card ({hero}px) drives the row taller than the stats column ({column}px)"
        );
    }
}

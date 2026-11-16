use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation, ProgressBar, Spinner, Stack,
};

use crate::cloud::usage::{self, UsageError, UsageSnapshot};
use crate::config::{is_cloud_logged_in, load_config, resolve_cloud_backend_url, AppConfig};
use crate::i18n::{t, tfmt};

const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";

struct Readings {
    email: String,
    plan: String,
    storage_used: String,
    storage_available: String,
    storage_fraction: f64,
    uploads: String,
    uploads_fraction: Option<f64>,
    retention: String,
}

fn session_key(config: &AppConfig) -> (String, String) {
    (
        config.cloud_api_token.clone(),
        resolve_cloud_backend_url(config),
    )
}

fn remaining(used: i64, limit: i64) -> i64 {
    limit.saturating_sub(used).max(0)
}

fn bar_fraction(value: i64, limit: i64) -> f64 {
    if limit <= 0 {
        0.0
    } else {
        (value as f64 / limit as f64).clamp(0.0, 1.0)
    }
}

fn uploads_reading(used: i64, max: i64) -> (String, Option<f64>) {
    if max < 0 {
        (format!("{used} · {}", t("Unlimited")), None)
    } else {
        (
            tfmt(
                "{used} of {limit}",
                &[("used", &used.to_string()), ("limit", &max.to_string())],
            ),
            Some(bar_fraction(used, max)),
        )
    }
}

fn format_bytes(bytes: i64) -> String {
    let mut value = bytes.max(0) as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
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

fn capitalize(value: &str) -> String {
    let mut chars = value.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl Readings {
    fn of(snapshot: &UsageSnapshot) -> Self {
        let used = snapshot.usage.storage_bytes;
        let limit = snapshot.limits.max_storage_bytes;
        let (uploads, uploads_fraction) =
            uploads_reading(snapshot.usage.uploads, snapshot.limits.max_uploads);
        Self {
            email: if snapshot.email.trim().is_empty() {
                t("Connected")
            } else {
                snapshot.email.clone()
            },
            plan: if snapshot.tier.trim().is_empty() {
                t("Plan unavailable")
            } else {
                tfmt("{plan} plan", &[("plan", &capitalize(&snapshot.tier))])
            },
            storage_used: tfmt(
                "{used} of {limit}",
                &[
                    ("used", &format_bytes(used)),
                    ("limit", &format_bytes(limit)),
                ],
            ),
            storage_available: format_bytes(remaining(used, limit)),
            storage_fraction: bar_fraction(used, limit),
            uploads,
            uploads_fraction,
            retention: match snapshot.limits.retention_days {
                days if days > 0 => tfmt("{days} days", &[("days", &days.to_string())]),
                _ => t("Not set"),
            },
        }
    }
}

struct RequestState {
    generation: u64,
    active: bool,
    session: (String, String),
}

impl RequestState {
    fn begin(&mut self, session: (String, String)) -> Option<u64> {
        if self.active && self.session == session {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.active = true;
        self.session = session;
        Some(self.generation)
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.active = false;
    }

    fn complete(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.active = false;
        true
    }
}

#[derive(Clone)]
struct View {
    root: glib::WeakRef<GtkBox>,
    toolbar: GtkBox,
    states: Stack,
    spinner: Spinner,
    status: Label,
    refresh_button: Button,
    billing_button: Button,
    email: Label,
    plan: Label,
    storage_used: Label,
    storage_bar: ProgressBar,
    storage_available: Label,
    uploads: Label,
    uploads_bar: ProgressBar,
    uploads_bar_row: GtkBox,
    retention: Label,
    updated: Label,
    error: Label,
    request: Rc<RefCell<RequestState>>,
    has_snapshot: Rc<Cell<bool>>,
}

pub struct UsageWidgets {
    pub section: GtkBox,
    pub billing_button: Button,
}

pub fn build(go_cloud: impl Fn() + Clone + 'static, refresh_button: &Button) -> UsageWidgets {
    build_view(go_cloud, refresh_button).0
}

fn group_title(text: &str) -> Label {
    let title = Label::new(Some(&t(text)));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_margin_bottom(8);
    title
}

fn frame() -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_margin_bottom(24);
    frame.set_margin_start(4);
    frame.set_margin_end(4);
    frame
}

fn row(content: &impl IsA<gtk4::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row.append(content);
    row
}

fn value_row(name: &str) -> (GtkBox, Label) {
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.set_hexpand(true);
    let label = Label::new(Some(&t(name)));
    label.set_xalign(0.0);
    let value = Label::new(None);
    value.add_css_class("settings-mono-value");
    value.set_hexpand(true);
    value.set_halign(Align::Fill);
    value.set_xalign(1.0);
    value.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    hbox.append(&label);
    hbox.append(&value);
    (hbox, value)
}

fn build_view(
    go_cloud: impl Fn() + Clone + 'static,
    refresh_button: &Button,
) -> (UsageWidgets, Rc<View>) {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let toolbar = GtkBox::new(Orientation::Horizontal, 8);
    toolbar.set_margin_bottom(4);
    let spinner = Spinner::new();
    spinner.set_visible(false);
    let status = Label::new(None);
    status.add_css_class("usage-status");
    status.set_xalign(0.0);
    status.set_visible(false);
    toolbar.append(&spinner);
    toolbar.append(&status);
    root.append(&toolbar);

    let states = Stack::new();
    states.set_transition_type(gtk4::StackTransitionType::Crossfade);
    states.set_vhomogeneous(false);
    states.set_vexpand(true);
    root.append(&states);

    let signed_out = GtkBox::new(Orientation::Vertical, 12);
    signed_out.set_halign(Align::Center);
    signed_out.set_valign(Align::Center);
    signed_out.set_vexpand(true);
    let heading = Label::new(Some(&t("Connect to see your usage")));
    heading.add_css_class("settings-group-title");
    heading.add_css_class("usage-state-title");
    heading.set_xalign(0.5);
    let hint = Label::new(Some(&t("Sign in to view your uploads, storage and plan.")));
    hint.add_css_class("settings-sub-option");
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

    let loading = GtkBox::new(Orientation::Vertical, 12);
    loading.set_halign(Align::Center);
    loading.set_valign(Align::Center);
    loading.set_vexpand(true);
    let loading_spinner = Spinner::new();
    loading_spinner.start();
    loading_spinner.set_size_request(24, 24);
    let loading_label = Label::new(Some(&t("Loading usage…")));
    loading_label.add_css_class("settings-sub-option");
    loading.append(&loading_spinner);
    loading.append(&loading_label);
    states.add_named(&loading, Some("loading"));

    let failed = GtkBox::new(Orientation::Vertical, 12);
    failed.set_halign(Align::Center);
    failed.set_valign(Align::Center);
    failed.set_vexpand(true);
    let failed_title = Label::new(Some(&t("Usage is unavailable")));
    failed_title.add_css_class("settings-group-title");
    failed_title.add_css_class("usage-state-title");
    failed_title.set_xalign(0.5);
    failed.append(&failed_title);
    let error = Label::new(None);
    error.add_css_class("settings-sub-option");
    error.set_xalign(0.5);
    error.set_wrap(true);
    error.set_max_width_chars(46);
    error.set_justify(gtk4::Justification::Center);
    failed.append(&error);
    let retry = Button::with_label(&t("Retry"));
    retry.add_css_class("settings-primary-btn");
    retry.set_halign(Align::Center);
    retry.set_margin_top(8);
    failed.append(&retry);
    let sign_in = Button::with_label(&t("Go to Cloud settings"));
    sign_in.set_halign(Align::Center);
    sign_in.connect_clicked(move |_| go_cloud());
    failed.append(&sign_in);
    states.add_named(&failed, Some("error"));

    let data = GtkBox::new(Orientation::Vertical, 0);
    data.set_hexpand(true);

    data.append(&group_title("Account"));
    let account_frame = frame();
    let (email_row, email) = value_row("Email");
    email.set_max_width_chars(34);
    account_frame.append(&row(&email_row));
    let (plan_row, plan) = value_row("Plan");
    account_frame.append(&row(&plan_row));
    data.append(&account_frame);

    data.append(&group_title("Cloud storage"));
    let storage_frame = frame();
    let (storage_row, storage_used) = value_row("Used");
    storage_frame.append(&row(&storage_row));
    let storage_bar = ProgressBar::new();
    storage_bar.add_css_class("usage-progress");
    storage_bar.set_hexpand(true);
    storage_frame.append(&row(&storage_bar));
    let (available_row, storage_available) = value_row("Available");
    storage_frame.append(&row(&available_row));
    data.append(&storage_frame);

    data.append(&group_title("Uploads"));
    let uploads_frame = frame();
    let (uploads_row, uploads) = value_row("This month");
    uploads_frame.append(&row(&uploads_row));
    let uploads_bar = ProgressBar::new();
    uploads_bar.add_css_class("usage-progress");
    uploads_bar.set_hexpand(true);
    let uploads_bar_row = row(&uploads_bar);
    uploads_frame.append(&uploads_bar_row);
    data.append(&uploads_frame);

    data.append(&group_title("Details"));
    let details_frame = frame();
    let (retention_row, retention) = value_row("Retention");
    details_frame.append(&row(&retention_row));
    let (updated_row, updated) = value_row("Last updated");
    details_frame.append(&row(&updated_row));
    data.append(&details_frame);

    let billing = Button::with_label(&t("Manage billing"));
    billing.add_css_class("settings-primary-btn");
    billing.set_valign(Align::Center);
    billing.set_visible(false);
    billing.connect_clicked(|_| {
        std::thread::spawn(|| {
            if let Err(error) = crate::utils::open::open_url(BILLING_URL) {
                eprintln!("[usage] Could not open billing: {error}");
            }
        });
    });

    states.add_named(&data, Some("data"));

    let view = Rc::new(View {
        root: root.downgrade(),
        toolbar,
        states,
        spinner,
        status,
        refresh_button: refresh_button.clone(),
        billing_button: billing.clone(),
        email,
        plan,
        storage_used,
        storage_bar,
        storage_available,
        uploads,
        uploads_bar,
        uploads_bar_row,
        retention,
        updated,
        error,
        request: Rc::new(RefCell::new(RequestState {
            generation: 0,
            active: false,
            session: (String::new(), String::new()),
        })),
        has_snapshot: Rc::new(Cell::new(false)),
    });
    refresh_button.connect_clicked({
        let view = Rc::downgrade(&view);
        move |_| {
            if let Some(view) = view.upgrade() {
                view.refresh();
            }
        }
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
        let request = Rc::clone(&view.request);
        move |_| request.borrow_mut().invalidate()
    });
    let weak = Rc::downgrade(&view);
    glib::timeout_add_seconds_local(2, move || {
        let Some(view) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mapped = view.root.upgrade().is_some_and(|root| root.is_mapped());
        let changed = mapped && view.request.borrow().session != session_key(&load_config());
        if changed {
            view.refresh();
        }
        glib::ControlFlow::Continue
    });
    (
        UsageWidgets {
            section: root,
            billing_button: billing,
        },
        view,
    )
}

impl View {
    fn show_state(&self, name: &str) {
        self.states.set_visible_child_name(name);
        self.toolbar
            .set_visible(name != "signed-out" && name != "data");
        self.billing_button.set_visible(name == "data");
    }

    fn refresh(&self) {
        let config = load_config();
        let key = session_key(&config);
        if !is_cloud_logged_in(&config) {
            let mut request = self.request.borrow_mut();
            request.invalidate();
            request.session = key;
            drop(request);
            self.has_snapshot.set(false);
            self.refresh_button.set_sensitive(true);
            self.spinner.stop();
            self.spinner.set_visible(false);
            self.status.set_visible(false);
            self.show_state("signed-out");
            return;
        }
        let mut request = self.request.borrow_mut();
        if request.session != key {
            self.has_snapshot.set(false);
        }
        let Some(generation) = request.begin(key) else {
            return;
        };
        drop(request);

        self.refresh_button.set_sensitive(false);
        self.status.remove_css_class("usage-status-error");
        self.status.set_text(&if self.has_snapshot.get() {
            t("Refreshing…")
        } else {
            t("Loading usage…")
        });
        self.status.set_visible(true);
        self.spinner.set_visible(true);
        self.spinner.start();
        self.show_state(if self.has_snapshot.get() {
            "data"
        } else {
            "loading"
        });
        self.toolbar.set_visible(true);

        let mut config = config;
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = usage::fetch(&mut config);
            let _ = tx.send_blocking((result, session_key(&config)));
        });
        let view = self.clone();
        glib::spawn_future_local(async move {
            let Ok((result, key)) = rx.recv().await else {
                return;
            };
            if !view.request.borrow_mut().complete(generation) {
                return;
            }
            if !view.root.upgrade().is_some_and(|root| root.is_mapped()) {
                return;
            }
            let current = load_config();
            if !is_cloud_logged_in(&current) {
                view.has_snapshot.set(false);
                view.request.borrow_mut().session = session_key(&current);
                view.refresh_button.set_sensitive(true);
                view.spinner.stop();
                view.spinner.set_visible(false);
                view.status.set_visible(false);
                view.show_state("signed-out");
                return;
            }
            let current_key = session_key(&current);
            if current_key != key {
                view.request.borrow_mut().session = current_key;
                view.has_snapshot.set(false);
                view.refresh();
                return;
            }
            view.request.borrow_mut().session = key;
            view.refresh_button.set_sensitive(true);
            view.spinner.stop();
            view.spinner.set_visible(false);
            match result {
                Ok(snapshot) => {
                    view.render(&snapshot);
                    view.has_snapshot.set(true);
                    view.status.set_visible(false);
                    view.show_state("data");
                }
                Err(error) => {
                    if view.has_snapshot.get() && matches!(error, UsageError::Unavailable) {
                        view.status.add_css_class("usage-status-error");
                        view.status
                            .set_text(&t("Could not refresh. Showing previous values."));
                        return;
                    }
                    view.has_snapshot.set(false);
                    view.error.set_text(&match error {
                        UsageError::SignedOut | UsageError::Expired => t(
                            "Your session has expired. Connect your account again in Cloud settings.",
                        ),
                        UsageError::Unavailable => t(
                            "We couldn't load your usage. Check your connection and try again.",
                        ),
                    });
                    view.status.set_visible(false);
                    view.show_state("error");
                }
            }
        });
    }

    fn render(&self, snapshot: &UsageSnapshot) {
        let readings = Readings::of(snapshot);
        self.email.set_text(&readings.email);
        self.email.set_tooltip_text(Some(&readings.email));
        self.plan.set_text(&readings.plan);
        self.storage_used.set_text(&readings.storage_used);
        self.storage_bar.set_fraction(readings.storage_fraction);
        self.storage_available.set_text(&readings.storage_available);
        self.uploads.set_text(&readings.uploads);
        match readings.uploads_fraction {
            Some(fraction) => {
                self.uploads_bar.set_fraction(fraction);
                self.uploads_bar_row.set_visible(true);
            }
            None => self.uploads_bar_row.set_visible(false),
        }
        self.retention.set_text(&readings.retention);
        self.updated
            .set_text(&chrono::Local::now().format("%b %-d, %H:%M").to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_with_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1 GiB");
        assert_eq!(format_bytes(-5), "0 B");
        let max = format_bytes(i64::MAX);
        assert!(max.ends_with(" EiB"), "i64::MAX formatted as {max}");
    }

    #[test]
    fn remaining_never_goes_below_zero() {
        assert_eq!(remaining(4, 10), 6);
        assert_eq!(remaining(10, 10), 0);
        assert_eq!(remaining(11, 10), 0);
    }

    #[test]
    fn bar_fraction_is_bounded() {
        assert_eq!(bar_fraction(5, 10), 0.5);
        assert_eq!(bar_fraction(15, 10), 1.0);
        assert_eq!(bar_fraction(5, 0), 0.0);
        assert_eq!(bar_fraction(5, -1), 0.0);
    }

    #[test]
    fn uploads_reading_hides_the_bar_only_when_unlimited() {
        let (text, fraction) = uploads_reading(7, -1);
        assert!(text.contains("7"));
        assert!(fraction.is_none());
        assert_eq!(uploads_reading(0, 0), ("0 of 0".to_string(), Some(0.0)));
        assert_eq!(uploads_reading(25, 20), ("25 of 20".to_string(), Some(1.0)));
        assert_eq!(uploads_reading(7, 20), ("7 of 20".to_string(), Some(0.35)));
    }

    #[test]
    fn session_key_changes_with_token_or_backend() {
        let config = AppConfig {
            cloud_api_token: "a".to_string(),
            cloud_backend_url: "http://127.0.0.1:1".to_string(),
            ..AppConfig::default()
        };
        let key = session_key(&config);
        let mut rotated = config.clone();
        rotated.cloud_api_token = "b".to_string();
        assert_ne!(key, session_key(&rotated));
        let mut other_backend = config.clone();
        other_backend.cloud_backend_url = "http://127.0.0.1:2".to_string();
        assert_ne!(key, session_key(&other_backend));
        let same = config.clone();
        assert_eq!(key, session_key(&same));
    }

    #[test]
    fn readings_label_zero_quota_and_missing_retention() {
        let snapshot = UsageSnapshot {
            email: "user@example.com".into(),
            tier: "pro".into(),
            usage: usage::Usage {
                uploads: 0,
                storage_bytes: 0,
            },
            limits: usage::Limits {
                max_uploads: 0,
                max_storage_bytes: 1000,
                retention_days: 0,
            },
        };
        let readings = Readings::of(&snapshot);
        assert_eq!(readings.uploads, "0 of 0");
        assert_eq!(readings.uploads_fraction, Some(0.0));
        assert_eq!(readings.retention, "Not set");
        assert_eq!(readings.storage_used, "0 B of 1000 B");
        assert_eq!(readings.storage_available, "1000 B");
    }

    fn request_state() -> RequestState {
        RequestState {
            generation: 0,
            active: false,
            session: (String::new(), String::new()),
        }
    }

    #[test]
    fn a_second_begin_for_the_same_session_is_ignored() {
        let mut state = request_state();
        let key = ("token-a".to_string(), "backend".to_string());
        let generation = state.begin(key.clone()).unwrap();
        assert!(state.begin(key).is_none());
        assert_eq!(state.generation, generation);
        assert!(state.active);
    }

    #[test]
    fn invalidation_lets_a_remap_start_a_new_request() {
        let mut state = request_state();
        let key = ("token-a".to_string(), "backend".to_string());
        let first = state.begin(key.clone()).unwrap();
        state.invalidate();
        assert!(!state.active);
        let second = state.begin(key).unwrap();
        assert_ne!(first, second);
        assert!(!state.complete(first));
        assert!(state.active);
        assert!(state.complete(second));
        assert!(!state.active);
    }

    #[test]
    fn a_session_switch_replaces_the_pending_request() {
        let mut state = request_state();
        let first = state
            .begin(("token-a".to_string(), "backend".to_string()))
            .unwrap();
        let second = state
            .begin(("token-b".to_string(), "backend".to_string()))
            .unwrap();
        assert_ne!(first, second);
        assert!(!state.complete(first));
        assert!(state.active);
        assert!(state.complete(second));
    }

    #[test]
    fn a_stale_completion_cannot_clear_the_newer_request() {
        let mut state = request_state();
        let stale = state
            .begin(("token-a".to_string(), "backend".to_string()))
            .unwrap();
        let current = state
            .begin(("token-a".to_string(), "other".to_string()))
            .unwrap();
        assert!(!state.complete(stale));
        assert!(state.active);
        assert!(state.complete(current));
        assert!(!state.active);
    }

    #[test]
    fn billing_lives_in_the_page_header_and_refresh_is_external() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let refresh = Button::from_icon_name("view-refresh-symbolic");
            let (widgets, view) = build_view(|| {}, &refresh);
            assert!(widgets.billing_button.has_css_class("settings-primary-btn"));
            assert!(widgets.billing_button.parent().is_none());
            assert_eq!(view.refresh_button, refresh);
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
    }
}

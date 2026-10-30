//! Cloud usage in Settings. All values come from the authenticated account API.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, FlowBox, Label, Orientation, ProgressBar,
    Spinner, Stack,
};

use crate::cloud::usage::{self, UsageError, UsageSnapshot};
use crate::config::{is_cloud_logged_in, load_config};
use crate::i18n::{t, tfmt};

const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";

#[derive(Clone)]
struct View {
    root: glib::WeakRef<GtkBox>,
    states: Stack,
    plan: Label,
    account: Label,
    uploads: Label,
    uploads_detail: Label,
    uploads_caption: Label,
    uploads_bar: ProgressBar,
    storage: Label,
    storage_detail: Label,
    storage_caption: Label,
    storage_bar: ProgressBar,
    loading_status: Label,
    spinner: Spinner,
    refresh: Button,
    error: Label,
    generation: Rc<Cell<u64>>,
    session: Rc<RefCell<String>>,
    has_snapshot: Rc<Cell<bool>>,
}

pub fn build(go_cloud: impl Fn() + Clone + 'static) -> GtkBox {
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

    let dashboard = GtkBox::new(Orientation::Vertical, 0);
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

    let metrics = FlowBox::new();
    metrics.add_css_class("usage-grid");
    metrics.set_selection_mode(gtk4::SelectionMode::None);
    metrics.set_activate_on_single_click(false);
    metrics.set_min_children_per_line(1);
    metrics.set_max_children_per_line(2);
    metrics.set_homogeneous(true);
    metrics.set_column_spacing(12);
    metrics.set_row_spacing(12);
    metrics.set_margin_top(22);
    let (storage_card, storage, storage_caption, storage_detail, storage_bar) =
        metric("Storage", "Used space");
    let (uploads_card, uploads, uploads_caption, uploads_detail, uploads_bar) =
        metric("Uploads", "This month");
    metrics.insert(&storage_card, -1);
    metrics.insert(&uploads_card, -1);
    dashboard.append(&metrics);

    let footer = GtkBox::new(Orientation::Horizontal, 12);
    footer.add_css_class("usage-footer");
    footer.set_margin_top(18);
    let footer_text = label("Monthly usage resets in UTC.", "usage-muted");
    footer_text.set_hexpand(true);
    footer_text.set_wrap(true);
    footer.append(&footer_text);
    let refresh = Button::with_label(&t("Refresh"));
    refresh.add_css_class("usage-secondary");
    footer.append(&refresh);
    let billing = Button::with_label(&t("Manage billing"));
    billing.add_css_class("usage-secondary");
    billing.connect_clicked(|_| {
        std::thread::spawn(|| {
            if let Err(error) = crate::utils::open::open_url(BILLING_URL) {
                eprintln!("[usage] Could not open billing: {error}");
            }
        });
    });
    footer.append(&billing);
    dashboard.append(&footer);
    states.add_named(&dashboard, Some("dashboard"));

    let view = Rc::new(View {
        root: root.downgrade(),
        states,
        plan,
        account,
        uploads,
        uploads_detail,
        uploads_caption,
        uploads_bar,
        storage,
        storage_detail,
        storage_caption,
        storage_bar,
        loading_status,
        spinner,
        refresh: refresh.clone(),
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
    refresh.connect_clicked({
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
    let view = Rc::downgrade(&view);
    glib::timeout_add_seconds_local(2, move || {
        let Some(view) = view.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mapped = view.root.upgrade().is_some_and(|root| root.is_mapped());
        let changed = mapped && *view.session.borrow() != load_config().cloud_api_token;
        if changed {
            view.refresh();
        }
        glib::ControlFlow::Continue
    });
    root
}

fn label(text: &str, class: &str) -> Label {
    let label = Label::new(Some(&t(text)));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label
}

fn metric(title: &str, subtitle: &str) -> (GtkBox, Label, Label, Label, ProgressBar) {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("usage-metric");
    card.set_hexpand(true);
    card.set_size_request(240, -1);
    let header = GtkBox::new(Orientation::Horizontal, 10);
    let name = label(title, "usage-metric-title");
    name.set_hexpand(true);
    header.append(&name);
    header.append(&label(subtitle, "usage-muted"));
    card.append(&header);
    let value = label("", "usage-value");
    value.set_halign(Align::Start);
    value.set_margin_top(26);
    card.append(&value);
    let caption = label("", "usage-caption");
    caption.set_halign(Align::Start);
    caption.set_margin_top(4);
    card.append(&caption);
    let bar = ProgressBar::new();
    bar.add_css_class("usage-progress");
    bar.set_margin_top(24);
    bar.set_hexpand(true);
    card.append(&bar);
    let detail = label("", "usage-metric-detail");
    detail.set_halign(Align::Start);
    detail.set_margin_top(9);
    card.append(&detail);
    (card, value, caption, detail, bar)
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
        self.refresh.set_sensitive(false);
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
            view.refresh.set_sensitive(true);
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

    fn show_skeleton(&self) {
        for (label, width, height) in [
            (&self.plan, 72, 26),
            (&self.account, 150, 20),
            (&self.uploads, 90, 34),
            (&self.uploads_caption, 110, 16),
            (&self.uploads_detail, 75, 16),
            (&self.storage, 120, 34),
            (&self.storage_caption, 90, 16),
            (&self.storage_detail, 75, 16),
        ] {
            label.set_text("");
            label.set_size_request(width, height);
            label.add_css_class("usage-skeleton");
        }
        for bar in [&self.uploads_bar, &self.storage_bar] {
            bar.set_fraction(0.0);
            bar.set_opacity(1.0);
            bar.add_css_class("usage-progress-loading");
        }
    }

    fn render(&self, snapshot: &UsageSnapshot) {
        for label in [
            &self.plan,
            &self.account,
            &self.uploads,
            &self.uploads_caption,
            &self.uploads_detail,
            &self.storage,
            &self.storage_caption,
            &self.storage_detail,
        ] {
            label.remove_css_class("usage-skeleton");
            label.set_size_request(-1, -1);
        }
        self.storage_bar.remove_css_class("usage-progress-loading");
        self.uploads_bar.remove_css_class("usage-progress-loading");
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
        self.storage
            .set_text(&format_bytes(snapshot.usage.storage_bytes));
        self.storage_caption.set_text(&tfmt(
            "of {limit}",
            &[("limit", &format_bytes(snapshot.limits.max_storage_bytes))],
        ));
        let fraction = ratio(
            snapshot.usage.storage_bytes,
            snapshot.limits.max_storage_bytes,
        );
        self.storage_bar.set_fraction(fraction);
        self.storage_detail.set_text(&percent_used(
            snapshot.usage.storage_bytes,
            snapshot.limits.max_storage_bytes,
        ));
        if snapshot.limits.max_uploads < 0 {
            self.uploads.set_text(&snapshot.usage.uploads.to_string());
            self.uploads_caption.set_text(&t("No monthly limit"));
            self.uploads_detail.set_text("");
            self.uploads_bar.set_opacity(0.0);
        } else {
            self.uploads.set_text(&snapshot.usage.uploads.to_string());
            self.uploads_caption.set_text(&tfmt(
                "of {limit} monthly uploads",
                &[("limit", &snapshot.limits.max_uploads.to_string())],
            ));
            self.uploads_detail.set_text(&percent_used(
                snapshot.usage.uploads,
                snapshot.limits.max_uploads,
            ));
            self.uploads_bar
                .set_fraction(ratio(snapshot.usage.uploads, snapshot.limits.max_uploads));
            self.uploads_bar.set_opacity(1.0);
        }
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

fn capitalize(value: &str) -> String {
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
    provider.load_from_data(include_str!("usage.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn skeleton_is_replaced_by_actual_usage() {
        let Some(result) = crate::test_support::with_gtk(|| {
            let root = GtkBox::new(Orientation::Vertical, 0);
            let view = View {
                root: root.downgrade(),
                states: Stack::new(),
                plan: Label::new(None),
                account: Label::new(None),
                uploads: Label::new(None),
                uploads_detail: Label::new(None),
                uploads_caption: Label::new(None),
                uploads_bar: ProgressBar::new(),
                storage: Label::new(None),
                storage_detail: Label::new(None),
                storage_caption: Label::new(None),
                storage_bar: ProgressBar::new(),
                loading_status: Label::new(None),
                spinner: Spinner::new(),
                refresh: Button::new(),
                error: Label::new(None),
                generation: Rc::new(Cell::new(0)),
                session: Rc::new(RefCell::new(String::new())),
                has_snapshot: Rc::new(Cell::new(false)),
            };
            view.show_skeleton();
            let loading = (
                view.storage.has_css_class("usage-skeleton"),
                view.uploads_bar.has_css_class("usage-progress-loading"),
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
                view.storage_detail.text().to_string(),
                view.uploads_bar.opacity(),
            )
        }) else {
            return;
        };
        assert_eq!(result.0, (true, true));
        assert!(!result.1);
        assert!(!result.2);
        assert_eq!(result.3, "7");
        assert_eq!(result.4, "No monthly limit");
        assert_eq!(result.5, "Less than 0.1% used");
        assert_eq!(result.6, 0.0);
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
}

//! Compact, row-specific shortcut registration feedback.

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, CssProvider, Image, Label, Orientation,
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::shortcuts::ShortcutSettingsWidgets;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Issue {
    action: String,
    binding: String,
    reason: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Status {
    Offline,
    Unavailable(String),
    Paused,
    Active(Vec<Issue>),
}

fn parse_status(status: Option<&Value>, now_ms: u64) -> Status {
    let Some(status) = status else {
        return Status::Offline;
    };
    let timestamp = status["timestamp_ms"].as_u64().unwrap_or(0);
    if timestamp == 0 || now_ms.saturating_sub(timestamp) > 10_000 {
        return Status::Offline;
    }
    if let Some(error) = status["error"].as_str().filter(|error| !error.is_empty()) {
        return Status::Unavailable(error.to_owned());
    }
    if status["suspended"].as_bool() == Some(true) {
        return Status::Paused;
    }
    let Some(errors) = status["errors"].as_array() else {
        return Status::Unavailable("Could not read the shortcut listener status.".into());
    };
    Status::Active(
        errors
            .iter()
            .filter_map(|entry| {
                Some(Issue {
                    action: entry["action"].as_str()?.to_owned(),
                    binding: entry["binding"].as_str()?.to_owned(),
                    reason: entry["reason"].as_str()?.to_owned(),
                })
            })
            .collect(),
    )
}

fn legacy_replacement<'a>(value: &str, old: &str, new: &'a str) -> Option<&'a str> {
    value.trim().eq_ignore_ascii_case(old).then_some(new)
}

#[derive(Clone)]
struct Row {
    action: &'static str,
    button: Button,
    status: Label,
}

#[derive(Clone)]
struct View {
    root: glib::WeakRef<GtkBox>,
    title: Label,
    detail: Label,
    defaults: Button,
    rows: Vec<Row>,
}

fn short_reason(reason: &str) -> &str {
    if reason == "Already in use or reserved by Windows" {
        "Already in use or reserved"
    } else {
        reason
    }
}

impl View {
    fn refresh(&self) -> glib::ControlFlow {
        let Some(root) = self.root.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let status = super::actions::shell_settings_path()
            .and_then(|path| {
                path.parent()
                    .map(|parent| parent.join("hotkeys-status.json"))
            })
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let state = parse_status(status.as_ref(), now);
        self.apply(&root, state);
        glib::ControlFlow::Continue
    }

    fn apply(&self, root: &GtkBox, state: Status) {
        // The listener temporarily releases every key while the editor is open.
        if state == Status::Paused {
            return;
        }

        let issues = match &state {
            Status::Active(issues) => issues.as_slice(),
            _ => &[],
        };
        let mut visible_issues = 0;
        for row in &self.rows {
            let binding = row.button.label().unwrap_or_default();
            let issue = issues.iter().find(|issue| {
                issue.action == row.action && issue.binding.eq_ignore_ascii_case(binding.trim())
            });
            if let Some(issue) = issue {
                visible_issues += 1;
                row.status.set_text(short_reason(&issue.reason));
                row.status.set_tooltip_text(Some(&issue.reason));
                row.status.set_visible(true);
                row.button.add_css_class("shortcut-conflict");
                row.button.set_tooltip_text(Some(&issue.reason));
            } else {
                row.status.set_visible(false);
                row.button.remove_css_class("shortcut-conflict");
                row.button.set_tooltip_text(None);
            }
        }

        match state {
            Status::Offline => {
                self.title.set_text("Shortcuts are offline");
                self.detail
                    .set_text("Start the ApexShot capture app to enable them.");
                self.defaults.set_visible(false);
                root.set_visible(true);
            }
            Status::Unavailable(error) => {
                self.title.set_text("Shortcuts are unavailable");
                self.detail.set_text(&error);
                self.defaults.set_visible(false);
                root.set_visible(true);
            }
            Status::Active(_) if visible_issues > 0 => {
                let title = if visible_issues == 1 {
                    "1 shortcut needs attention".to_string()
                } else {
                    format!("{visible_issues} shortcuts need attention")
                };
                self.title.set_text(&title);
                self.detail
                    .set_text("Choose another shortcut in the highlighted rows.");
                self.defaults.set_visible(self.rows.iter().any(|row| {
                    let old = match row.action {
                        "quick_capture" => "Shift+Super+5",
                        "capture_area" => "Shift+Super+4",
                        "capture_fullscreen" => "Shift+Super+3",
                        _ => return false,
                    };
                    row.status.is_visible()
                        && row
                            .button
                            .label()
                            .is_some_and(|label| label.trim().eq_ignore_ascii_case(old))
                }));
                root.set_visible(true);
            }
            Status::Active(_) | Status::Paused => root.set_visible(false),
        }
    }
}

pub fn install(widgets: &ShortcutSettingsWidgets) {
    install_css();
    let root = GtkBox::new(Orientation::Horizontal, 12);
    root.add_css_class("shortcut-notice");
    root.set_hexpand(true);
    root.set_visible(false);

    let icon = Image::from_icon_name("dialog-warning-symbolic");
    icon.add_css_class("shortcut-notice-icon");
    icon.set_pixel_size(18);
    icon.set_valign(Align::Start);
    root.append(&icon);

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = Label::new(None);
    title.add_css_class("shortcut-notice-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    text.append(&title);
    let detail = Label::new(None);
    detail.add_css_class("shortcut-notice-detail");
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    text.append(&detail);
    root.append(&text);

    let defaults = Button::with_label("Use Windows defaults");
    defaults.add_css_class("shortcut-notice-action");
    defaults.set_valign(Align::Center);
    defaults.set_tooltip_text(Some(
        "Replace only the old Win+Shift capture defaults; other shortcuts stay unchanged.",
    ));
    let choices = [
        (
            widgets.quick_capture_btn.clone(),
            "Shift+Super+5",
            "Ctrl+Alt+Shift+M",
        ),
        (
            widgets.capture_area_btn.clone(),
            "Shift+Super+4",
            "Ctrl+Alt+Shift+A",
        ),
        (
            widgets.capture_fullscreen_btn.clone(),
            "Shift+Super+3",
            "Ctrl+Alt+Shift+F",
        ),
    ];
    defaults.connect_clicked(move |_| {
        for (shortcut, old, new) in &choices {
            if legacy_replacement(shortcut.label().as_deref().unwrap_or_default(), old, new)
                .is_some()
            {
                shortcut.set_label(new);
            }
        }
    });
    root.append(&defaults);

    let rows = [
        ("quick_capture", &widgets.quick_capture_btn),
        ("capture_area", &widgets.capture_area_btn),
        ("capture_crosshair", &widgets.capture_crosshair_btn),
        ("capture_previous_area", &widgets.capture_prev_btn),
        ("capture_fullscreen", &widgets.capture_fullscreen_btn),
        ("show_last_preview", &widgets.show_last_preview_btn),
        ("save_last_capture", &widgets.save_last_capture_btn),
        ("record_screen", &widgets.record_screen_btn),
        ("recording_stop_save", &widgets.recording_stop_save_btn),
    ]
    .into_iter()
    .filter_map(|(action, button)| {
        Some(Row {
            action,
            button: button.clone(),
            status: widgets.status_labels.get(action)?.clone(),
        })
    })
    .collect();
    widgets
        .section
        .insert_child_after(&root, widgets.section.first_child().as_ref());
    let view = View {
        root: root.downgrade(),
        title,
        detail,
        defaults,
        rows,
    };
    view.refresh();
    glib::timeout_add_local(Duration::from_secs(1), move || view.refresh());
}

fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("shortcuts_status.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::{legacy_replacement, parse_status, Issue, Row, Status, View};
    use gtk4::{prelude::*, Box as GtkBox, Button, Label, Orientation};
    use serde_json::json;

    #[test]
    fn reports_missing_stale_and_conflicting_registrations() {
        assert_eq!(parse_status(None, 20_000), Status::Offline);
        assert_eq!(
            parse_status(Some(&json!({"timestamp_ms": 1})), 20_000),
            Status::Offline
        );
        let status = json!({
            "timestamp_ms": 20_000,
            "errors": [{"action": "capture_area", "binding": "Ctrl+X", "reason": "Already in use"}],
        });
        assert_eq!(
            parse_status(Some(&status), 20_000),
            Status::Active(vec![Issue {
                action: "capture_area".into(),
                binding: "Ctrl+X".into(),
                reason: "Already in use".into(),
            }])
        );
        assert_eq!(
            parse_status(Some(&json!({"timestamp_ms": 20_000, "errors": []})), 20_000),
            Status::Active(vec![])
        );
    }

    #[test]
    fn windows_defaults_preserve_custom_and_disabled_bindings() {
        assert_eq!(
            legacy_replacement("Shift+Super+4", "Shift+Super+4", "Ctrl+Alt+Shift+A"),
            Some("Ctrl+Alt+Shift+A")
        );
        assert_eq!(
            legacy_replacement("Ctrl+Alt+A", "Shift+Super+4", "Ctrl+Alt+Shift+A"),
            None
        );
        assert_eq!(
            legacy_replacement("", "Shift+Super+4", "Ctrl+Alt+Shift+A"),
            None
        );
    }

    #[test]
    fn conflicts_appear_on_the_matching_row_and_clear_after_editing() {
        let Some(result) = crate::test_support::with_gtk(|| {
            let root = GtkBox::new(Orientation::Horizontal, 0);
            let title = Label::new(None);
            let detail = Label::new(None);
            let defaults = Button::new();
            let button = Button::with_label("Shift+Super+4");
            let row_status = Label::new(None);
            let view = View {
                root: root.downgrade(),
                title: title.clone(),
                detail,
                defaults: defaults.clone(),
                rows: vec![Row {
                    action: "capture_area",
                    button: button.clone(),
                    status: row_status.clone(),
                }],
            };
            let issue = Issue {
                action: "capture_area".into(),
                binding: "Shift+Super+4".into(),
                reason: "Already in use or reserved by Windows".into(),
            };
            view.apply(&root, Status::Active(vec![issue.clone()]));
            let conflict = (
                root.is_visible(),
                title.text().to_string(),
                row_status.text().to_string(),
                button.has_css_class("shortcut-conflict"),
                defaults.is_visible(),
            );
            button.set_label("Ctrl+Alt+Shift+A");
            view.apply(&root, Status::Active(vec![issue]));
            (
                conflict,
                row_status.is_visible(),
                button.has_css_class("shortcut-conflict"),
                root.is_visible(),
            )
        }) else {
            return;
        };
        let (conflict, status_visible, highlighted, notice_visible) = result;
        assert_eq!(
            conflict,
            (
                true,
                "1 shortcut needs attention".into(),
                "Already in use or reserved".into(),
                true,
                true,
            )
        );
        assert!(!status_visible && !highlighted && !notice_visible);
    }

    #[test]
    fn status_stylesheet_parses() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let found = errors.clone();
            provider.connect_parsing_error(move |_, _, error| {
                found.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(include_str!("shortcuts_status.css"));
            let result = errors.borrow().clone();
            result
        }) else {
            return;
        };
        assert!(errors.is_empty(), "CSS parsing errors: {errors:?}");
    }
}

//! The upgrade card a cloud export shows when the account is out of space.

use std::cell::RefCell;
use std::rc::Rc;

use crate::i18n::{t, tfmt};
use crate::studio_upsell::{
    centered_label, close_button, install_css, link_button, primary_button, px, theme_is_dark,
    LIGHT_CLASS,
};
use gtk4::{
    gdk, glib, prelude::*, Align, Application, Box as GtkBox, EventControllerKey, Orientation,
    Overlay, PropagationPhase, Window,
};

use super::upload::{CloudLimit, CloudLimitKind};

pub const BILLING_URL: &str = "https://apexshot.org/dashboard/settings/billing";
pub const DASHBOARD_URL: &str = "https://apexshot.org/dashboard";

/// The card's headline for `limit`.
pub fn headline(limit: &CloudLimit) -> String {
    match limit.kind {
        CloudLimitKind::MonthlyUploads => t("You've used this month's free uploads"),
        CloudLimitKind::Storage => t("Your free Cloud storage is full"),
        CloudLimitKind::Other => t("Your capture is saved"),
    }
}

/// The card's headline once an upload is close to a limit but still works.
pub fn headline_almost(kind: CloudLimitKind) -> String {
    match kind {
        CloudLimitKind::MonthlyUploads => t("Almost out of free uploads"),
        CloudLimitKind::Storage => t("Your free Cloud storage is almost full"),
        CloudLimitKind::Other => t("Your capture is saved"),
    }
}

/// The lede under the full-limit headline.
pub fn lede(limit: &CloudLimit) -> String {
    match limit.kind {
        CloudLimitKind::MonthlyUploads => {
            let mut sentences = Vec::new();
            if let (Some(used), Some(max)) = (limit.used, limit.max) {
                sentences.push(tfmt(
                    "{used} of {max} free uploads used.",
                    &[("used", &used.to_string()), ("max", &max.to_string())],
                ));
            }
            sentences.push(
                match limit.resets_at.as_deref().and_then(format_reset_date) {
                    Some(date) => tfmt("Uploads come back on {date}.", &[("date", &date)]),
                    None => t("Uploads come back next month."),
                },
            );
            sentences.push(t("Go Pro for unlimited uploads."));
            sentences.join(" ")
        }
        CloudLimitKind::Storage => {
            let mut sentences = Vec::new();
            if let (Some(used), Some(max)) = (limit.used, limit.max) {
                sentences.push(tfmt(
                    "{used} of {max} used.",
                    &[
                        ("used", &format_storage_size(used)),
                        ("max", &format_storage_size(max)),
                    ],
                ));
            }
            sentences.push(t("Delete old uploads, or Go Pro for 50 GB."));
            sentences.join(" ")
        }
        CloudLimitKind::Other => {
            let message = limit.message.trim();
            if message.is_empty() {
                t("Go Pro for unlimited uploads.")
            } else {
                message.to_string()
            }
        }
    }
}

/// The lede under the near-limit headline.
pub fn lede_almost(limit: &CloudLimit) -> String {
    match limit.kind {
        CloudLimitKind::MonthlyUploads => {
            let mut sentences = Vec::new();
            if let (Some(used), Some(max)) = (limit.used, limit.max) {
                sentences.push(tfmt(
                    "{used} of {max} used this month.",
                    &[("used", &used.to_string()), ("max", &max.to_string())],
                ));
            }
            sentences.push(t("Go Pro for unlimited uploads."));
            sentences.join(" ")
        }
        CloudLimitKind::Storage => {
            let mut sentences = Vec::new();
            if let (Some(used), Some(max)) = (limit.used, limit.max) {
                sentences.push(tfmt(
                    "{used} of {max} used.",
                    &[
                        ("used", &format_storage_size(used)),
                        ("max", &format_storage_size(max)),
                    ],
                ));
            }
            sentences.push(t("Go Pro for 50 GB."));
            sentences.join(" ")
        }
        CloudLimitKind::Other => lede(limit),
    }
}

/// The reassurance line, omitted when the headline already says it.
pub fn saved_note(limit: &CloudLimit) -> Option<String> {
    (limit.kind != CloudLimitKind::Other).then(|| t("Your capture is saved on this computer."))
}

/// The headline and lede the card shows.
pub fn copy_for(limit: &CloudLimit, almost: bool) -> (String, String, Option<String>) {
    let headline = if almost {
        headline_almost(limit.kind)
    } else {
        headline(limit)
    };
    let lede = if almost {
        lede_almost(limit)
    } else {
        lede(limit)
    };
    let note = saved_note(limit);
    (headline, lede, note)
}

/// Format an RFC 3339 reset timestamp as a local "December 1".
pub fn format_reset_date(raw: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    Some(format_date_in(parsed.with_timezone(&chrono::Local)))
}

fn format_date_in<Tz: chrono::TimeZone>(date: chrono::DateTime<Tz>) -> String {
    use chrono::Datelike;

    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{} {}", MONTHS[date.month0() as usize], date.day())
}

/// Compact storage size for the card's lede.
pub fn format_storage_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f < KB {
        format!("{bytes} B")
    } else if bytes_f < MB {
        format!("{:.0} KB", bytes_f / KB)
    } else if bytes_f < GB {
        format!("{:.1} MB", bytes_f / MB)
    } else {
        // Whole gigabytes read as "2 GB"; otherwise one decimal, "1.8 GB".
        let gb = (bytes_f / GB * 10.0).round() / 10.0;
        if gb.fract() == 0.0 {
            format!("{gb:.0} GB")
        } else {
            format!("{gb:.1} GB")
        }
    }
}

/// Parse the `cloud-limit` command line shared by both platforms.
///
/// The first argument is the kind; `--almost` selects the near-limit copy.
pub fn parse_cli_limit(args: &[String]) -> Result<(CloudLimit, bool), String> {
    let Some(kind_arg) = args.first() else {
        return Err("cloud-limit needs a kind: monthly_uploads, storage, or other".to_string());
    };
    let kind = CloudLimitKind::from_arg(kind_arg).ok_or_else(|| {
        format!("unknown cloud limit kind '{kind_arg}': use monthly_uploads, storage, or other")
    })?;

    let mut used = None;
    let mut max = None;
    let mut resets_at = None;
    let mut message = String::new();
    let mut almost = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--used" => {
                index += 1;
                used = Some(parse_u64(args.get(index), "--used")?);
            }
            "--max" => {
                index += 1;
                max = Some(parse_u64(args.get(index), "--max")?);
            }
            "--resets-at" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--resets-at needs an RFC 3339 timestamp".to_string())?;
                resets_at = Some(value.clone());
            }
            "--message" => {
                index += 1;
                message = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--message needs text".to_string())?;
            }
            "--almost" => almost = true,
            other => return Err(format!("unknown cloud-limit option '{other}'")),
        }
        index += 1;
    }

    Ok((
        CloudLimit {
            kind,
            message,
            used,
            max,
            resets_at,
        },
        almost,
    ))
}

fn parse_u64(value: Option<&String>, flag: &str) -> Result<u64, String> {
    value
        .ok_or_else(|| format!("{flag} needs a number"))?
        .parse()
        .map_err(|_| format!("{flag} needs a number"))
}

/// Show the full-limit card over `window`.
pub fn show_prompt(window: &impl IsA<Window>, limit: &CloudLimit) {
    show_prompt_with(window, limit, false);
}

/// Show the near-limit card over `window`.
pub fn show_prompt_almost(window: &impl IsA<Window>, limit: &CloudLimit) {
    show_prompt_with(window, limit, true);
}

fn show_prompt_with(window: &impl IsA<Window>, limit: &CloudLimit, almost: bool) {
    install_css();
    let parent: &Window = window.upcast_ref();
    let dark = theme_is_dark(parent);

    let modal = Window::builder()
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .resizable(false)
        .default_width(px(480.0))
        .build();
    modal.set_destroy_with_parent(true);
    build_card(&modal, dark, limit, almost);
    modal.present();
}

/// Show the card in its own window, with no parent.
pub fn show_prompt_standalone(app: &Application, limit: &CloudLimit) {
    show_prompt_standalone_with(app, limit, false);
}

/// Show the near-limit card in its own window, with no parent.
pub fn show_prompt_standalone_almost(app: &Application, limit: &CloudLimit) {
    show_prompt_standalone_with(app, limit, true);
}

fn show_prompt_standalone_with(app: &Application, limit: &CloudLimit, almost: bool) {
    install_css();
    let window = Window::builder()
        .application(app)
        .decorated(false)
        .resizable(false)
        .default_width(px(480.0))
        .title(crate::app_identity::app_name())
        .build();
    let dark = theme_is_dark(&window);
    build_card(&window, dark, limit, almost);
    window.present();
}

/// Run the standalone card as this process's whole GTK application.
pub fn show_standalone(limit: CloudLimit, almost: bool) -> Result<(), String> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let pending = Rc::new(RefCell::new(Some((limit, almost))));
    app.connect_activate(move |app| {
        let mut pending = pending.borrow_mut();
        let Some((limit, almost)) = pending.take() else {
            return;
        };
        if almost {
            show_prompt_standalone_almost(app, &limit);
        } else {
            show_prompt_standalone(app, &limit);
        }
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_card(window: &Window, dark: bool, limit: &CloudLimit, almost: bool) {
    window.add_css_class("apexshot-studio-prompt");

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-card");
    if !dark {
        card.add_css_class(LIGHT_CLASS);
    }
    card.set_size_request(px(480.0), -1);
    card.set_overflow(gtk4::Overflow::Hidden);

    let hero = Overlay::new();
    hero.set_child(Some(&crate::dither_field::dither_field(dark, 150.0)));

    let (headline_text, lede_text, note_text) = copy_for(limit, almost);

    let words = GtkBox::new(Orientation::Vertical, px(8.0));
    words.set_valign(Align::End);
    words.set_margin_start(px(28.0));
    words.set_margin_end(px(28.0));
    words.set_margin_bottom(px(6.0));
    let headline = centered_label(&headline_text, "apexshot-studio-headline");
    headline.add_css_class("apexshot-studio-headline-compact");
    headline.set_max_width_chars(26);
    let lede = centered_label(&lede_text, "apexshot-studio-lede");
    lede.set_max_width_chars(52);
    words.append(&headline);
    words.append(&lede);
    if let Some(note) = note_text {
        let note = centered_label(&note, "apexshot-studio-note");
        note.set_max_width_chars(52);
        words.append(&note);
    }
    hero.add_overlay(&words);

    let close = close_button();
    close.set_halign(Align::End);
    close.set_valign(Align::Start);
    close.set_margin_top(px(12.0));
    close.set_margin_end(px(12.0));
    close.connect_clicked({
        let window = window.downgrade();
        move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        }
    });
    hero.add_overlay(&close);
    card.append(&hero);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("apexshot-studio-body");

    let go_pro = primary_button(&t("Go Pro"));
    go_pro.add_css_class("apexshot-studio-cta");
    go_pro.set_halign(Align::Center);
    go_pro.set_size_request(px(320.0), -1);
    go_pro.connect_clicked({
        let window = window.downgrade();
        move |_| {
            open_in_background(BILLING_URL, t("Could not open billing"));
            if let Some(window) = window.upgrade() {
                window.close();
            }
        }
    });
    body.append(&go_pro);

    let links = GtkBox::new(Orientation::Horizontal, px(6.0));
    links.set_halign(Align::Center);
    links.set_margin_top(px(8.0));

    if limit.kind == CloudLimitKind::Storage {
        let manage = link_button(&t("Manage uploads"));
        manage.connect_clicked({
            let window = window.downgrade();
            move |_| {
                open_in_background(DASHBOARD_URL, t("Could not open the dashboard"));
                if let Some(window) = window.upgrade() {
                    window.close();
                }
            }
        });
        links.append(&manage);
    }

    let keep = link_button(&t("Keep working"));
    keep.connect_clicked({
        let window = window.downgrade();
        move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        }
    });
    links.append(&keep);
    body.append(&links);
    card.append(&body);

    // The card is undecorated, so the whole card is the drag handle; its
    // buttons and links still take their own clicks.
    let handle = gtk4::WindowHandle::new();
    handle.set_child(Some(&card));
    window.set_child(Some(&handle));

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(PropagationPhase::Capture);
    keys.connect_key_pressed({
        let window = window.downgrade();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}

fn open_in_background(url: &'static str, failure_title: String) {
    std::thread::spawn(move || {
        if let Err(error) = crate::utils::open::open_url(url) {
            crate::utils::notify::desktop_notification(&failure_title, &error);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

    fn limit(kind: CloudLimitKind) -> CloudLimit {
        CloudLimit {
            kind,
            message: String::new(),
            used: None,
            max: None,
            resets_at: None,
        }
    }

    #[test]
    fn monthly_with_numbers_and_a_reset_date_names_the_date() {
        let mut limit = limit(CloudLimitKind::MonthlyUploads);
        limit.used = Some(20);
        limit.max = Some(20);
        limit.resets_at = Some("2026-12-01T00:00:00Z".to_string());

        assert_eq!(headline(&limit), "You've used this month's free uploads");
        let copy = lede(&limit);
        assert!(copy.contains("20 of 20 free uploads used."));
        let local_date =
            format_reset_date("2026-12-01T00:00:00Z").expect("a formattable reset date");
        assert!(copy.contains(&local_date));
        assert!(copy.contains("Go Pro for unlimited uploads."));
    }

    #[test]
    fn monthly_without_numbers_or_a_date_still_reads_whole() {
        let limit = limit(CloudLimitKind::MonthlyUploads);
        assert_eq!(
            lede(&limit),
            "Uploads come back next month. Go Pro for unlimited uploads."
        );
    }

    #[test]
    fn monthly_without_a_date_keeps_the_number_sentence() {
        let mut limit = limit(CloudLimitKind::MonthlyUploads);
        limit.used = Some(12);
        limit.max = Some(20);
        assert_eq!(
            lede(&limit),
            "12 of 20 free uploads used. Uploads come back next month. Go Pro for unlimited uploads."
        );
    }

    #[test]
    fn storage_copy_reports_human_sizes() {
        let mut full = limit(CloudLimitKind::Storage);
        full.used = Some(2 * 1024 * 1024 * 1024);
        full.max = Some(2 * 1024 * 1024 * 1024);
        assert_eq!(headline(&full), "Your free Cloud storage is full");
        assert_eq!(
            lede(&full),
            "2 GB of 2 GB used. Delete old uploads, or Go Pro for 50 GB."
        );

        let bare = limit(CloudLimitKind::Storage);
        assert_eq!(lede(&bare), "Delete old uploads, or Go Pro for 50 GB.");
    }

    #[test]
    fn other_copy_uses_the_server_message_and_omits_the_saved_note() {
        let mut limit = limit(CloudLimitKind::Other);
        limit.message = "This account cannot upload more.".to_string();
        assert_eq!(headline(&limit), "Your capture is saved");
        assert_eq!(lede(&limit), "This account cannot upload more.");
        assert_eq!(saved_note(&limit), None);
    }

    #[test]
    fn the_saved_note_reassures_for_limit_kinds() {
        assert_eq!(
            saved_note(&limit(CloudLimitKind::MonthlyUploads)).as_deref(),
            Some("Your capture is saved on this computer.")
        );
        assert_eq!(
            saved_note(&limit(CloudLimitKind::Storage)).as_deref(),
            Some("Your capture is saved on this computer.")
        );
    }

    #[test]
    fn almost_copy_stays_short() {
        let mut monthly = limit(CloudLimitKind::MonthlyUploads);
        monthly.used = Some(19);
        monthly.max = Some(20);
        assert_eq!(headline_almost(monthly.kind), "Almost out of free uploads");
        assert_eq!(
            lede_almost(&monthly),
            "19 of 20 used this month. Go Pro for unlimited uploads."
        );

        let mut storage = limit(CloudLimitKind::Storage);
        storage.used = Some(2 * 1024 * 1024 * 1024);
        storage.max = Some(2 * 1024 * 1024 * 1024);
        assert_eq!(
            headline_almost(storage.kind),
            "Your free Cloud storage is almost full"
        );
        assert_eq!(
            lede_almost(&storage),
            "2 GB of 2 GB used. Go Pro for 50 GB."
        );
    }

    #[test]
    fn a_reset_across_midnight_in_local_time_moves_to_the_new_month() {
        let utc = Utc
            .with_ymd_and_hms(2026, 12, 31, 23, 30, 0)
            .single()
            .expect("valid date");
        let nairobi = FixedOffset::east_opt(3 * 3600).expect("valid offset");
        assert_eq!(format_date_in(utc.with_timezone(&nairobi)), "January 1");
    }

    #[test]
    fn sizes_use_the_editors_compact_scale() {
        assert_eq!(format_storage_size(0), "0 B");
        assert_eq!(format_storage_size(2048), "2 KB");
        assert_eq!(format_storage_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_storage_size(3 * 1024 * 1024 * 1024), "3 GB");
        assert_eq!(format_storage_size(1932735283), "1.8 GB");
    }

    #[test]
    fn the_command_line_round_trips_every_field() {
        let args = vec![
            "monthly_uploads".to_string(),
            "--used".to_string(),
            "20".to_string(),
            "--max".to_string(),
            "20".to_string(),
            "--resets-at".to_string(),
            "2026-12-01T00:00:00Z".to_string(),
            "--message".to_string(),
            "Monthly upload limit reached".to_string(),
            "--almost".to_string(),
        ];

        let (limit, almost) = parse_cli_limit(&args).expect("valid arguments");
        assert!(almost);
        assert_eq!(limit.kind, CloudLimitKind::MonthlyUploads);
        assert_eq!(limit.used, Some(20));
        assert_eq!(limit.max, Some(20));
        assert_eq!(limit.resets_at.as_deref(), Some("2026-12-01T00:00:00Z"));
        assert_eq!(limit.message, "Monthly upload limit reached");
    }

    #[test]
    fn the_command_line_rejects_an_unknown_kind() {
        assert!(parse_cli_limit(&["weekly".to_string()]).is_err());
        assert!(parse_cli_limit(&[]).is_err());
    }
}

//! The four setup pages and the final checklist.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, CheckButton, Label, Orientation, Overlay, Switch};

use super::hero::{self, HeroScene};
use super::widgets::{
    brand, email_pill, heading, icon, label, primary_button, px, secondary_button, settings_row,
    spacer, vbox, wrapped,
};
use super::{report_error, status_line, Shell, Step};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;

const STUDIO_URL: &str = "https://apexshot.org/studio";

/// A centered page: heading, a body slot, and the footer along the bottom.
fn centered_page(shell: &Shell, title: &str, subtitle: &str) -> (GtkBox, GtkBox) {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let header = heading(shell, title, subtitle);
    header.add_css_class("ob-centered");
    header.set_margin_start(px(shell.scale(), 48.0));
    header.set_margin_end(px(shell.scale(), 48.0));
    page.append(&header);

    let body = vbox(0);
    body.set_halign(Align::Center);
    body.set_valign(Align::Start);
    body.set_margin_top(px(shell.scale(), 30.0));
    page.append(&body);

    page.append(&spacer());
    (page, body)
}

pub(crate) fn build_appearance(shell: &Shell) -> GtkBox {
    let (page, body) = centered_page(
        shell,
        "Make it yours",
        "ApexShot follows Windows by default. Pick the look you want to live in.",
    );
    let selected = shell.config().ui_theme;
    let row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 16.0));
    row.set_halign(Align::Center);

    let cards: Rc<RefCell<Vec<(String, Button, Label)>>> = Rc::new(RefCell::new(Vec::new()));
    for (id, title, caption) in [
        ("system", "System", "Match Windows"),
        ("dark", "Dark", "Easy on the eyes"),
        ("light", "Light", "Crisp and bright"),
    ] {
        let (card, badge) = theme_card(shell, id, title, caption);
        if selected == id {
            card.add_css_class("ob-choice-card-selected");
            badge.set_opacity(1.0);
        } else {
            badge.set_opacity(0.0);
        }

        {
            let shell = shell.clone();
            let cards = Rc::clone(&cards);
            let id = id.to_string();
            card.connect_clicked(move |_| {
                shell.update_config(|config| config.ui_theme = id.clone());
                shell.apply_theme();
                for (card_id, card, badge) in cards.borrow().iter() {
                    let active = *card_id == id;
                    if active {
                        card.add_css_class("ob-choice-card-selected");
                        badge.set_opacity(1.0);
                    } else {
                        card.remove_css_class("ob-choice-card-selected");
                        badge.set_opacity(0.0);
                    }
                }
            });
        }

        cards
            .borrow_mut()
            .push((id.to_string(), card.clone(), badge));
        row.append(&card);
    }
    body.append(&row);

    let next = {
        let shell = shell.clone();
        Rc::new(move || shell.advance())
    };
    let footer = super::widgets::footer(shell, Step::Appearance, "Next", next, true);
    page.append(&footer);
    page
}

fn theme_card(shell: &Shell, id: &'static str, title: &str, caption: &str) -> (Button, Label) {
    let card = Button::new();
    card.add_css_class("ob-choice-card");
    card.set_size_request(px(shell.scale(), 184.0), -1);

    let content = vbox(px(shell.scale(), 12.0));
    content.set_halign(Align::Fill);

    let preview = hero::theme_preview(id);
    preview.set_size_request(px(shell.scale(), 152.0), px(shell.scale(), 94.0));
    let preview_overlay = Overlay::new();
    preview_overlay.set_child(Some(&preview));
    let badge = Label::new(Some("✓"));
    badge.add_css_class("ob-check-badge");
    badge.set_halign(Align::End);
    badge.set_valign(Align::Start);
    badge.set_margin_top(px(shell.scale(), 7.0));
    badge.set_margin_end(px(shell.scale(), 7.0));
    preview_overlay.add_overlay(&badge);
    content.append(&preview_overlay);

    let title = label(title, "ob-choice-title");
    title.set_margin_top(px(shell.scale(), 2.0));
    content.append(&title);
    content.append(&wrapped(caption, "ob-choice-caption", 24));

    card.set_child(Some(&content));
    (card, badge)
}

pub(crate) fn build_formats(shell: &Shell) -> GtkBox {
    let (page, body) = centered_page(
        shell,
        "Every shot, ready to paste",
        "Pick what ApexShot saves, then decide what happens the moment a capture lands.",
    );
    let config = shell.config();
    let selected = config.screenshot_format.to_ascii_uppercase();

    let row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 16.0));
    row.set_halign(Align::Center);
    let cards: Rc<RefCell<Vec<(String, Button, Label)>>> = Rc::new(RefCell::new(Vec::new()));

    for (id, class, title, caption) in [
        ("PNG", "ob-format-png", "PNG", "Lossless · sharp UI text"),
        (
            "JPEG",
            "ob-format-jpeg",
            "JPEG",
            "Compact · best for photos",
        ),
        ("WebP", "ob-format-webp", "WebP", "Modern · small and sharp"),
    ] {
        let card = Button::new();
        card.add_css_class("ob-choice-card");
        card.set_size_request(px(shell.scale(), 178.0), -1);
        let content = vbox(px(shell.scale(), 12.0));
        content.set_halign(Align::Fill);

        let preview = GtkBox::new(Orientation::Vertical, 0);
        preview.add_css_class("ob-format-preview");
        preview.add_css_class(class);
        preview.set_size_request(-1, px(shell.scale(), 82.0));
        let tag = Label::new(Some(title));
        tag.add_css_class("ob-format-tag");
        tag.set_halign(Align::Start);
        tag.set_valign(Align::End);
        tag.set_margin_start(px(shell.scale(), 10.0));
        tag.set_margin_bottom(px(shell.scale(), 8.0));
        tag.set_hexpand(true);
        preview.append(&tag);
        content.append(&preview);

        let caption_label = wrapped(caption, "ob-choice-caption", 26);
        caption_label.set_margin_top(px(shell.scale(), 2.0));
        content.append(&caption_label);

        let badge = Label::new(Some("✓"));
        badge.add_css_class("ob-check-badge");
        badge.set_halign(Align::End);
        badge.set_valign(Align::Start);
        badge.set_margin_top(px(shell.scale(), 7.0));
        badge.set_margin_end(px(shell.scale(), 7.0));

        let overlay = Overlay::new();
        overlay.set_child(Some(&content));
        overlay.add_overlay(&badge);
        card.set_child(Some(&overlay));

        if selected == id {
            card.add_css_class("ob-choice-card-selected");
            badge.set_opacity(1.0);
        } else {
            badge.set_opacity(0.0);
        }

        {
            let shell = shell.clone();
            let cards = Rc::clone(&cards);
            let id = id.to_string();
            card.connect_clicked(move |_| {
                shell.update_config(|config| config.screenshot_format = id.clone());
                for (card_id, card, badge) in cards.borrow().iter() {
                    let active = *card_id == id;
                    if active {
                        card.add_css_class("ob-choice-card-selected");
                        badge.set_opacity(1.0);
                    } else {
                        card.remove_css_class("ob-choice-card-selected");
                        badge.set_opacity(0.0);
                    }
                }
            });
        }

        cards
            .borrow_mut()
            .push((id.to_string(), card.clone(), badge));
        row.append(&card);
    }
    body.append(&row);

    let settings = vbox(0);
    settings.add_css_class("ob-settings-card");
    settings.set_size_request(px(shell.scale(), 600.0), -1);
    settings.set_margin_top(px(shell.scale(), 22.0));

    let clipboard = Switch::new();
    clipboard.add_css_class("ob-switch");
    clipboard.set_active(config.after_capture_copy_file_to_clipboard);
    {
        let shell = shell.clone();
        clipboard.connect_state_set(move |_, state| {
            shell.update_config(|config| config.after_capture_copy_file_to_clipboard = state);
            glib::Propagation::Proceed
        });
    }
    settings.append(&settings_row(
        shell,
        icon_names::custom::COPY_SYMBOLIC,
        "Copy to clipboard after capture",
        Some("The file is ready to paste as soon as the shot is taken."),
        &clipboard,
    ));

    let separator = GtkBox::new(Orientation::Horizontal, 0);
    separator.add_css_class("ob-separator");
    settings.append(&separator);

    let quick_access = Switch::new();
    quick_access.add_css_class("ob-switch");
    quick_access.set_active(config.after_capture_show_quick_access);
    {
        let shell = shell.clone();
        quick_access.connect_state_set(move |_, state| {
            shell.update_config(|config| config.after_capture_show_quick_access = state);
            glib::Propagation::Proceed
        });
    }
    settings.append(&settings_row(
        shell,
        icon_names::custom::IMAGE_ALT_SYMBOLIC,
        "Show Quick Access after capture",
        Some("A small card with copy, save and open, right where you are working."),
        &quick_access,
    ));
    body.append(&settings);

    let next = {
        let shell = shell.clone();
        Rc::new(move || shell.advance())
    };
    let footer = super::widgets::footer(shell, Step::Formats, "Next", next, true);
    page.append(&footer);
    page
}

pub(crate) fn build_recording(shell: &Shell) -> GtkBox {
    let page = GtkBox::new(Orientation::Horizontal, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let left = GtkBox::new(Orientation::Vertical, 0);
    left.set_size_request(px(shell.scale(), 500.0), -1);
    left.set_hexpand(false);
    page.append(&left);

    let content = vbox(0);
    content.add_css_class("ob-split-left");
    content.set_vexpand(true);
    left.append(&content);

    content.append(&wrapped(
        "Recordings that feel directed.",
        "ob-headline",
        20,
    ));
    let copy = wrapped(
        "ApexShot keeps the screen, your face and your voice apart, so the editor can compose them the way you want. Pick a starting point; every recording can change it later.",
        "ob-lead",
        46,
    );
    copy.set_margin_top(px(shell.scale(), 16.0));
    content.append(&copy);

    let config = shell.config();
    let scene = Rc::new(RefCell::new(HeroScene::for_config(&config)));
    let preview = hero::build(Rc::clone(&scene));
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.set_margin_top(px(shell.scale(), 10.0));
    preview.set_margin_bottom(px(shell.scale(), 10.0));
    preview.set_margin_end(px(shell.scale(), 10.0));
    preview.set_margin_start(px(shell.scale(), 6.0));
    page.append(&preview);

    let options = vbox(0);
    options.add_css_class("ob-settings-card");
    options.set_margin_top(px(shell.scale(), 24.0));
    options.set_halign(Align::Fill);

    let initial = match *scene.borrow() {
        HeroScene::CameraAndVoice => 0,
        HeroScene::ScreenAndVoice => 1,
        HeroScene::ScreenOnly => 2,
    };
    let mut group: Option<CheckButton> = None;
    for (index, (scene_value, title, caption, icon_name)) in [
        (
            HeroScene::CameraAndVoice,
            "Camera and voice",
            "Your face in a soft bubble, your voice on its own track.",
            icon_names::custom::WEBCAM_SYMBOLIC,
        ),
        (
            HeroScene::ScreenAndVoice,
            "Screen and voice",
            "No camera bubble. Narration is still recorded.",
            icon_names::custom::RECORD_SCREEN_SYMBOLIC,
        ),
        (
            HeroScene::ScreenOnly,
            "Screen only",
            "Silent pixels. Add the camera and voice later in the editor.",
            icon_names::custom::SCREENSHOOTER_SYMBOLIC,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let radio = CheckButton::new();
        radio.add_css_class("ob-radio-button");
        if let Some(first) = &group {
            radio.set_group(Some(first));
        } else {
            group = Some(radio.clone());
        }
        radio.set_active(index == initial);

        {
            let shell = shell.clone();
            let scene = Rc::clone(&scene);
            let preview = preview.clone();
            radio.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                *scene.borrow_mut() = scene_value;
                preview.queue_draw();
                shell.update_config(|config| {
                    config.rec_webcam_enabled = scene_value.camera();
                    config.rec_mic = scene_value.microphone();
                });
            });
        }

        let row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 14.0));
        row.add_css_class("ob-option-row");
        row.set_hexpand(true);
        let badge = GtkBox::new(Orientation::Vertical, 0);
        badge.add_css_class("ob-row-icon");
        badge.set_valign(Align::Center);
        badge.append(&icon(icon_name, px(shell.scale(), 17.0)));
        row.append(&badge);

        let text = vbox(px(shell.scale(), 3.0));
        text.set_hexpand(true);
        text.set_valign(Align::Center);
        let title_label = label(title, "ob-choice-title");
        text.append(&title_label);
        text.append(&wrapped(caption, "ob-choice-caption", 42));
        row.append(&text);
        radio.set_valign(Align::Center);
        row.append(&radio);

        let click = gtk4::GestureClick::new();
        {
            let radio = radio.clone();
            click.connect_released(move |_, _, _, _| radio.set_active(true));
        }
        row.add_controller(click);

        options.append(&row);
        if index < 2 {
            let separator = GtkBox::new(Orientation::Horizontal, 0);
            separator.add_css_class("ob-separator");
            options.append(&separator);
        }
    }
    content.append(&options);

    let footer = super::widgets::footer(
        shell,
        Step::Recording,
        "Next",
        {
            let shell = shell.clone();
            Rc::new(move || shell.advance())
        },
        true,
    );
    left.append(&footer);

    page
}

pub(crate) fn build_plan(shell: &Shell) -> GtkBox {
    let (page, body) = centered_page(
        shell,
        "Start free. Stay free.",
        "ApexShot on Windows is free while the Studio tools are finished for this platform.",
    );

    let row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 18.0));
    row.set_halign(Align::Center);

    let cards: Rc<RefCell<Vec<(&'static str, GtkBox)>>> = Rc::new(RefCell::new(Vec::new()));

    let free = plan_card(
        shell,
        PlanCopy {
            name: "Free",
            price: "$0",
            unit: "/ forever",
            badge: None,
            features: &[
                "Capture, annotate and OCR",
                "Screen and audio recording",
                "Video editor and clean exports",
                "Local files, no watermark",
            ],
            cta: "Continue free",
            studio: false,
        },
    );
    {
        let shell = shell.clone();
        let cards_for_click = Rc::clone(&cards);
        let card_box = free.clone();
        let button = plan_button(&free);
        button.connect_clicked(move |_| {
            highlight_plan(&cards_for_click, "free");
            shell.advance();
        });
        cards.borrow_mut().push(("free", card_box));
    }

    let studio = plan_card(
        shell,
        PlanCopy {
            name: "Studio",
            price: "$39",
            unit: "/ one-time",
            badge: Some("Linux build"),
            features: &[
                "Everything in Free",
                "Motion exports and animated titles",
                "Offline neural upscaling",
                "Advanced zoom camera",
                "12 months of updates",
            ],
            cta: "Read about Studio",
            studio: true,
        },
    );
    {
        let cards_for_click = Rc::clone(&cards);
        let card_box = studio.clone();
        let button = plan_button(&studio);
        button.connect_clicked(move |_| {
            highlight_plan(&cards_for_click, "studio");
            if let Err(error) = crate::utils::open::open_url(STUDIO_URL) {
                eprintln!("[onboarding] could not open the Studio page: {error}");
            }
        });
        cards.borrow_mut().push(("studio", card_box));
    }

    row.append(&free);
    row.append(&studio);
    highlight_plan(&cards, "free");
    body.append(&row);

    let note = wrapped(
        "Studio is a one-time purchase for the official Linux build. It is not available in this Windows build yet, and nothing on this page asks you to pay for it.",
        "ob-fineprint",
        74,
    );
    note.set_halign(Align::Center);
    note.set_xalign(0.5);
    note.set_justify(gtk4::Justification::Center);
    note.set_margin_top(px(shell.scale(), 22.0));
    body.append(&note);

    let next = {
        let shell = shell.clone();
        Rc::new(move || shell.advance())
    };
    let footer = super::widgets::footer(shell, Step::Plan, "Continue", next, false);
    page.append(&footer);
    page
}

/// Everything one plan card says. Kept together so the two cards stay a
/// visual pair instead of two long argument lists.
struct PlanCopy<'a> {
    name: &'a str,
    price: &'a str,
    unit: &'a str,
    badge: Option<&'a str>,
    features: &'a [&'a str],
    cta: &'a str,
    studio: bool,
}

/// Build one plan card and return its root. The CTA is the last button in the
/// card; [`plan_button`] finds it.
fn plan_card(shell: &Shell, copy: PlanCopy) -> GtkBox {
    let card = vbox(0);
    card.add_css_class("ob-plan-card");
    if copy.studio {
        card.add_css_class("ob-plan-card-studio");
    }

    let head = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 8.0));
    let title = label(copy.name, "ob-plan-name");
    head.append(&title);
    if let Some(badge) = copy.badge {
        let pill = Label::new(Some(badge));
        pill.add_css_class("ob-badge");
        pill.set_valign(Align::Center);
        head.append(&pill);
    }
    card.append(&head);

    let price_row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 6.0));
    price_row.add_css_class("ob-plan-price-row");
    price_row.set_valign(Align::End);
    let price = label(copy.price, "ob-plan-price");
    price_row.append(&price);
    let unit = label(copy.unit, "ob-plan-unit");
    unit.set_valign(Align::End);
    unit.set_margin_bottom(px(shell.scale(), 6.0));
    price_row.append(&unit);
    card.append(&price_row);

    let list = vbox(px(shell.scale(), 9.0));
    list.set_margin_top(px(shell.scale(), 18.0));
    for item in copy.features {
        let row = GtkBox::new(Orientation::Horizontal, px(shell.scale(), 9.0));
        let dot = Label::new(Some("✓"));
        dot.add_css_class("ob-check");
        dot.set_valign(Align::Start);
        row.append(&dot);
        let text = Label::new(Some(&t(item)));
        text.add_css_class("ob-plan-feature");
        text.set_xalign(0.0);
        text.set_halign(Align::Start);
        text.set_wrap(true);
        row.append(&text);
        list.append(&row);
    }
    card.append(&list);

    let fill = vbox(0);
    fill.set_vexpand(true);
    card.append(&fill);

    let button = if copy.studio {
        primary_button(shell, copy.cta, false)
    } else {
        secondary_button(shell, copy.cta)
    };
    button.set_halign(Align::Fill);
    button.set_margin_top(px(shell.scale(), 22.0));
    card.append(&button);

    card
}

fn plan_button(card: &GtkBox) -> Button {
    let mut child = card.last_child();
    while let Some(widget) = child {
        if let Some(button) = widget.downcast_ref::<Button>() {
            return button.clone();
        }
        child = widget.prev_sibling();
    }
    // Every plan card ends with its button; this is unreachable in practice.
    Button::new()
}

fn highlight_plan(cards: &Rc<RefCell<Vec<(&'static str, GtkBox)>>>, selected: &str) {
    for (id, card) in cards.borrow().iter() {
        if *id == selected {
            card.add_css_class("ob-plan-card-selected");
        } else {
            card.remove_css_class("ob-plan-card-selected");
        }
    }
}

pub(crate) fn build_finish(shell: &Shell) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);

    let top = vbox(0);
    top.set_halign(Align::Center);
    top.set_valign(Align::Center);
    top.set_vexpand(true);
    top.set_margin_top(px(shell.scale(), 44.0));

    let mark = brand(shell);
    mark.set_halign(Align::Center);
    top.append(&mark);

    let title = wrapped("You're all set!", "ob-title", 40);
    title.set_halign(Align::Center);
    title.set_xalign(0.5);
    title.set_justify(gtk4::Justification::Center);
    title.set_margin_top(px(shell.scale(), 18.0));
    top.append(&title);

    let subtitle = wrapped(
        "ApexShot is ready. Capture from the tray, or open Settings to make it exactly yours.",
        "ob-subtitle",
        60,
    );
    subtitle.set_halign(Align::Center);
    subtitle.set_xalign(0.5);
    subtitle.set_justify(gtk4::Justification::Center);
    subtitle.set_margin_top(px(shell.scale(), 8.0));
    top.append(&subtitle);

    let config = shell.config();
    if !config.cloud_user_email.trim().is_empty() {
        let pill = email_pill(shell, config.cloud_user_email.trim());
        pill.set_halign(Align::Center);
        pill.set_margin_top(px(shell.scale(), 16.0));
        top.append(&pill);
    }

    let card = vbox(0);
    card.add_css_class("ob-settings-card");
    card.set_size_request(px(shell.scale(), 560.0), -1);
    card.set_margin_top(px(shell.scale(), 24.0));

    let open_settings = Rc::new(Cell::new(true));
    let open_switch = Switch::new();
    open_switch.add_css_class("ob-switch");
    open_switch.set_active(true);
    {
        let open_settings = Rc::clone(&open_settings);
        open_switch.connect_state_set(move |_, state| {
            open_settings.set(state);
            glib::Propagation::Proceed
        });
    }
    card.append(&settings_row(
        shell,
        icon_names::custom::SETTINGS_SYMBOLIC,
        "Open Settings when setup finishes",
        Some("Fine-tune shortcuts, recording sources and where files go."),
        &open_switch,
    ));

    let separator = GtkBox::new(Orientation::Horizontal, 0);
    separator.add_css_class("ob-separator");
    card.append(&separator);

    let startup = Switch::new();
    startup.add_css_class("ob-switch");
    startup.set_active(config.start_at_login);
    let startup_hint = status_line();
    startup_hint.set_visible(false);
    {
        let shell = shell.clone();
        let startup_hint = startup_hint.clone();
        startup.connect_state_set(move |_, state| {
            if state {
                match crate::settings::windowing::install_autostart_entry_smart() {
                    Ok(_) => {
                        startup_hint.set_visible(false);
                        shell.update_config(|config| config.start_at_login = true);
                        glib::Propagation::Proceed
                    }
                    Err(error) => {
                        eprintln!("[onboarding] could not install the startup entry: {error}");
                        report_error(
                            &startup_hint,
                            &t("Startup isn't available from this build. Settings can add it once the capture shell is installed."),
                        );
                        startup_hint.set_visible(true);
                        glib::Propagation::Stop
                    }
                }
            } else {
                if let Err(error) = crate::settings::windowing::uninstall_autostart_entry() {
                    eprintln!("[onboarding] could not remove the startup entry: {error}");
                }
                startup_hint.set_visible(false);
                shell.update_config(|config| config.start_at_login = false);
                glib::Propagation::Proceed
            }
        });
    }
    card.append(&settings_row(
        shell,
        icon_names::custom::ARROW2_TOP_RIGHT_SYMBOLIC,
        "Start ApexShot when Windows starts",
        Some("Your capture tools are there before you need them."),
        &startup,
    ));
    card.append(&startup_hint);

    let separator = GtkBox::new(Orientation::Horizontal, 0);
    separator.add_css_class("ob-separator");
    card.append(&separator);

    let telemetry = Switch::new();
    telemetry.add_css_class("ob-switch");
    telemetry.set_active(config.telemetry_enabled);
    {
        let shell = shell.clone();
        telemetry.connect_state_set(move |_, state| {
            shell.update_config(|config| config.telemetry_enabled = state);
            glib::Propagation::Proceed
        });
    }
    card.append(&settings_row(
        shell,
        icon_names::custom::INFO_SYMBOLIC,
        "Share anonymous diagnostics",
        Some("Crash and usage counts only. Never captures, filenames or personal data."),
        &telemetry,
    ));

    top.append(&card);
    page.append(&top);

    let start = primary_button(shell, "Get started", true);
    start.set_halign(Align::Center);
    start.set_size_request(px(shell.scale(), 220.0), -1);
    start.set_margin_bottom(px(shell.scale(), 42.0));
    {
        let shell = shell.clone();
        let open_settings = Rc::clone(&open_settings);
        start.connect_clicked(move |_| {
            if !shell.config().start_at_login {
                let _ = crate::settings::windowing::uninstall_autostart_entry();
            }
            shell.persist();
            shell.finish(open_settings.get());
        });
    }
    page.append(&start);
    shell.set_primary({
        let start = start.clone();
        Rc::new(move || start.emit_clicked())
    });

    page
}

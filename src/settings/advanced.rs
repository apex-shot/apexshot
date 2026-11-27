use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Label, Orientation};

use super::select::SettingsSelect;

#[allow(dead_code)]
pub struct AdvancedSettingsWidgets {
    pub section: GtkBox,
    pub retina_suffix_check: CheckButton,
    pub ocr_lang_input: SettingsSelect,
    pub ocr_keep_layout_check: CheckButton,
    pub ocr_line_breaks_check: CheckButton,
    pub organizer_auto_name_check: CheckButton,
    pub organizer_name_style_input: SettingsSelect,
    pub telemetry_enabled_check: CheckButton,
    pub telemetry_hint: Label,
}

pub fn build_advanced_section(config: &AppConfig) -> AdvancedSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 14);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(super::PAGE_TOP_MARGIN);
    section.set_margin_bottom(8);

    macro_rules! build_row {
        ($content:expr, $is_muted:expr) => {{
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            row.add_css_class("settings-table-row");
            if $is_muted {
                row.add_css_class("settings-table-row-muted");
            }
            row.set_hexpand(true);
            row.append($content);
            row
        }};
    }

    let build_frame = || -> gtk4::Box {
        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("settings-table-frame");
        frame.set_margin_bottom(24);
        frame.set_margin_start(4);
        frame.set_margin_end(4);
        frame
    };

    // --- File Naming Group ---
    let filename_title = Label::new(Some(&t("File Naming")));
    filename_title.add_css_class("settings-group-title");
    filename_title.set_xalign(0.0);
    filename_title.set_halign(Align::Start);
    filename_title.set_margin_bottom(8);
    section.append(&filename_title);

    let filename_frame = build_frame();

    // Filename format info
    let format_hbox = GtkBox::new(Orientation::Horizontal, 12);
    format_hbox.set_hexpand(true);
    let format_vbox = GtkBox::new(Orientation::Vertical, 4);
    format_vbox.set_hexpand(true);
    let lbl_format = Label::new(Some(&t("Filename format")));
    lbl_format.set_xalign(0.0);
    let format_hint = Label::new(Some(&t("ApexShot-YYYY-MM-DD_HH-MM-SS.png")));
    format_hint.add_css_class("settings-sub-option");
    format_hint.set_xalign(0.0);
    format_vbox.append(&lbl_format);
    format_vbox.append(&format_hint);
    format_hbox.append(&format_vbox);
    filename_frame.append(&build_row!(&format_hbox, false));

    // HiDPI/Retina suffix
    let retina_suffix_check = CheckButton::new();
    retina_suffix_check.set_active(config.adv_retina_suffix);
    // Only the Windows shell applies the @2x suffix, so the row is Windows-only.
    if cfg!(windows) {
        let retina_hbox = GtkBox::new(Orientation::Horizontal, 12);
        retina_hbox.set_hexpand(true);
        let lbl_retina = Label::new(Some(&t("Add @2x suffix for HiDPI screenshots")));
        lbl_retina.set_xalign(0.0);
        lbl_retina.set_hexpand(true);
        retina_hbox.append(&lbl_retina);
        retina_hbox.append(&retina_suffix_check);
        filename_frame.append(&build_row!(&retina_hbox, false));
    }

    section.append(&filename_frame);

    // --- Text Recognition Group ---
    let ocr_title = Label::new(Some(&t("Text Recognition (OCR)")));
    ocr_title.add_css_class("settings-group-title");
    ocr_title.set_xalign(0.0);
    ocr_title.set_halign(Align::Start);
    ocr_title.set_margin_bottom(8);
    section.append(&ocr_title);

    let ocr_frame = build_frame();

    // OCR Language
    let ocr_lang_input = SettingsSelect::new(
        [
            ("eng", t("English")),
            ("spa", t("Spanish")),
            ("fra", t("French")),
            ("deu", t("German")),
            ("ita", t("Italian")),
            ("por", t("Portuguese")),
            ("chi_sim", t("Chinese (Simplified)")),
            ("jpn", t("Japanese")),
            ("rus", t("Russian")),
        ],
        &config.adv_ocr_language,
    );

    let ocr_lang_hbox = GtkBox::new(Orientation::Horizontal, 12);
    ocr_lang_hbox.set_hexpand(true);
    let lbl_lang = Label::new(Some(&t("Primary language")));
    lbl_lang.set_xalign(0.0);
    lbl_lang.set_hexpand(true);
    ocr_lang_hbox.append(&lbl_lang);
    ocr_lang_hbox.append(ocr_lang_input.widget());
    ocr_frame.append(&build_row!(&ocr_lang_hbox, false));

    // Keep layout
    let ocr_keep_layout_check = CheckButton::new();
    ocr_keep_layout_check.set_active(config.ocr_keep_layout);
    let ocr_layout_hbox = GtkBox::new(Orientation::Horizontal, 12);
    ocr_layout_hbox.set_hexpand(true);
    let ocr_layout_vbox = GtkBox::new(Orientation::Vertical, 4);
    ocr_layout_vbox.set_hexpand(true);
    let lbl_layout = Label::new(Some(&t("Keep layout")));
    lbl_layout.set_xalign(0.0);
    let layout_hint = Label::new(Some(&t(
        "Copied text keeps tables, code indentation, and paragraphs.",
    )));
    layout_hint.add_css_class("settings-sub-option");
    layout_hint.set_xalign(0.0);
    layout_hint.set_wrap(true);
    layout_hint.set_max_width_chars(48);
    ocr_layout_vbox.append(&lbl_layout);
    ocr_layout_vbox.append(&layout_hint);
    ocr_layout_hbox.append(&ocr_layout_vbox);
    ocr_layout_hbox.append(&ocr_keep_layout_check);
    ocr_frame.append(&build_row!(&ocr_layout_hbox, false));

    // Keep line breaks
    let ocr_line_breaks_check = CheckButton::new();
    ocr_line_breaks_check.set_active(config.adv_ocr_keep_line_breaks);
    // Only the Windows OCR path honours this toggle, so the row is Windows-only.
    if cfg!(windows) {
        let ocr_breaks_hbox = GtkBox::new(Orientation::Horizontal, 12);
        ocr_breaks_hbox.set_hexpand(true);
        let lbl_breaks = Label::new(Some(&t("Preserve line breaks")));
        lbl_breaks.set_xalign(0.0);
        lbl_breaks.set_hexpand(true);
        ocr_breaks_hbox.append(&lbl_breaks);
        ocr_breaks_hbox.append(&ocr_line_breaks_check);
        ocr_frame.append(&build_row!(&ocr_breaks_hbox, true));
    }

    section.append(&ocr_frame);

    // --- Screenshot Names Group ---
    let organizer_title = Label::new(Some(&t("Screenshot names")));
    organizer_title.add_css_class("settings-group-title");
    organizer_title.set_xalign(0.0);
    organizer_title.set_halign(Align::Start);
    organizer_title.set_margin_bottom(8);
    section.append(&organizer_title);

    let organizer_frame = build_frame();

    let organizer_auto_name_check = CheckButton::new();
    organizer_auto_name_check.set_active(config.organizer_auto_name);
    let organizer_name_hbox = GtkBox::new(Orientation::Horizontal, 12);
    organizer_name_hbox.set_hexpand(true);
    let organizer_name_vbox = GtkBox::new(Orientation::Vertical, 4);
    organizer_name_vbox.set_hexpand(true);
    let lbl_organizer_name = Label::new(Some(&t("Name screenshots automatically")));
    lbl_organizer_name.set_xalign(0.0);
    let organizer_name_hint = Label::new(Some(&t(
        "Names come from the window title and on-screen text, on this computer. Private windows and explicit content get a dated name.",
    )));
    organizer_name_hint.add_css_class("settings-sub-option");
    organizer_name_hint.set_xalign(0.0);
    organizer_name_hint.set_wrap(true);
    organizer_name_hint.set_max_width_chars(48);
    organizer_name_vbox.append(&lbl_organizer_name);
    organizer_name_vbox.append(&organizer_name_hint);
    organizer_name_hbox.append(&organizer_name_vbox);
    organizer_name_hbox.append(&organizer_auto_name_check);
    organizer_frame.append(&build_row!(&organizer_name_hbox, false));

    let organizer_name_style_input = SettingsSelect::new(
        [
            ("clean", t("Title")),
            ("dated", t("Date and title")),
            ("by_app", t("App and title")),
        ],
        &config.organizer_name_style,
    );

    let organizer_style_hbox = GtkBox::new(Orientation::Horizontal, 12);
    organizer_style_hbox.set_hexpand(true);
    let lbl_organizer_style = Label::new(Some(&t("Name style")));
    lbl_organizer_style.set_xalign(0.0);
    lbl_organizer_style.set_hexpand(true);
    organizer_style_hbox.append(&lbl_organizer_style);
    organizer_style_hbox.append(organizer_name_style_input.widget());
    organizer_frame.append(&build_row!(&organizer_style_hbox, true));

    section.append(&organizer_frame);

    // --- Privacy Group ---
    let privacy_title = Label::new(Some(&t("Privacy")));
    privacy_title.add_css_class("settings-group-title");
    privacy_title.set_xalign(0.0);
    privacy_title.set_halign(Align::Start);
    privacy_title.set_margin_bottom(8);
    section.append(&privacy_title);

    let privacy_frame = build_frame();

    let telemetry_enabled_check = CheckButton::new();
    telemetry_enabled_check.set_active(config.telemetry_enabled);
    let telemetry_hbox = GtkBox::new(Orientation::Horizontal, 12);
    telemetry_hbox.set_hexpand(true);
    let telemetry_vbox = GtkBox::new(Orientation::Vertical, 4);
    telemetry_vbox.set_hexpand(true);
    let lbl_telemetry = Label::new(Some(&t("Share anonymous usage statistics")));
    lbl_telemetry.set_xalign(0.0);
    let telemetry_hint = Label::new(Some(&t(
        "Daily heartbeat: app version, distro, desktop, and coarse feature counts. The update window also reports each update download (version and size). No screenshots or personal data. Override with APEXSHOT_TELEMETRY=0.",
    )));
    telemetry_hint.add_css_class("settings-sub-option");
    telemetry_hint.set_xalign(0.0);
    telemetry_hint.set_wrap(true);
    telemetry_hint.set_max_width_chars(48);
    telemetry_vbox.append(&lbl_telemetry);
    telemetry_vbox.append(&telemetry_hint);
    telemetry_hbox.append(&telemetry_vbox);
    telemetry_hbox.append(&telemetry_enabled_check);
    privacy_frame.append(&build_row!(&telemetry_hbox, false));

    section.append(&privacy_frame);

    AdvancedSettingsWidgets {
        section,
        retina_suffix_check,
        ocr_lang_input,
        ocr_keep_layout_check,
        ocr_line_breaks_check,
        organizer_auto_name_check,
        organizer_name_style_input,
        telemetry_enabled_check,
        telemetry_hint,
    }
}

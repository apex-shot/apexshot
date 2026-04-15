use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Label, Orientation,
};

#[allow(dead_code)]
pub struct AdvancedSettingsWidgets {
    pub section: GtkBox,
    pub ask_name_check: CheckButton,
    pub retina_suffix_check: CheckButton,
    pub clipboard_mode_input: ComboBoxText,
    pub ocr_lang_input: ComboBoxText,
    pub ocr_line_breaks_check: CheckButton,
}

pub fn build_advanced_section(config: &AppConfig) -> AdvancedSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 14);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(20);
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
    let filename_title = Label::new(Some("File Naming"));
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
    let lbl_format = Label::new(Some("Filename format"));
    lbl_format.set_xalign(0.0);
    let format_hint = Label::new(Some("ApexShot-YYYY-MM-DD_HH-MM-SS.png"));
    format_hint.add_css_class("settings-sub-option");
    format_hint.set_xalign(0.0);
    format_vbox.append(&lbl_format);
    format_vbox.append(&format_hint);
    format_hbox.append(&format_vbox);
    filename_frame.append(&build_row!(&format_hbox, false));

    // Ask name after capture
    let ask_name_check = CheckButton::new();
    ask_name_check.set_active(config.adv_ask_name_after_capture);
    let ask_name_hbox = GtkBox::new(Orientation::Horizontal, 12);
    ask_name_hbox.set_hexpand(true);
    let lbl_ask_name = Label::new(Some("Ask for name after capture"));
    lbl_ask_name.set_xalign(0.0);
    lbl_ask_name.set_hexpand(true);
    ask_name_hbox.append(&lbl_ask_name);
    ask_name_hbox.append(&ask_name_check);
    filename_frame.append(&build_row!(&ask_name_hbox, true));

    // HiDPI/Retina suffix
    let retina_suffix_check = CheckButton::new();
    retina_suffix_check.set_active(config.adv_retina_suffix);
    let retina_hbox = GtkBox::new(Orientation::Horizontal, 12);
    retina_hbox.set_hexpand(true);
    let lbl_retina = Label::new(Some("Add @2x suffix for HiDPI screenshots"));
    lbl_retina.set_xalign(0.0);
    lbl_retina.set_hexpand(true);
    retina_hbox.append(&lbl_retina);
    retina_hbox.append(&retina_suffix_check);
    filename_frame.append(&build_row!(&retina_hbox, false));

    section.append(&filename_frame);

    // --- Clipboard Group ---
    let clipboard_title = Label::new(Some("Clipboard"));
    clipboard_title.add_css_class("settings-group-title");
    clipboard_title.set_xalign(0.0);
    clipboard_title.set_halign(Align::Start);
    clipboard_title.set_margin_bottom(8);
    section.append(&clipboard_title);

    let clipboard_frame = build_frame();

    // Clipboard mode
    let clipboard_mode_input = ComboBoxText::new();
    clipboard_mode_input.add_css_class("settings-select");
    clipboard_mode_input.append(Some("File & Image (default)"), "File & Image (default)");
    clipboard_mode_input.append(Some("Image Only"), "Image Only");
    clipboard_mode_input.set_active_id(Some(&config.adv_clipboard_mode));

    let clipboard_hbox = GtkBox::new(Orientation::Horizontal, 12);
    clipboard_hbox.set_hexpand(true);
    let clip_vbox = GtkBox::new(Orientation::Vertical, 4);
    clip_vbox.set_hexpand(true);
    let lbl_clip = Label::new(Some("Copy behavior"));
    lbl_clip.set_xalign(0.0);
    let clip_hint = Label::new(Some("Adjust if you encounter issues with clipboard managers."));
    clip_hint.add_css_class("settings-sub-option-hint");
    clip_hint.set_xalign(0.0);
    clip_vbox.append(&lbl_clip);
    clip_vbox.append(&clip_hint);
    clipboard_hbox.append(&clip_vbox);
    clipboard_hbox.append(&clipboard_mode_input);
    clipboard_frame.append(&build_row!(&clipboard_hbox, false));

    section.append(&clipboard_frame);

    // --- Text Recognition Group ---
    let ocr_title = Label::new(Some("Text Recognition (OCR)"));
    ocr_title.add_css_class("settings-group-title");
    ocr_title.set_xalign(0.0);
    ocr_title.set_halign(Align::Start);
    ocr_title.set_margin_bottom(8);
    section.append(&ocr_title);

    let ocr_frame = build_frame();

    // OCR Language
    let ocr_lang_input = ComboBoxText::new();
    ocr_lang_input.add_css_class("settings-select");
    ocr_lang_input.append(Some("eng"), "English");
    ocr_lang_input.append(Some("spa"), "Spanish");
    ocr_lang_input.append(Some("fra"), "French");
    ocr_lang_input.append(Some("deu"), "German");
    ocr_lang_input.append(Some("ita"), "Italian");
    ocr_lang_input.append(Some("por"), "Portuguese");
    ocr_lang_input.append(Some("chi_sim"), "Chinese (Simplified)");
    ocr_lang_input.append(Some("jpn"), "Japanese");
    ocr_lang_input.append(Some("rus"), "Russian");
    ocr_lang_input.set_active_id(Some(&config.adv_ocr_language));

    let ocr_lang_hbox = GtkBox::new(Orientation::Horizontal, 12);
    ocr_lang_hbox.set_hexpand(true);
    let lbl_lang = Label::new(Some("Primary language"));
    lbl_lang.set_xalign(0.0);
    lbl_lang.set_hexpand(true);
    ocr_lang_hbox.append(&lbl_lang);
    ocr_lang_hbox.append(&ocr_lang_input);
    ocr_frame.append(&build_row!(&ocr_lang_hbox, false));

    // Keep line breaks
    let ocr_line_breaks_check = CheckButton::new();
    ocr_line_breaks_check.set_active(config.adv_ocr_keep_line_breaks);
    let ocr_breaks_hbox = GtkBox::new(Orientation::Horizontal, 12);
    ocr_breaks_hbox.set_hexpand(true);
    let lbl_breaks = Label::new(Some("Preserve line breaks"));
    lbl_breaks.set_xalign(0.0);
    lbl_breaks.set_hexpand(true);
    ocr_breaks_hbox.append(&lbl_breaks);
    ocr_breaks_hbox.append(&ocr_line_breaks_check);
    ocr_frame.append(&build_row!(&ocr_breaks_hbox, true));

    section.append(&ocr_frame);

    AdvancedSettingsWidgets {
        section,
        ask_name_check,
        retina_suffix_check,
        clipboard_mode_input,
        ocr_lang_input,
        ocr_line_breaks_check,
    }
}

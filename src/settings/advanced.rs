use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Grid, Label,
    Orientation, Window,
};

#[allow(dead_code)]
pub struct AdvancedSettingsWidgets {
    pub section: GtkBox,
    pub filename_edit_btn: Button,
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

    // Edit filename template
    let filename_edit_btn = Button::with_label("Edit");
    filename_edit_btn.add_css_class("secondary-settings-button");
    let filename_hbox = GtkBox::new(Orientation::Horizontal, 12);
    filename_hbox.set_hexpand(true);
    let lbl_filename = Label::new(Some("Filename template"));
    lbl_filename.set_xalign(0.0);
    lbl_filename.set_hexpand(true);
    filename_hbox.append(&lbl_filename);
    filename_hbox.append(&filename_edit_btn);
    filename_frame.append(&build_row!(&filename_hbox, false));

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
        filename_edit_btn,
        ask_name_check,
        retina_suffix_check,
        clipboard_mode_input,
        ocr_lang_input,
        ocr_line_breaks_check,
    }
}

pub fn show_filename_format_modal(parent: &impl IsA<Window>, config: &AppConfig) {
    let dialog = Window::new();
    dialog.set_title(Some("Filename Template"));
    dialog.set_transient_for(Some(parent));
    dialog.set_modal(true);
    dialog.set_default_size(500, -1);
    dialog.set_resizable(false);

    let vbox = GtkBox::new(Orientation::Vertical, 20);
    vbox.add_css_class("modal-container");
    vbox.set_hexpand(true);
    vbox.set_vexpand(true);
    vbox.set_margin_start(30);
    vbox.set_margin_end(30);
    vbox.set_margin_top(24);
    vbox.set_margin_bottom(24);

    let instr = Label::new(Some("Create a custom filename format:"));
    instr.set_xalign(0.0);
    vbox.append(&instr);

    let entry = Entry::new();
    entry.set_text(&config.adv_filename_pattern);
    entry.add_css_class("format-entry");
    vbox.append(&entry);

    // Preview
    let preview_box = GtkBox::new(Orientation::Horizontal, 8);
    preview_box.set_margin_top(4);
    let preview_label = Label::new(Some("Preview:"));
    preview_label.add_css_class("settings-sub-option-hint");
    let preview_text = Label::new(Some("ApexShot 2025-04-16 at 14.30.45"));
    preview_text.add_css_class("settings-sub-option");
    preview_box.append(&preview_label);
    preview_box.append(&preview_text);
    vbox.append(&preview_box);

    // Tag palette
    let palette_box = GtkBox::new(Orientation::Vertical, 16);
    palette_box.set_margin_top(10);
    palette_box.set_margin_bottom(10);
    palette_box.add_css_class("format-palette-box");

    let grid = Grid::new();
    grid.set_column_spacing(40);
    grid.set_row_spacing(12);

    let tags = [
        ("Year:", "%y", "Hour:", "%H"),
        ("Month:", "%m", "Minutes:", "%M"),
        ("Day:", "%d", "Seconds:", "%S"),
        ("Window:", "%t", "Random:", "%r"),
        ("App:", "%a", "", ""),
    ];

    let mut r = 0;
    for (l1, t1, l2, t2) in tags {
        let lbl1 = Label::new(Some(l1));
        lbl1.set_xalign(1.0);
        lbl1.add_css_class("settings-sub-option-hint");
        let btn1 = Button::with_label(t1);
        btn1.add_css_class("filename-tag-pill");
        grid.attach(&lbl1, 0, r, 1, 1);
        grid.attach(&btn1, 1, r, 1, 1);

        if !l2.is_empty() {
            let lbl2 = Label::new(Some(l2));
            lbl2.set_xalign(1.0);
            lbl2.add_css_class("settings-sub-option-hint");
            let btn2 = Button::with_label(t2);
            btn2.add_css_class("filename-tag-pill");
            grid.attach(&lbl2, 2, r, 1, 1);
            grid.attach(&btn2, 3, r, 1, 1);

            let e = entry.clone();
            let t = t2.to_string();
            btn2.connect_clicked(move |_| {
                let pos = e.position();
                let txt = e.text().to_string();
                let mut new_txt = txt.clone();
                new_txt.insert_str(pos as usize, &t);
                e.set_text(&new_txt);
                e.set_position(pos + t.len() as i32);
            });
        }

        let e = entry.clone();
        let t = t1.to_string();
        btn1.connect_clicked(move |_| {
            let pos = e.position();
            let txt = e.text().to_string();
            let mut new_txt = txt.clone();
            new_txt.insert_str(pos as usize, &t);
            e.set_text(&new_txt);
            e.set_position(pos + t.len() as i32);
        });

        r += 1;
    }
    palette_box.append(&grid);
    vbox.append(&palette_box);

    let utc_check = CheckButton::with_label("Use UTC time zone");
    utc_check.set_active(config.adv_filename_use_utc);
    vbox.append(&utc_check);

    // Buttons
    let bottom_box = GtkBox::new(Orientation::Horizontal, 12);
    let restore_btn = Button::with_label("Restore Defaults");
    restore_btn.add_css_class("secondary-settings-button");
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let cancel_btn = Button::with_label("Cancel");
    cancel_btn.add_css_class("secondary-settings-button");
    let ok_btn = Button::with_label("OK");
    ok_btn.add_css_class("primary-settings-button");
    ok_btn.set_width_request(80);

    bottom_box.append(&restore_btn);
    bottom_box.append(&spacer);
    bottom_box.append(&cancel_btn);
    bottom_box.append(&ok_btn);
    vbox.append(&bottom_box);

    let d = dialog.clone();
    cancel_btn.connect_clicked(move |_| d.close());
    let d2 = dialog.clone();
    ok_btn.connect_clicked(move |_| d2.close());

    dialog.set_child(Some(&vbox));
    dialog.present();
}

use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Grid, Label, Orientation, Separator, Button, Entry, Window};

#[allow(dead_code)]
pub struct AdvancedSettingsWidgets {
    pub section: GtkBox,
    pub filename_edit_btn: Button,
    pub ask_name_check: CheckButton,
    pub retina_suffix_check: CheckButton,
    pub clipboard_mode_input: ComboBoxText,
    pub pinned_rounded_check: CheckButton,
    pub pinned_shadow_check: CheckButton,
    pub pinned_border_check: CheckButton,
    pub ocr_lang_input: ComboBoxText,
    pub ocr_line_breaks_check: CheckButton,
    pub reset_dialogs_btn: Button,
}

pub fn build_advanced_section(config: &AppConfig) -> AdvancedSettingsWidgets {
    // ... we will connect at build time later, or just return widgets and let mod.rs connect
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let grid = Grid::new();
    grid.set_column_spacing(24);
    grid.set_row_spacing(12); // Tighter row spacing for sub-items
    grid.set_margin_top(20);
    grid.set_hexpand(true);

    // Spacers for center-hugging
    let l_spacer = GtkBox::new(Orientation::Horizontal, 0); l_spacer.set_hexpand(true);
    let r_spacer = GtkBox::new(Orientation::Horizontal, 0); r_spacer.set_hexpand(true);
    grid.attach(&l_spacer, 0, 0, 1, 1);
    grid.attach(&r_spacer, 3, 0, 1, 1);

    let mut row = 0;
    let label_group = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);

    // 1. File name
    let filename_label = Label::new(Some("File name:"));
    filename_label.add_css_class("settings-group-title");
    filename_label.set_xalign(1.0);
    filename_label.set_halign(Align::End);
    label_group.add_widget(&filename_label);

    let filename_edit_btn = Button::with_label("Edit");
    filename_edit_btn.add_css_class("secondary-settings-button");
    filename_edit_btn.set_halign(Align::Start);
    
    grid.attach(&filename_label, 1, row, 1, 1);
    grid.attach(&filename_edit_btn, 2, row, 1, 1);

    row += 1;
    let ask_name_check = CheckButton::with_label("Ask for name after every capture");
    ask_name_check.set_active(config.adv_ask_name_after_capture);
    ask_name_check.set_halign(Align::Start);
    grid.attach(&ask_name_check, 2, row, 1, 1);

    row += 1;
    let retina_suffix_check = CheckButton::with_label("Add @2x suffix to Retina screenshots");
    retina_suffix_check.set_active(config.adv_retina_suffix);
    retina_suffix_check.set_halign(Align::Start);
    grid.attach(&retina_suffix_check, 2, row, 1, 1);

    row += 1;
    let sep1 = Separator::new(Orientation::Horizontal);
    sep1.set_margin_top(12);
    sep1.set_margin_bottom(12);
    grid.attach(&sep1, 0, row, 4, 1);

    row += 1;
    // 2. Copy to clipboard
    let clipboard_label = Label::new(Some("Copy to clipboard:"));
    clipboard_label.add_css_class("settings-group-title");
    clipboard_label.set_xalign(1.0);
    clipboard_label.set_halign(Align::End);
    label_group.add_widget(&clipboard_label);

    let clipboard_mode_input = ComboBoxText::new();
    clipboard_mode_input.append(Some("File & Image (default)"), "File & Image (default)");
    clipboard_mode_input.append(Some("Image Only"), "Image Only");
    clipboard_mode_input.set_active_id(Some(&config.adv_clipboard_mode));
    clipboard_mode_input.set_halign(Align::Start);
    
    grid.attach(&clipboard_label, 1, row, 1, 1);
    grid.attach(&clipboard_mode_input, 2, row, 1, 1);

    row += 1;
    let clip_hint = Label::new(Some("Adjust this option if you've encountered any issues\nwith pasting from clipboard or clipboard managers."));
    clip_hint.add_css_class("settings-sub-option-hint");
    clip_hint.set_xalign(0.0);
    clip_hint.set_halign(Align::Start);
    grid.attach(&clip_hint, 2, row, 1, 1);

    row += 1;
    let sep2 = Separator::new(Orientation::Horizontal);
    sep2.set_margin_top(12);
    sep2.set_margin_bottom(12);
    grid.attach(&sep2, 0, row, 4, 1);

    row += 1;
    // 3. Pinned screenshots
    let pinned_label = Label::new(Some("Pinned screenshots:"));
    pinned_label.add_css_class("settings-group-title");
    pinned_label.set_xalign(1.0);
    pinned_label.set_halign(Align::End);
    label_group.add_widget(&pinned_label);

    let pinned_rounded_check = CheckButton::with_label("Rounded corners");
    pinned_rounded_check.set_active(config.adv_pinned_rounded_corners);
    pinned_rounded_check.set_halign(Align::Start);
    grid.attach(&pinned_label, 1, row, 1, 1);
    grid.attach(&pinned_rounded_check, 2, row, 1, 1);

    row += 1;
    let pinned_shadow_check = CheckButton::with_label("Shadow");
    pinned_shadow_check.set_active(config.adv_pinned_shadow);
    pinned_shadow_check.set_halign(Align::Start);
    grid.attach(&pinned_shadow_check, 2, row, 1, 1);

    row += 1;
    let pinned_border_check = CheckButton::with_label("Border");
    pinned_border_check.set_active(config.adv_pinned_border);
    pinned_border_check.set_halign(Align::Start);
    grid.attach(&pinned_border_check, 2, row, 1, 1);

    row += 1;
    let sep3 = Separator::new(Orientation::Horizontal);
    sep3.set_margin_top(12);
    sep3.set_margin_bottom(12);
    grid.attach(&sep3, 0, row, 4, 1);

    row += 1;
    // 4. Text recognition
    let ocr_label = Label::new(Some("Text recognition:"));
    ocr_label.add_css_class("settings-group-title");
    ocr_label.set_xalign(1.0);
    ocr_label.set_halign(Align::End);
    label_group.add_widget(&ocr_label);

    let lang_lbl = Label::new(Some("Main language:"));
    lang_lbl.add_css_class("settings-sub-option-hint");
    lang_lbl.set_xalign(0.0);
    lang_lbl.set_halign(Align::Start);
    grid.attach(&ocr_label, 1, row, 1, 1);
    grid.attach(&lang_lbl, 2, row, 1, 1);

    row += 1;
    let ocr_lang_input = ComboBoxText::new();
    ocr_lang_input.append(Some("English"), "English");
    ocr_lang_input.append(Some("Spanish"), "Spanish");
    ocr_lang_input.set_active_id(Some(&config.adv_ocr_language));
    ocr_lang_input.set_halign(Align::Start);
    grid.attach(&ocr_lang_input, 2, row, 1, 1);

    row += 1;
    let ocr_line_breaks_check = CheckButton::with_label("Keep line breaks");
    ocr_line_breaks_check.set_active(config.adv_ocr_keep_line_breaks);
    ocr_line_breaks_check.set_halign(Align::Start);
    grid.attach(&ocr_line_breaks_check, 2, row, 1, 1);

    row += 1;
    let sep4 = Separator::new(Orientation::Horizontal);
    sep4.set_margin_top(12);
    sep4.set_margin_bottom(12);
    grid.attach(&sep4, 0, row, 4, 1);

    row += 1;
    // 5. Dialogs
    let dialogs_label = Label::new(Some("Dialogs:"));
    dialogs_label.add_css_class("settings-group-title");
    dialogs_label.set_xalign(1.0);
    dialogs_label.set_halign(Align::End);
    label_group.add_widget(&dialogs_label);

    let reset_dialogs_btn = Button::with_label("Reset All Warning Dialogs");
    reset_dialogs_btn.add_css_class("secondary-settings-button");
    reset_dialogs_btn.set_halign(Align::Start);

    grid.attach(&dialogs_label, 1, row, 1, 1);
    grid.attach(&reset_dialogs_btn, 2, row, 1, 1);

    section.append(&grid);

    AdvancedSettingsWidgets {
        section,
        filename_edit_btn,
        ask_name_check,
        retina_suffix_check,
        clipboard_mode_input,
        pinned_rounded_check,
        pinned_shadow_check,
        pinned_border_check,
        ocr_lang_input,
        ocr_line_breaks_check,
        reset_dialogs_btn,
    }
}

pub fn show_filename_format_modal(parent: &impl IsA<Window>, config: &AppConfig) {
    let dialog = Window::new();
    dialog.set_title(Some("File Name Format"));
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

    let instr = Label::new(Some("Type text and drag elements to create a custom format:"));
    instr.set_xalign(0.0);
    vbox.append(&instr);

    let entry = Entry::new();
    entry.set_text(&config.adv_filename_pattern);
    entry.add_css_class("format-entry"); // We'll style it search-like/clean
    vbox.append(&entry);

    let preview_box = GtkBox::new(Orientation::Horizontal, 4);
    let preview_label = Label::new(Some("Preview:"));
    preview_label.set_opacity(0.6);
    let preview_text = Label::new(Some("CleanShot 2021-09-16 at 16.57.28"));
    preview_text.set_opacity(0.8);
    preview_box.append(&preview_label);
    preview_box.append(&preview_text);
    vbox.append(&preview_box);

    // PALETTE GRID
    let palette_box = GtkBox::new(Orientation::Vertical, 16);
    palette_box.set_margin_top(10);
    palette_box.set_margin_bottom(10);
    palette_box.add_css_class("format-palette-box"); // Gray background box
    
    let grid = Grid::new();
    grid.set_column_spacing(40);
    grid.set_row_spacing(12);

    let tags = [
        ("Year:", "%y", "Hour:", "%H"),
        ("Month:", "%m", "Minutes:", "%M"),
        ("Day:", "%d", "Seconds:", "%S"),
        ("Day of week:", "%w", "AM/PM:", "%p"),
        ("Window title:", "%t", "Random chars:", "%r"),
        ("App name:", "%a", "", ""),
    ];

    let mut r = 0;
    for (l1, t1, l2, t2) in tags {
        let lbl1 = Label::new(Some(l1)); lbl1.set_xalign(1.0);
        let btn1 = Button::with_label(t1); btn1.add_css_class("filename-tag-pill");
        grid.attach(&lbl1, 0, r, 1, 1);
        grid.attach(&btn1, 1, r, 1, 1);

        if !l2.is_empty() {
            let lbl2 = Label::new(Some(l2)); lbl2.set_xalign(1.0);
            let btn2 = Button::with_label(t2); btn2.add_css_class("filename-tag-pill");
            grid.attach(&lbl2, 2, r, 1, 1);
            grid.attach(&btn2, 3, r, 1, 1);
            
            let e = entry.clone(); let t = t2.to_string();
            btn2.connect_clicked(move |_| {
                let pos = e.position();
                let txt = e.text().to_string();
                let mut new_txt = txt.clone();
                new_txt.insert_str(pos as usize, &t);
                e.set_text(&new_txt);
                e.set_position(pos + t.len() as i32);
            });
        }
        
        let e = entry.clone(); let t = t1.to_string();
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

    let bottom_box = GtkBox::new(Orientation::Horizontal, 12);
    let restore_btn = Button::with_label("Restore Defaults");
    restore_btn.add_css_class("secondary-settings-button");
    let cancel_btn = Button::with_label("Cancel");
    cancel_btn.add_css_class("secondary-settings-button");
    let ok_btn = Button::with_label("OK");
    ok_btn.add_css_class("primary-settings-button");
    ok_btn.set_width_request(80);

    bottom_box.append(&restore_btn);
    let spacer = GtkBox::new(Orientation::Horizontal, 0); spacer.set_hexpand(true);
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

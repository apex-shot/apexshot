use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Grid, Label,
    Orientation, Popover, Scale, Stack,
};

#[allow(dead_code)]
pub struct RecordingSettingsWidgets {
    pub section: GtkBox,
    // General
    pub video_export_location_entry: Entry,
    pub video_export_location_browse: Button,
    pub rec_controls_check: CheckButton,
    pub rec_display_time_check: CheckButton,
    pub rec_hidpi_check: CheckButton,
    pub rec_notifications_check: CheckButton,
    pub rec_cursor_check: CheckButton,
    pub rec_clicks_check: CheckButton,
    pub rec_keystrokes_check: CheckButton,
    pub rec_remember_selection_check: CheckButton,
    pub rec_dim_screen_check: CheckButton,
    pub rec_countdown_check: CheckButton,
    // Click Options (Popover)
    pub rec_click_size_input: Scale,
    pub rec_click_color_input: ComboBoxText,
    pub rec_click_style_input: ComboBoxText,
    pub rec_click_animate_check: CheckButton,
    // Key Options (Popover)
    pub rec_key_size_input: Scale,
    pub rec_key_position_input: ComboBoxText,
    pub rec_key_appearance_input: ComboBoxText,
    pub rec_key_blur_bg_check: CheckButton,
    pub rec_key_filter_input: ComboBoxText,
    // Video
    pub rec_video_max_res_input: ComboBoxText,
    pub rec_video_fps_input: ComboBoxText,
    pub rec_video_mono_check: CheckButton,
    pub rec_video_open_editor_check: CheckButton,
    // GIF
    pub rec_gif_fps_input: Scale,
    pub rec_gif_quality_input: Scale,
    pub rec_gif_optimize_check: CheckButton,
    pub rec_gif_size_idx_input: ComboBoxText,
}

pub fn build_recording_section(config: &AppConfig) -> RecordingSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);
    section.set_vexpand(true);

    // --- Tab Switcher ---
    let switcher_box = GtkBox::new(Orientation::Horizontal, 0);
    switcher_box.set_halign(Align::Center);
    switcher_box.set_margin_top(10);
    switcher_box.set_margin_bottom(20);
    switcher_box.add_css_class("recording-tab-switcher");

    let btn_general = Button::with_label("General");
    let btn_video = Button::with_label("Video");
    let btn_gif = Button::with_label("GIF");

    for btn in [&btn_general, &btn_video, &btn_gif] {
        btn.add_css_class("recording-tab-button");
    }
    btn_general.add_css_class("active");

    switcher_box.append(&btn_general);
    switcher_box.append(&btn_video);
    switcher_box.append(&btn_gif);

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

    // --- Switcher Handlers ---
    let s_clone = stack.clone();
    let bg = btn_general.clone();
    let bv = btn_video.clone();
    let bgif = btn_gif.clone();
    btn_general.connect_clicked(move |_| {
        s_clone.set_visible_child_name("general");
        bg.add_css_class("active");
        bv.remove_css_class("active");
        bgif.remove_css_class("active");
    });
    let s_clone = stack.clone();
    let bg = btn_general.clone();
    let bv = btn_video.clone();
    let bgif = btn_gif.clone();
    btn_video.connect_clicked(move |_| {
        s_clone.set_visible_child_name("video");
        bg.remove_css_class("active");
        bv.add_css_class("active");
        bgif.remove_css_class("active");
    });
    let s_clone = stack.clone();
    let bg = btn_general.clone();
    let bv = btn_video.clone();
    let bgif = btn_gif.clone();
    btn_gif.connect_clicked(move |_| {
        s_clone.set_visible_child_name("gif");
        bg.remove_css_class("active");
        bv.remove_css_class("active");
        bgif.add_css_class("active");
    });

    // --- GENERAL TAB ---
    let general_grid = Grid::new();
    general_grid.set_hexpand(true);
    general_grid.set_row_spacing(24);
    general_grid.set_column_spacing(12);
    let l_spacer = GtkBox::new(Orientation::Horizontal, 0);
    l_spacer.set_hexpand(true);
    let r_spacer = GtkBox::new(Orientation::Horizontal, 0);
    r_spacer.set_hexpand(true);
    general_grid.attach(&l_spacer, 0, 0, 1, 1);
    general_grid.attach(&r_spacer, 4, 0, 1, 1);

    let mut row = 0;
    let create_row = |grid: &Grid, label_text: &str, row_idx: i32| -> CheckButton {
        let label = Label::new(Some(label_text));
        label.add_css_class("settings-group-title");
        label.set_xalign(1.0);
        label.set_size_request(140, -1);
        let check = CheckButton::new();
        let cell = GtkBox::new(Orientation::Horizontal, 0);
        cell.set_size_request(28, -1);
        cell.set_halign(Align::Start);
        cell.append(&check);
        grid.attach(&label, 1, row_idx, 1, 1);
        grid.attach(&cell, 2, row_idx, 1, 1);
        check
    };

    let save_location_label = Label::new(Some("Save location:"));
    save_location_label.add_css_class("settings-group-title");
    save_location_label.set_xalign(1.0);
    save_location_label.set_size_request(140, -1);
    let video_export_location_entry = Entry::new();
    video_export_location_entry.set_hexpand(true);
    video_export_location_entry.set_width_chars(28);
    video_export_location_entry.set_placeholder_text(Some("Choose a folder"));
    video_export_location_entry.set_text(&config.video_export_location);
    let video_export_location_browse = Button::with_label("Browse");
    let video_export_location_row = GtkBox::new(Orientation::Horizontal, 8);
    video_export_location_row.set_halign(Align::Start);
    video_export_location_row.append(&video_export_location_entry);
    video_export_location_row.append(&video_export_location_browse);
    general_grid.attach(&save_location_label, 1, row, 1, 1);
    general_grid.attach(&video_export_location_row, 3, row, 1, 1);

    row += 1;

    let rec_controls_check = create_row(&general_grid, "Controls:", row);
    rec_controls_check.set_active(config.rec_controls);
    let rec_controls_opt = Label::new(Some("Show controls while recording"));
    rec_controls_opt.set_xalign(0.0);
    general_grid.attach(&rec_controls_opt, 3, row, 1, 1);

    row += 1;
    let rec_display_time_check = create_row(&general_grid, "Menu bar:", row);
    rec_display_time_check.set_active(config.rec_display_time);
    let rec_display_time_opt = Label::new(Some("Display recording time"));
    rec_display_time_opt.set_xalign(0.0);
    general_grid.attach(&rec_display_time_opt, 3, row, 1, 1);

    row += 1;
    let rec_hidpi_check = create_row(&general_grid, "Retina:", row);
    rec_hidpi_check.set_active(config.rec_hidpi);
    let rec_hidpi_opt = Label::new(Some("Scale Retina videos to 1x"));
    rec_hidpi_opt.set_xalign(0.0);
    general_grid.attach(&rec_hidpi_opt, 3, row, 1, 1);

    row += 1;
    let rec_notifications_check = create_row(&general_grid, "Notifications:", row);
    rec_notifications_check.set_active(config.rec_notifications);
    let rec_notifications_opt = Label::new(Some("Enable \"Do Not Disturb\" while recording"));
    rec_notifications_opt.set_xalign(0.0);
    general_grid.attach(&rec_notifications_opt, 3, row, 1, 1);

    row += 1;
    let rec_cursor_check = create_row(&general_grid, "Cursor:", row);
    rec_cursor_check.set_active(config.rec_cursor);
    let rec_cursor_opt = Label::new(Some("Show cursor"));
    rec_cursor_opt.set_xalign(0.0);
    general_grid.attach(&rec_cursor_opt, 3, row, 1, 1);

    row += 1;
    let rec_clicks_check = CheckButton::new();
    rec_clicks_check.set_active(config.rec_clicks);
    let clicks_cell = GtkBox::new(Orientation::Horizontal, 0);
    clicks_cell.set_size_request(28, -1);
    clicks_cell.set_halign(Align::Start);
    clicks_cell.append(&rec_clicks_check);
    let clicks_hbox = GtkBox::new(Orientation::Horizontal, 12);
    let clicks_opt = Label::new(Some("Highlight clicks"));
    clicks_opt.set_xalign(0.0);
    let clicks_btn = Button::with_label("Options...");
    clicks_btn.add_css_class("settings-action-button");
    clicks_hbox.append(&clicks_opt);
    clicks_hbox.append(&clicks_btn);
    general_grid.attach(&clicks_cell, 2, row, 1, 1);
    general_grid.attach(&clicks_hbox, 3, row, 1, 1);

    // Click Options Popover
    let click_popover = Popover::new();
    let click_content = GtkBox::new(Orientation::Vertical, 16);
    click_content.set_margin_top(12);
    click_content.set_margin_bottom(12);
    click_content.set_margin_start(16);
    click_content.set_margin_end(16);

    let rec_click_size_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    rec_click_size_input.set_value(config.rec_click_size);
    rec_click_size_input.set_size_request(120, -1);
    let size_box = GtkBox::new(Orientation::Horizontal, 10);
    size_box.append(&Label::new(Some("Size")));
    size_box.append(&rec_click_size_input);
    click_content.append(&size_box);

    let rec_click_color_input = ComboBoxText::new();
    for c in [
        "Gray", "Indigo", "Red", "Blue", "Green", "Yellow", "Orange", "Purple", "White",
    ] {
        rec_click_color_input.append(Some(c), c);
    }
    rec_click_color_input.set_active(Some(config.rec_click_color as u32));
    let color_box = GtkBox::new(Orientation::Horizontal, 10);
    color_box.append(&Label::new(Some("Color")));
    color_box.append(&rec_click_color_input);
    click_content.append(&color_box);

    let rec_click_style_input = ComboBoxText::new();
    rec_click_style_input.append(Some("0"), "Outline");
    rec_click_style_input.append(Some("1"), "Filled");
    rec_click_style_input.set_active(Some(config.rec_click_style as u32));
    let style_box = GtkBox::new(Orientation::Horizontal, 10);
    style_box.append(&Label::new(Some("Style")));
    style_box.append(&rec_click_style_input);
    click_content.append(&style_box);

    let rec_click_animate_check = CheckButton::with_label("Animate clicks");
    rec_click_animate_check.set_active(config.rec_click_animate);
    click_content.append(&rec_click_animate_check);

    let click_done_btn = Button::with_label("Done");
    click_done_btn.add_css_class("suggested-action");
    click_done_btn.set_halign(Align::End);
    click_content.append(&click_done_btn);

    click_popover.set_child(Some(&click_content));
    click_popover.set_parent(&clicks_btn);
    let cp_clone = click_popover.clone();
    click_done_btn.connect_clicked(move |_| {
        cp_clone.popdown();
    });
    clicks_btn.connect_clicked(move |_| {
        click_popover.popup();
    });

    row += 1;
    let rec_keystrokes_check = create_row(&general_grid, "Keyboard:", row);
    rec_keystrokes_check.set_active(config.rec_keystrokes);
    let keys_hbox = GtkBox::new(Orientation::Horizontal, 12);
    let keys_opt = Label::new(Some("Show Keystrokes"));
    keys_opt.set_xalign(0.0);
    let keys_btn = Button::with_label("Options...");
    keys_btn.add_css_class("settings-action-button");
    keys_hbox.append(&keys_opt);
    keys_hbox.append(&keys_btn);
    general_grid.attach(&keys_hbox, 3, row, 1, 1);

    // Keystroke Options Popover
    let key_popover = Popover::new();
    let key_content = GtkBox::new(Orientation::Vertical, 16);
    key_content.set_margin_top(12);
    key_content.set_margin_bottom(12);
    key_content.set_margin_start(12);
    key_content.set_margin_end(12);

    let rec_key_size_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    rec_key_size_input.set_value(config.rec_key_size);
    rec_key_size_input.set_size_request(120, -1);
    let ksize_box = GtkBox::new(Orientation::Horizontal, 10);
    ksize_box.append(&Label::new(Some("Size")));
    ksize_box.append(&rec_key_size_input);
    key_content.append(&ksize_box);

    let rec_key_position_input = ComboBoxText::new();
    for p in [
        "Bottom-Center",
        "Bottom-Left",
        "Bottom-Right",
        "Top-Center",
        "Top-Left",
        "Top-Right",
    ] {
        rec_key_position_input.append(Some(p), p);
    }
    rec_key_position_input.set_active(Some(config.rec_key_position as u32));
    let kpos_box = GtkBox::new(Orientation::Horizontal, 10);
    kpos_box.append(&Label::new(Some("Position")));
    kpos_box.append(&rec_key_position_input);
    key_content.append(&kpos_box);

    let rec_key_appearance_input = ComboBoxText::new();
    rec_key_appearance_input.append(Some("0"), "Dark");
    rec_key_appearance_input.append(Some("1"), "Light");
    rec_key_appearance_input.set_active(Some(config.rec_key_appearance as u32));
    let kapp_box = GtkBox::new(Orientation::Horizontal, 10);
    kapp_box.append(&Label::new(Some("Appearance")));
    kapp_box.append(&rec_key_appearance_input);
    key_content.append(&kapp_box);

    let rec_key_blur_bg_check = CheckButton::with_label("Blur background");
    rec_key_blur_bg_check.set_active(config.rec_key_blur_bg);
    key_content.append(&rec_key_blur_bg_check);

    let rec_key_filter_input = ComboBoxText::new();
    rec_key_filter_input.append(Some("0"), "Show all keys");
    rec_key_filter_input.append(Some("1"), "Show only command keys");
    rec_key_filter_input.set_active(Some(config.rec_key_filter as u32));
    key_content.append(&rec_key_filter_input);

    let key_done_btn = Button::with_label("Done");
    key_done_btn.add_css_class("suggested-action");
    key_done_btn.set_halign(Align::End);
    key_content.append(&key_done_btn);

    key_popover.set_child(Some(&key_content));
    key_popover.set_parent(&keys_btn);
    let kp_clone = key_popover.clone();
    key_done_btn.connect_clicked(move |_| {
        kp_clone.popdown();
    });
    keys_btn.connect_clicked(move |_| {
        key_popover.popup();
    });

    row += 1;
    let rec_remember_selection_check = create_row(&general_grid, "Recording area:", row);
    rec_remember_selection_check.set_active(config.rec_remember_selection);
    let rec_area_opt = Label::new(Some("Remember last selection"));
    rec_area_opt.set_xalign(0.0);
    general_grid.attach(&rec_area_opt, 3, row, 1, 1);

    row += 1;
    let rec_dim_screen_check = CheckButton::new();
    rec_dim_screen_check.set_active(config.rec_dim_screen);
    let dim_cell = GtkBox::new(Orientation::Horizontal, 0);
    dim_cell.set_size_request(28, -1);
    dim_cell.set_halign(Align::Start);
    dim_cell.append(&rec_dim_screen_check);
    let dim_opt = Label::new(Some("Dim screen while recording"));
    dim_opt.set_xalign(0.0);
    general_grid.attach(&dim_cell, 2, row, 1, 1);
    general_grid.attach(&dim_opt, 3, row, 1, 1);

    row += 1;
    let rec_countdown_check = CheckButton::new();
    rec_countdown_check.set_active(config.rec_countdown);
    let countdown_cell = GtkBox::new(Orientation::Horizontal, 0);
    countdown_cell.set_size_request(28, -1);
    countdown_cell.set_halign(Align::Start);
    countdown_cell.append(&rec_countdown_check);
    let countdown_opt = Label::new(Some("Show countdown before start"));
    countdown_opt.set_xalign(0.0);
    general_grid.attach(&countdown_cell, 2, row, 1, 1);
    general_grid.attach(&countdown_opt, 3, row, 1, 1);

    stack.add_named(&general_grid, Some("general"));

    // --- VIDEO TAB ---
    let video_grid = Grid::new();
    video_grid.set_hexpand(true);
    video_grid.set_row_spacing(30);
    video_grid.set_column_spacing(12);
    let l_spacer_v = GtkBox::new(Orientation::Horizontal, 0);
    l_spacer_v.set_hexpand(true);
    let r_spacer_v = GtkBox::new(Orientation::Horizontal, 0);
    r_spacer_v.set_hexpand(true);
    video_grid.attach(&l_spacer_v, 0, 0, 1, 1);
    video_grid.attach(&r_spacer_v, 4, 0, 1, 1);

    let mut vrow = 0;
    let res_label = Label::new(Some("Max resolution:"));
    res_label.add_css_class("settings-group-title");
    res_label.set_xalign(1.0);
    res_label.set_size_request(140, -1);
    let rec_video_max_res_input = ComboBoxText::new();
    rec_video_max_res_input.add_css_class("settings-select");
    for (i, lbl) in [("0", "Original"), ("1", "1080p"), ("2", "720p")] {
        rec_video_max_res_input.append(Some(i), lbl);
    }
    rec_video_max_res_input.set_active_id(Some(&config.rec_video_max_res.to_string()));
    rec_video_max_res_input.set_halign(Align::Start);
    video_grid.attach(&res_label, 1, vrow, 1, 1);
    video_grid.attach(&rec_video_max_res_input, 3, vrow, 1, 1);

    vrow += 1;
    let fps_label = Label::new(Some("Video FPS:"));
    fps_label.add_css_class("settings-group-title");
    fps_label.set_xalign(1.0);
    let rec_video_fps_input = ComboBoxText::new();
    rec_video_fps_input.add_css_class("settings-select");
    for (i, lbl) in [("0", "24"), ("1", "30"), ("2", "50"), ("3", "60")] {
        rec_video_fps_input.append(Some(i), lbl);
    }
    rec_video_fps_input.set_active_id(Some(&config.rec_video_fps.to_string()));
    rec_video_fps_input.set_halign(Align::Start);
    video_grid.attach(&fps_label, 1, vrow, 1, 1);
    video_grid.attach(&rec_video_fps_input, 3, vrow, 1, 1);

    vrow += 1;
    let audio_label = Label::new(Some("Audio:"));
    audio_label.add_css_class("settings-group-title");
    audio_label.set_xalign(1.0);
    let audio_btn = Button::with_label("Computer Audio Settings...");
    audio_btn.add_css_class("settings-action-button");
    audio_btn.set_halign(Align::Start);
    video_grid.attach(&audio_label, 1, vrow, 1, 1);
    video_grid.attach(&audio_btn, 3, vrow, 1, 1);

    vrow += 1;
    let rec_video_mono_check = CheckButton::new();
    rec_video_mono_check.set_active(config.rec_video_mono);
    let mono_cell = GtkBox::new(Orientation::Horizontal, 0);
    mono_cell.set_size_request(28, -1);
    mono_cell.set_halign(Align::Start);
    mono_cell.append(&rec_video_mono_check);
    let mono_opt = Label::new(Some("Record audio in mono"));
    mono_opt.set_xalign(0.0);
    video_grid.attach(&mono_cell, 2, vrow, 1, 1);
    video_grid.attach(&mono_opt, 3, vrow, 1, 1);

    vrow += 1;
    let rec_video_open_editor_check = CheckButton::new();
    rec_video_open_editor_check.set_active(config.rec_video_open_editor);
    let editor_cell = GtkBox::new(Orientation::Horizontal, 0);
    editor_cell.set_size_request(28, -1);
    editor_cell.set_halign(Align::Start);
    editor_cell.append(&rec_video_open_editor_check);
    let editor_opt = Label::new(Some("Open Video Editor after recording"));
    editor_opt.set_xalign(0.0);
    video_grid.attach(&editor_cell, 2, vrow, 1, 1);
    video_grid.attach(&editor_opt, 3, vrow, 1, 1);

    stack.add_named(&video_grid, Some("video"));

    // --- GIF TAB ---
    let gif_grid = Grid::new();
    gif_grid.set_hexpand(true);
    gif_grid.set_row_spacing(30);
    gif_grid.set_column_spacing(12);
    let l_spacer_g = GtkBox::new(Orientation::Horizontal, 0);
    l_spacer_g.set_hexpand(true);
    let r_spacer_g = GtkBox::new(Orientation::Horizontal, 0);
    r_spacer_g.set_hexpand(true);
    gif_grid.attach(&l_spacer_g, 0, 0, 1, 1);
    gif_grid.attach(&r_spacer_g, 4, 0, 1, 1);

    let mut grow = 0;
    let gfps_label = Label::new(Some("GIF FPS:"));
    gfps_label.add_css_class("settings-group-title");
    gfps_label.set_xalign(1.0);
    gfps_label.set_size_request(140, -1);
    let rec_gif_fps_input = Scale::with_range(Orientation::Horizontal, 5.0, 60.0, 1.0);
    rec_gif_fps_input.set_value(config.rec_gif_fps as f64);
    rec_gif_fps_input.set_size_request(150, -1);
    gif_grid.attach(&gfps_label, 1, grow, 1, 1);
    gif_grid.attach(&rec_gif_fps_input, 3, grow, 1, 1);

    grow += 1;
    let gqual_label = Label::new(Some("GIF quality:"));
    gqual_label.add_css_class("settings-group-title");
    gqual_label.set_xalign(1.0);
    let rec_gif_quality_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    rec_gif_quality_input.set_value(config.rec_gif_quality);
    rec_gif_quality_input.set_size_request(150, -1);
    gif_grid.attach(&gqual_label, 1, grow, 1, 1);
    gif_grid.attach(&rec_gif_quality_input, 3, grow, 1, 1);

    grow += 1;
    let rec_gif_optimize_check = CheckButton::new();
    rec_gif_optimize_check.set_active(config.rec_gif_optimize);
    let opt_cell = GtkBox::new(Orientation::Horizontal, 0);
    opt_cell.set_size_request(28, -1);
    opt_cell.set_halign(Align::Start);
    opt_cell.append(&rec_gif_optimize_check);
    let opt_opt = Label::new(Some("Optimize GIFs"));
    opt_opt.set_xalign(0.0);
    gif_grid.attach(&opt_cell, 2, grow, 1, 1);
    gif_grid.attach(&opt_opt, 3, grow, 1, 1);

    grow += 1;
    let gsize_label = Label::new(Some("GIF size:"));
    gsize_label.add_css_class("settings-group-title");
    gsize_label.set_xalign(1.0);
    let rec_gif_size_idx_input = ComboBoxText::new();
    rec_gif_size_idx_input.add_css_class("settings-select");
    for (i, lbl) in [
        ("0", "800 x auto (default)"),
        ("1", "640 x auto"),
        ("2", "480 x auto"),
        ("3", "Original"),
    ] {
        rec_gif_size_idx_input.append(Some(i), lbl);
    }
    rec_gif_size_idx_input.set_active_id(Some(&config.rec_gif_size_idx.to_string()));
    rec_gif_size_idx_input.set_halign(Align::Start);
    gif_grid.attach(&gsize_label, 1, grow, 1, 1);
    gif_grid.attach(&rec_gif_size_idx_input, 3, grow, 1, 1);

    stack.add_named(&gif_grid, Some("gif"));

    section.append(&switcher_box);
    section.append(&stack);

    RecordingSettingsWidgets {
        section,
        video_export_location_entry,
        video_export_location_browse,
        rec_controls_check,
        rec_display_time_check,
        rec_hidpi_check,
        rec_notifications_check,
        rec_cursor_check,
        rec_clicks_check,
        rec_keystrokes_check,
        rec_remember_selection_check,
        rec_dim_screen_check,
        rec_countdown_check,
        rec_click_size_input,
        rec_click_color_input,
        rec_click_style_input,
        rec_click_animate_check,
        rec_key_size_input,
        rec_key_position_input,
        rec_key_appearance_input,
        rec_key_blur_bg_check,
        rec_key_filter_input,
        rec_video_max_res_input,
        rec_video_fps_input,
        rec_video_mono_check,
        rec_video_open_editor_check,
        rec_gif_fps_input,
        rec_gif_quality_input,
        rec_gif_optimize_check,
        rec_gif_size_idx_input,
    }
}

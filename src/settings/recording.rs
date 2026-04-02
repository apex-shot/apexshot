use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Label, Orientation,
    Popover, Scale, Stack,
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

    let general_frame = build_frame();
    general_frame.set_margin_top(12);

    let create_row = |frame: &GtkBox, label_text: &str, is_muted: bool| -> CheckButton {
        let hbox = GtkBox::new(Orientation::Horizontal, 12);
        hbox.set_hexpand(true);
        let label = Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let check = CheckButton::new();
        hbox.append(&label);
        hbox.append(&check);
        frame.append(&build_row!(&hbox, is_muted));
        check
    };

    let video_export_location_entry = Entry::new();
    video_export_location_entry.set_hexpand(true);
    video_export_location_entry.set_width_chars(28);
    video_export_location_entry.set_placeholder_text(Some("Choose a folder"));
    video_export_location_entry.set_text(&config.video_export_location);
    let video_export_location_browse = Button::with_label("Browse");

    let export_hbox = GtkBox::new(Orientation::Horizontal, 12);
    export_hbox.set_hexpand(true);
    let export_label = Label::new(Some("Save location"));
    export_label.set_xalign(0.0);
    export_label.set_hexpand(true);
    let entry_row = GtkBox::new(Orientation::Horizontal, 8);
    entry_row.append(&video_export_location_entry);
    entry_row.append(&video_export_location_browse);
    export_hbox.append(&export_label);
    export_hbox.append(&entry_row);
    general_frame.append(&build_row!(&export_hbox, false));

    let rec_controls_check = create_row(&general_frame, "Use keyboard shortcuts to control recordings (elapsed time appears in the top bar)", true);
    rec_controls_check.set_active(config.rec_controls);

    let rec_display_time_check =
        create_row(&general_frame, "Display recording time in the top bar", false);
    rec_display_time_check.set_active(config.rec_display_time);

    let rec_hidpi_check = create_row(&general_frame, "Scale Retina videos to 1x", true);
    rec_hidpi_check.set_active(config.rec_hidpi);

    let rec_notifications_check = create_row(
        &general_frame,
        "Enable \"Do Not Disturb\" while recording",
        false,
    );
    rec_notifications_check.set_active(config.rec_notifications);

    let rec_cursor_check = create_row(&general_frame, "Show cursor", true);
    rec_cursor_check.set_active(config.rec_cursor);

    let rec_clicks_check = CheckButton::new();
    rec_clicks_check.set_active(config.rec_clicks);
    let clicks_hbox = GtkBox::new(Orientation::Horizontal, 12);
    clicks_hbox.set_hexpand(true);
    let clicks_opt = Label::new(Some("Highlight clicks"));
    clicks_opt.set_xalign(0.0);
    clicks_opt.set_hexpand(true);
    let clicks_btn = Button::with_label("Options...");
    clicks_btn.add_css_class("secondary-settings-button");
    let clicks_rhs = GtkBox::new(Orientation::Horizontal, 8);
    clicks_rhs.append(&clicks_btn);
    clicks_rhs.append(&rec_clicks_check);
    clicks_hbox.append(&clicks_opt);
    clicks_hbox.append(&clicks_rhs);
    general_frame.append(&build_row!(&clicks_hbox, false));

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

    let rec_keystrokes_check = CheckButton::new();
    rec_keystrokes_check.set_active(config.rec_keystrokes);
    let keys_hbox = GtkBox::new(Orientation::Horizontal, 12);
    keys_hbox.set_hexpand(true);
    let keys_opt = Label::new(Some("Show Keystrokes"));
    keys_opt.set_xalign(0.0);
    keys_opt.set_hexpand(true);
    let keys_btn = Button::with_label("Options...");
    keys_btn.add_css_class("secondary-settings-button");
    let keys_rhs = GtkBox::new(Orientation::Horizontal, 8);
    keys_rhs.append(&keys_btn);
    keys_rhs.append(&rec_keystrokes_check);
    keys_hbox.append(&keys_opt);
    keys_hbox.append(&keys_rhs);
    general_frame.append(&build_row!(&keys_hbox, true));

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

    let rec_remember_selection_check =
        create_row(&general_frame, "Remember last selection area", false);
    rec_remember_selection_check.set_active(config.rec_remember_selection);

    let rec_dim_screen_check = create_row(&general_frame, "Dim screen while recording", true);
    rec_dim_screen_check.set_active(config.rec_dim_screen);

    let rec_countdown_check = create_row(&general_frame, "Show countdown before start", false);
    rec_countdown_check.set_active(config.rec_countdown);

    stack.add_named(&general_frame, Some("general"));

    // --- VIDEO TAB ---
    let video_frame = build_frame();
    video_frame.set_margin_top(12);

    let res_hbox = GtkBox::new(Orientation::Horizontal, 12);
    res_hbox.set_hexpand(true);
    let res_label = Label::new(Some("Max resolution"));
    res_label.set_xalign(0.0);
    res_label.set_hexpand(true);
    let rec_video_max_res_input = ComboBoxText::new();
    rec_video_max_res_input.add_css_class("settings-select");
    for (i, lbl) in [("0", "Original"), ("1", "1080p"), ("2", "720p")] {
        rec_video_max_res_input.append(Some(i), lbl);
    }
    rec_video_max_res_input.set_active_id(Some(&config.rec_video_max_res.to_string()));
    res_hbox.append(&res_label);
    res_hbox.append(&rec_video_max_res_input);
    video_frame.append(&build_row!(&res_hbox, false));

    let fps_hbox = GtkBox::new(Orientation::Horizontal, 12);
    fps_hbox.set_hexpand(true);
    let fps_label = Label::new(Some("Video FPS"));
    fps_label.set_xalign(0.0);
    fps_label.set_hexpand(true);
    let rec_video_fps_input = ComboBoxText::new();
    rec_video_fps_input.add_css_class("settings-select");
    for (i, lbl) in [("0", "24"), ("1", "30"), ("2", "50"), ("3", "60")] {
        rec_video_fps_input.append(Some(i), lbl);
    }
    rec_video_fps_input.set_active_id(Some(&config.rec_video_fps.to_string()));
    fps_hbox.append(&fps_label);
    fps_hbox.append(&rec_video_fps_input);
    video_frame.append(&build_row!(&fps_hbox, true));

    let audio_hbox = GtkBox::new(Orientation::Horizontal, 12);
    audio_hbox.set_hexpand(true);
    let audio_label = Label::new(Some("Audio"));
    audio_label.set_xalign(0.0);
    audio_label.set_hexpand(true);
    let audio_btn = Button::with_label("Computer Audio Settings...");
    audio_btn.add_css_class("secondary-settings-button");
    audio_hbox.append(&audio_label);
    audio_hbox.append(&audio_btn);
    video_frame.append(&build_row!(&audio_hbox, false));

    let rec_video_mono_check = CheckButton::new();
    rec_video_mono_check.set_active(config.rec_video_mono);
    let mono_hbox = GtkBox::new(Orientation::Horizontal, 12);
    mono_hbox.set_hexpand(true);
    let mono_opt = Label::new(Some("Record audio in mono"));
    mono_opt.set_xalign(0.0);
    mono_opt.set_hexpand(true);
    mono_hbox.append(&mono_opt);
    mono_hbox.append(&rec_video_mono_check);
    video_frame.append(&build_row!(&mono_hbox, true));

    let rec_video_open_editor_check = CheckButton::new();
    rec_video_open_editor_check.set_active(config.rec_video_open_editor);
    let editor_hbox = GtkBox::new(Orientation::Horizontal, 12);
    editor_hbox.set_hexpand(true);
    let editor_opt = Label::new(Some("Open Video Editor after recording"));
    editor_opt.set_xalign(0.0);
    editor_opt.set_hexpand(true);
    editor_hbox.append(&editor_opt);
    editor_hbox.append(&rec_video_open_editor_check);
    video_frame.append(&build_row!(&editor_hbox, false));

    stack.add_named(&video_frame, Some("video"));

    // --- GIF TAB ---
    let gif_frame = build_frame();
    gif_frame.set_margin_top(12);

    let rec_gif_fps_input = Scale::with_range(Orientation::Horizontal, 5.0, 60.0, 1.0);
    rec_gif_fps_input.set_value(config.rec_gif_fps as f64);
    rec_gif_fps_input.set_size_request(150, -1);
    let gfps_hbox = GtkBox::new(Orientation::Horizontal, 12);
    gfps_hbox.set_hexpand(true);
    let gfps_label = Label::new(Some("GIF FPS"));
    gfps_label.set_xalign(0.0);
    gfps_label.set_hexpand(true);
    gfps_hbox.append(&gfps_label);
    gfps_hbox.append(&rec_gif_fps_input);
    gif_frame.append(&build_row!(&gfps_hbox, false));

    let rec_gif_quality_input = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    rec_gif_quality_input.set_value(config.rec_gif_quality);
    rec_gif_quality_input.set_size_request(150, -1);
    let gqual_hbox = GtkBox::new(Orientation::Horizontal, 12);
    gqual_hbox.set_hexpand(true);
    let gqual_label = Label::new(Some("GIF quality"));
    gqual_label.set_xalign(0.0);
    gqual_label.set_hexpand(true);
    gqual_hbox.append(&gqual_label);
    gqual_hbox.append(&rec_gif_quality_input);
    gif_frame.append(&build_row!(&gqual_hbox, true));

    let rec_gif_optimize_check = CheckButton::new();
    rec_gif_optimize_check.set_active(config.rec_gif_optimize);
    let opt_hbox = GtkBox::new(Orientation::Horizontal, 12);
    opt_hbox.set_hexpand(true);
    let opt_opt = Label::new(Some("Optimize GIFs"));
    opt_opt.set_xalign(0.0);
    opt_opt.set_hexpand(true);
    opt_hbox.append(&opt_opt);
    opt_hbox.append(&rec_gif_optimize_check);
    gif_frame.append(&build_row!(&opt_hbox, false));

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
    let gsize_hbox = GtkBox::new(Orientation::Horizontal, 12);
    gsize_hbox.set_hexpand(true);
    let gsize_label = Label::new(Some("GIF size"));
    gsize_label.set_xalign(0.0);
    gsize_label.set_hexpand(true);
    gsize_hbox.append(&gsize_label);
    gsize_hbox.append(&rec_gif_size_idx_input);
    gif_frame.append(&build_row!(&gsize_hbox, true));

    stack.add_named(&gif_frame, Some("gif"));

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

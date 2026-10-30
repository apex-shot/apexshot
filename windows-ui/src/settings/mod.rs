use crate::config::load_config;
use crate::i18n::{self, t};
use gtk4::{
    prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, FileChooserAction,
    FileChooserNative, Label, Orientation, Overlay as GtkOverlay, ResponseType, ScrolledWindow,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

#[path = "../../../src/settings/about.rs"]
mod about;
pub(crate) mod actions;
#[path = "../../../src/settings/advanced.rs"]
mod advanced;
#[path = "../../../src/settings/after_capture.rs"]
mod after_capture;
#[path = "../../../src/settings/annotate.rs"]
mod annotate;
mod autosave;
#[path = "../../../src/settings/cloud.rs"]
mod cloud;
mod cloud_login;
#[path = "../../../src/settings/general.rs"]
mod general;
#[path = "../../../src/settings/quick_access.rs"]
mod quick_access;
#[path = "../../../src/settings/recording.rs"]
mod recording;
#[path = "../../../src/settings/screenshots.rs"]
mod screenshots;
#[path = "../../../src/settings/select.rs"]
mod select;
pub(crate) mod shell;
#[path = "../../../src/settings/shortcuts.rs"]
mod shortcuts;
mod shortcuts_status;
#[path = "../../../src/settings/studio.rs"]
mod studio;
mod switches;
#[path = "../../../src/settings/theme_picker.rs"]
mod theme_picker;
#[path = "../../../src/settings/ui_support.rs"]
pub(crate) mod ui_support;
mod usage;
mod webcam;
pub(crate) mod windowing;

/// Top inset for page content. The Windows surface removes the page-title row,
/// so the first section needs only a hairline of breathing room.
pub(crate) const PAGE_TOP_MARGIN: i32 = if cfg!(target_os = "windows") { 4 } else { 20 };

use self::{
    actions::{install_checkbox_behaviors, SaveInputs},
    after_capture::build_after_capture_section,
    general::build_general_section,
    quick_access::build_quick_access_section,
    screenshots::build_screenshots_section,
    ui_support::{
        install_settings_css, window_control_button, WindowControl, CAPTION_BUTTON_WIDTH,
        TITLE_BAR_HEIGHT,
    },
    windowing::{prefers_dark_glass_theme, prefers_reduced_transparency},
};

/// The bundled outline glyph for the current theme.
fn themed_icon(dark: bool, dark_icon: &'static str, light_icon: &'static str) -> &'static str {
    if dark {
        dark_icon
    } else {
        light_icon
    }
}

fn page_title<'a>(titles: &'a [String], name: &str) -> Option<&'a str> {
    name.parse::<usize>()
        .ok()
        .and_then(|index| titles.get(index))
        .map(String::as_str)
}

/// A short page-level note for a feature the Windows shell does not run yet.
fn platform_note(text: &str) -> Label {
    let note = Label::new(Some(text));
    note.add_css_class("settings-sub-option-hint");
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_halign(Align::Start);
    note.set_margin_bottom(12);
    note
}

/// Short, user-facing summary of a save failure (full chain goes to stderr).
pub(crate) fn format_save_error(err: &anyhow::Error) -> String {
    const MAX_LEN: usize = 160;
    let root = err
        .chain()
        .last()
        .map(|e| e.to_string())
        .unwrap_or_else(|| err.to_string());
    let compact = root.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_LEN {
        compact
    } else {
        let truncated: String = compact.chars().take(MAX_LEN.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SettingsToastKind {
    Success,
    Error,
}

pub(crate) fn show_settings_toast(
    toast: &Label,
    generation: &Rc<Cell<u32>>,
    text: &str,
    kind: SettingsToastKind,
    auto_hide: Option<Duration>,
) {
    toast.remove_css_class("settings-toast-success");
    toast.remove_css_class("settings-toast-error");
    match kind {
        SettingsToastKind::Success => toast.add_css_class("settings-toast-success"),
        SettingsToastKind::Error => toast.add_css_class("settings-toast-error"),
    }
    toast.set_text(text);
    toast.set_opacity(1.0);
    toast.queue_draw();

    let gen = generation.get().wrapping_add(1);
    generation.set(gen);

    if let Some(delay) = auto_hide {
        let toast = toast.clone();
        let generation = Rc::clone(generation);
        gtk4::glib::timeout_add_local_once(delay, move || {
            if generation.get() == gen {
                toast.set_opacity(0.0);
                toast.remove_css_class("settings-toast-success");
                toast.remove_css_class("settings-toast-error");
            }
        });
    }
}

pub fn show_settings_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    app.connect_activate(|application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
        let windows = application.windows();
        if let Some(existing_window) = windows.first() {
            existing_window.present();
            return;
        }
        build_settings_window(application);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_settings_window(app: &Application) {
    crate::icons::install();

    install_settings_css();
    install_window_css();
    shell::install_css();

    let config = load_config().sanitized();
    if let Err(error) = actions::sync_shell_settings_on_start() {
        eprintln!("[settings] Could not synchronize the capture shell: {error:#}");
    }
    i18n::apply_gtk_direction(&config.ui_language);
    let prefers_dark = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();
    let ui_scale = crate::platform::ui_scale();
    // The shell draws its type and spacing at logical size: the window keeps
    // the roomy frame a scaled display earns without zooming the interface.
    let text_scale = ui_scale.min(1.0);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Settings"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(1240)
        .default_height(820)
        .build();

    // The app draws its own title bar, like the Linux window.
    window.set_decorated(false);
    window.add_css_class("editor-window");

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("editor-root");
    root_box.add_css_class("settings-shell");
    if !prefers_dark {
        root_box.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        root_box.add_css_class("editor-reduced-transparency");
    }

    let toast = Label::new(Some(&t("")));
    toast.add_css_class("settings-toast");
    toast.set_halign(Align::Center);
    toast.set_valign(Align::Start);
    toast.set_margin_top((52.0 * ui_scale).round() as i32);
    toast.set_opacity(0.0);
    toast.set_visible(true);
    toast.set_can_target(false);
    let toast_generation = Rc::new(Cell::new(0u32));

    let window_overlay = GtkOverlay::new();
    if !prefers_dark {
        window_overlay.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        window_overlay.add_css_class("editor-reduced-transparency");
    }
    window_overlay.set_child(Some(&root_box));
    window_overlay.add_overlay(&toast);

    // --- LAYOUT SPLIT ---
    let content_split = GtkBox::new(Orientation::Horizontal, 0);
    content_split.set_vexpand(true);
    content_split.set_hexpand(true);

    // The page stack is built before the sidebar: the sidebar's rows are
    // wired to it as they are created.
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vexpand(true);
    stack.add_css_class("settings-stack");

    use crate::capture::editor::window::icon_names::custom;
    let pages = [
        (
            t("General"),
            themed_icon(prefers_dark, custom::SETTINGS_2, custom::SETTINGS_2_LIGHT),
        ),
        (
            t("Shortcuts"),
            themed_icon(prefers_dark, custom::KEYBOARD, custom::KEYBOARD_LIGHT),
        ),
        (
            t("Quick Access"),
            themed_icon(
                prefers_dark,
                custom::PANELS_TOP_LEFT,
                custom::PANELS_TOP_LEFT_LIGHT,
            ),
        ),
        (
            t("Recording"),
            themed_icon(prefers_dark, custom::VIDEO, custom::VIDEO_LIGHT),
        ),
        (
            t("Screenshots"),
            themed_icon(prefers_dark, custom::CAMERA, custom::CAMERA_LIGHT),
        ),
        (
            t("Annotate"),
            themed_icon(prefers_dark, custom::PEN_TOOL, custom::PEN_TOOL_LIGHT),
        ),
        (
            t("Cloud"),
            themed_icon(prefers_dark, custom::CLOUD, custom::CLOUD_LIGHT),
        ),
        (
            t("Usage"),
            themed_icon(prefers_dark, custom::USAGE, custom::USAGE_LIGHT),
        ),
        (
            t("Advanced"),
            themed_icon(
                prefers_dark,
                custom::SLIDERS_HORIZONTAL,
                custom::SLIDERS_HORIZONTAL_LIGHT,
            ),
        ),
        (
            t("Studio"),
            themed_icon(prefers_dark, custom::STUDIO, custom::STUDIO_LIGHT),
        ),
        (
            t("About"),
            themed_icon(prefers_dark, custom::ABOUT, custom::ABOUT_LIGHT),
        ),
    ];
    // --- NAVIGATION (SIDEBAR) ---
    let sidebar = shell::build_sidebar(text_scale, TITLE_BAR_HEIGHT * ui_scale, &pages, &stack);
    let sidebar_head = sidebar.head.clone();

    let user_bar = shell::build_user_chip(&config, text_scale, {
        let stack = stack.clone();
        move || stack.set_visible_child_name("6")
    });
    sidebar.wrapper.append(&user_bar);

    content_split.append(&sidebar.wrapper);

    // --- BODY (STACK) ---
    let right_col = GtkBox::new(Orientation::Vertical, 0);
    right_col.set_vexpand(true);
    right_col.set_hexpand(true);

    // Custom title bar: a drag handle plus the same close/minimize pills as
    // the Linux window. The sidebar head beside it is draggable too.
    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("settings-window-controls");
    toolbar.set_size_request(-1, (TITLE_BAR_HEIGHT * ui_scale).round() as i32);

    let drag_handle = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle.set_hexpand(true);
    drag_handle.set_halign(Align::Fill);
    drag_handle.set_vexpand(false);
    toolbar.append(&drag_handle);

    let close_btn = window_control_button(WindowControl::Close, &t("Close"), ui_scale);
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let min_btn = window_control_button(WindowControl::Minimize, &t("Minimize"), ui_scale);
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    let right_box = GtkBox::new(Orientation::Horizontal, 0);
    right_box.set_halign(Align::End);
    right_box.set_valign(Align::Fill);
    right_box.append(&min_btn);
    right_box.append(&close_btn);
    toolbar.append(&right_box);

    windowing::install_window_drag(&drag_handle, &window);
    windowing::install_window_drag(&sidebar_head, &window);

    let general = build_general_section(&config);
    match windowing::autostart_entry_exists() {
        Ok(enabled) => general.start_at_login_check.set_active(enabled),
        Err(error) => {
            general.start_at_login_check.set_sensitive(false);
            general
                .start_at_login_check
                .set_tooltip_text(Some(&format!("Could not read Windows startup: {error}")));
        }
    }
    let after_capture = build_after_capture_section(&config);
    let recordings = recording::build_recording_section(&config);
    let webcam = webcam::build_webcam_group(&config);
    recordings.section.append(&webcam.0);
    let screenshots = build_screenshots_section(&config);
    let advanced = advanced::build_advanced_section(&config);
    let about = about::build_about_section();
    let quick_access = build_quick_access_section(&config);
    let shortcuts = shortcuts::build_shortcuts_section(&config);
    shortcuts::install_shortcut_editors(&shortcuts, &window);
    if std::env::var_os("APEXSHOT_DEBUG_SHORTCUT_DIALOG").is_some() {
        let first = shortcuts.open_file_btn.clone();
        gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            first.emit_clicked();
        });
    }
    let annotate = annotate::build_annotate_section(&config);
    let cloud = cloud::build_cloud_section(&config);
    let studio = studio::build_studio_section();

    shortcuts_status::install(&shortcuts);
    for button in [
        &shortcuts.open_file_btn,
        &shortcuts.open_clipboard_btn,
        &shortcuts.restore_file_btn,
        &shortcuts.toggle_overlays_btn,
    ] {
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&t(
            "This action requires the Windows image editor or overlay backend.",
        )));
    }
    annotate.section.prepend(&platform_note(&t(
        "The annotate editor is not available in the Windows build yet. These preferences are saved for a future release.",
    )));
    cloud.section.prepend(&platform_note(&t(
        "Cloud uploads are not wired in the Windows build yet. Destination and XBackBone settings are saved for a future release.",
    )));
    let (cloud_page, cloud_login) = cloud_login::wrap_cloud_page(&cloud.section);
    cloud
        .connect_btn
        .connect_clicked(move |_| cloud_login.begin());
    let usage = usage::build({
        let stack = stack.clone();
        move || stack.set_visible_child_name("6")
    });

    // Controls that have no Windows implementation yet stay visible but
    // disabled, so the page still documents the setting.
    after_capture.screenshot_after_capture_checks[3].set_sensitive(false);
    after_capture.screenshot_after_capture_checks[3].set_tooltip_text(Some(&t(
        "The annotate editor is not available on Windows yet.",
    )));
    after_capture.rec_save.set_tooltip_text(Some(&t(
        "When off, recordings remain in the temporary folder for the result card and editor.",
    )));
    general.theme_input.widget().set_tooltip_text(Some(&t(
        "The capture shell currently uses its own dark appearance.",
    )));
    general.show_icon_check.set_sensitive(false);
    general.show_icon_check.set_tooltip_text(Some(&t(
        "The Windows shell uses its tray icon to open capture, Settings, and Quit.",
    )));
    general.ui_language_input.widget().set_sensitive(false);
    general
        .ui_language_input
        .widget()
        .set_tooltip_text(Some(&t("Windows UI translations are not available yet.")));
    screenshots
        .timer_interval_input
        .widget()
        .set_tooltip_text(Some(&t(
            "The capture toolbar can change the timer for an individual screenshot.",
        )));
    recordings.rec_controls.set_tooltip_text(Some(&t(
        "When hidden, stop the recording from the tray or with its shortcut.",
    )));
    recordings.rec_hidpi.set_tooltip_text(Some(&t(
        "When off, output uses the capture's display scale before applying the maximum resolution.",
    )));
    recordings.rec_notifications.set_active(false);
    recordings.rec_notifications.set_sensitive(false);
    recordings.rec_notifications.set_tooltip_text(Some(&t(
        "Do Not Disturb integration is not available on Windows yet.",
    )));
    quick_access.close_after_dragging_check.set_sensitive(false);
    quick_access
        .close_after_dragging_check
        .set_tooltip_text(Some(&t(
            "Dragging the result card out of the app is not available on Windows yet.",
        )));
    quick_access
        .close_after_uploading_check
        .set_sensitive(false);
    quick_access
        .close_after_uploading_check
        .set_tooltip_text(Some(&t(
            "Uploading from the result card is not available on Windows yet.",
        )));
    advanced
        .telemetry_enabled_check
        .set_active(config.windows_telemetry_enabled);
    advanced.telemetry_hint.set_text(&t(
        "Optional daily heartbeat: app version, Windows, and feature counts. No screenshots, paths, or OCR text. Override with APEXSHOT_TELEMETRY=0.",
    ));
    advanced.telemetry_enabled_check.set_tooltip_text(Some(&t(
        "Off by default. Turning this on sends anonymous usage statistics.",
    )));
    advanced.ocr_lang_input.widget().set_tooltip_text(Some(&t(
        "Requires the corresponding Windows OCR language pack.",
    )));

    let screenshot_export_location_entry_pick = screenshots.export_location_entry.clone();
    let window_weak_picker = window.downgrade();
    screenshots
        .export_location_browse
        .connect_clicked(move |_| {
            let chooser = FileChooserNative::new(
                Some(&t("Select screenshot save location")),
                window_weak_picker.upgrade().as_ref(),
                FileChooserAction::SelectFolder,
                Some(&t("Select")),
                Some(&t("Cancel")),
            );
            let export_location_entry_pick = screenshot_export_location_entry_pick.clone();
            chooser.connect_response(move |dialog, response| {
                if response == ResponseType::Accept {
                    if let Some(file) = dialog.file() {
                        if let Some(path) = file.path() {
                            export_location_entry_pick.set_text(&path.to_string_lossy());
                        }
                    }
                }
                dialog.hide();
            });
            chooser.show();
        });

    let video_export_location_entry_pick = recordings.video_export_location_entry.clone();
    let window_weak_picker = window.downgrade();
    recordings
        .video_export_location_browse
        .connect_clicked(move |_| {
            let chooser = FileChooserNative::new(
                Some(&t("Select video save location")),
                window_weak_picker.upgrade().as_ref(),
                FileChooserAction::SelectFolder,
                Some(&t("Select")),
                Some(&t("Cancel")),
            );
            let export_location_entry_pick = video_export_location_entry_pick.clone();
            chooser.connect_response(move |dialog, response| {
                if response == ResponseType::Accept {
                    if let Some(file) = dialog.file() {
                        if let Some(path) = file.path() {
                            export_location_entry_pick.set_text(&path.to_string_lossy());
                        }
                    }
                }
                dialog.hide();
            });
            chooser.show();
        });

    let general_tab_section = GtkBox::new(Orientation::Vertical, 0);
    general_tab_section.append(&general.section);
    general_tab_section.append(&after_capture.wrapper);

    fn add_section(
        stack: &gtk4::Stack,
        widget: &impl IsA<gtk4::Widget>,
        name: &str,
        title: &str,
        scale: f64,
    ) {
        let scroller = ScrolledWindow::new();
        scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        let pad = (20.0 * scale).round() as i32;
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.set_margin_top((4.0 * scale).round() as i32);
        vbox.set_margin_bottom((24.0 * scale).round() as i32);
        vbox.set_margin_start(pad);
        vbox.set_margin_end(pad);
        vbox.append(widget);
        scroller.set_child(Some(&vbox));
        let adjustment = scroller.vadjustment();
        adjustment.set_value(adjustment.lower());
        stack.add_titled(&scroller, Some(name), title);
    }

    add_section(&stack, &general_tab_section, "0", &t("General"), text_scale);
    add_section(&stack, &shortcuts.section, "1", &t("Shortcuts"), text_scale);
    add_section(
        &stack,
        &quick_access.section,
        "2",
        &t("Quick Access"),
        ui_scale,
    );
    add_section(
        &stack,
        &recordings.section,
        "3",
        &t("Recording"),
        text_scale,
    );
    add_section(
        &stack,
        &screenshots.section,
        "4",
        &t("Screenshots"),
        ui_scale,
    );
    add_section(&stack, &annotate.section, "5", &t("Annotate"), text_scale);
    add_section(&stack, &cloud_page, "6", &t("Cloud"), text_scale);
    add_section(&stack, &usage, "7", &t("Usage"), text_scale);
    add_section(&stack, &advanced.section, "8", &t("Advanced"), text_scale);
    add_section(&stack, &studio, "9", &t("Studio"), text_scale);
    add_section(&stack, &about.section, "10", &t("About"), text_scale);

    stack.set_visible_child_name("0");

    // Development aid: open a specific page without driving the sidebar.
    if let Ok(page) = std::env::var("APEXSHOT_SETTINGS_PAGE") {
        let index = match page.trim().to_ascii_lowercase().as_str() {
            "general" => 0,
            "shortcuts" => 1,
            "quick" | "quick-access" => 2,
            "recording" => 3,
            "screenshots" => 4,
            "annotate" => 5,
            "cloud" => 6,
            "usage" => 7,
            "advanced" => 8,
            "studio" => 9,
            "about" => 10,
            other => other.parse().unwrap_or(0),
        };
        stack.set_visible_child_name(&index.to_string());
    }

    let page_titles: Vec<String> = pages.iter().map(|(title, _)| title.clone()).collect();
    let initial_title = stack
        .visible_child_name()
        .and_then(|name| page_title(&page_titles, &name).map(str::to_owned))
        .unwrap_or_else(|| t("General"));
    let page_heading = Label::new(Some(&initial_title));
    page_heading.add_css_class("settings-page-title");
    page_heading.set_xalign(0.0);
    page_heading.set_halign(Align::Start);
    page_heading.set_margin_start(22);
    page_heading.set_margin_top(22);
    page_heading.set_margin_bottom(2);

    {
        let page_heading = page_heading.clone();
        stack.connect_visible_child_name_notify(move |stack| {
            let Some(name) = stack.visible_child_name() else {
                return;
            };
            if let Some(title) = page_title(&page_titles, &name) {
                page_heading.set_text(title);
            }
        });
    }

    right_col.append(&toolbar);
    right_col.append(&shell::build_content_card(
        &stack,
        text_scale,
        &page_heading,
    ));
    content_split.append(&right_col);
    root_box.append(&content_split);
    window.set_child(Some(&window_overlay));

    // --- SAVE LOGIC ---
    let save_inputs = Rc::new(SaveInputs {
        start_at_login: general.start_at_login_check.clone(),
        play_sounds: general.play_sounds_check.clone(),
        shutter_sound: general.shutter_sound_input.clone(),
        show_menu_bar_icon: general.show_icon_check.clone(),
        screenshot_export_location: screenshots.export_location_entry.clone(),
        screenshot_format: screenshots.format_input.clone(),
        video_export_location: recordings.video_export_location_entry.clone(),
        rec_filename_pattern: recordings.rec_filename_pattern_entry.clone(),
        rec_remember_export_folder: recordings.rec_remember_export_folder.clone(),
        rec_controls: recordings.rec_controls.clone(),
        rec_hidpi: recordings.rec_hidpi.clone(),
        rec_notifications: recordings.rec_notifications.clone(),
        rec_countdown: recordings.rec_countdown.clone(),
        rec_video_max_res: recordings.rec_video_max_res.clone(),
        rec_video_fps: recordings.rec_video_fps.clone(),
        rec_video_quality: recordings.rec_video_quality.clone(),
        rec_video_mono: recordings.rec_video_mono.clone(),
        webcam: webcam.1,
        screenshot_quick_access: after_capture.screenshot_after_capture_checks[0].clone(),
        screenshot_copy_to_clipboard: after_capture.screenshot_after_capture_checks[1].clone(),
        screenshot_save: after_capture.screenshot_after_capture_checks[2].clone(),
        screenshot_open_annotate: after_capture.screenshot_after_capture_checks[3].clone(),
        rec_copy_to_clipboard: after_capture.rec_copy_to_clipboard.clone(),
        rec_save: after_capture.rec_save.clone(),
        rec_open_video_editor: after_capture.rec_open_video_editor.clone(),
        quick_access_position: quick_access.position_input.clone(),
        quick_access_overlay_size: quick_access.overlay_size_input.clone(),
        quick_access_auto_close_enabled: quick_access.auto_close_enabled_check.clone(),
        quick_access_auto_close_action: quick_access.auto_close_action_input.clone(),
        quick_access_auto_close_interval: quick_access.auto_close_interval_input.clone(),
        quick_access_close_after_dragging: quick_access.close_after_dragging_check.clone(),
        quick_access_close_after_uploading: quick_access.close_after_uploading_check.clone(),
        screenshot_timer_interval: screenshots.timer_interval_input.clone(),
        screenshot_capture_cursor: screenshots.show_cursor_check.clone(),
        annotate_inverse_arrow: annotate.inverse_arrow_check.clone(),
        annotate_smooth_drawing: annotate.smooth_drawing_check.clone(),
        annotate_draw_shadow: annotate.draw_shadow_check.clone(),
        annotate_auto_expand: annotate.auto_expand_check.clone(),
        annotate_show_color_names: annotate.show_color_names_check.clone(),
        annotate_always_on_top: annotate.always_on_top_check.clone(),
        annotate_close_after_export: annotate.close_after_export_check.clone(),
        annotate_overwrite_original_image: annotate.overwrite_original_image_check.clone(),
        shortcut_open_file: shortcuts.open_file_btn.clone(),
        shortcut_open_from_clipboard: shortcuts.open_clipboard_btn.clone(),
        shortcut_restore_recently_closed: shortcuts.restore_file_btn.clone(),
        shortcut_toggle_overlays: shortcuts.toggle_overlays_btn.clone(),
        shortcut_capture_menu: shortcuts.quick_capture_btn.clone(),
        shortcut_capture_area: shortcuts.capture_area_btn.clone(),
        shortcut_capture_crosshair: shortcuts.capture_crosshair_btn.clone(),
        shortcut_capture_previous_area: shortcuts.capture_prev_btn.clone(),
        shortcut_capture_fullscreen: shortcuts.capture_fullscreen_btn.clone(),
        shortcut_capture_window: shortcuts.capture_window_btn.clone(),
        shortcut_show_last_preview: shortcuts.show_last_preview_btn.clone(),
        shortcut_save_last_capture: shortcuts.save_last_capture_btn.clone(),
        shortcut_record_screen: shortcuts.record_screen_btn.clone(),
        shortcut_recording_stop_save: shortcuts.recording_stop_save_btn.clone(),
        adv_retina_suffix: advanced.retina_suffix_check.clone(),
        adv_clipboard_mode: screenshots.clipboard_mode_input.clone(),
        adv_ocr_language: advanced.ocr_lang_input.clone(),
        adv_ocr_keep_line_breaks: advanced.ocr_line_breaks_check.clone(),
        telemetry_enabled: advanced.telemetry_enabled_check.clone(),
        cloud_apexshot: cloud.apexshot_check.clone(),
        cloud_xbackbone: cloud.xbackbone_check.clone(),
        cloud_auto_upload: cloud.auto_upload_check.clone(),
        xbackbone_url: cloud.xb_url_entry.clone(),
        xbackbone_api_token: cloud.xb_token_entry.clone(),
        ui_theme: general.theme_input.clone(),
        ui_language: general.ui_language_input.clone(),
    });

    // --- AUTO-SAVE ---
    // No Save button: every edit is written a moment after the person stops
    // making it, including the last one when the window closes.
    let saver = autosave::AutoSaver::new(
        window.clone(),
        Rc::clone(&save_inputs),
        toast.clone(),
        Rc::clone(&toast_generation),
    );
    install_save_dirty_tracking(&save_inputs, saver.mark_dirty());
    switches::install(&root_box);

    let saver_for_close = Rc::clone(&saver);
    window.connect_close_request(move |_| {
        saver_for_close.flush();
        gtk4::glib::Propagation::Proceed
    });

    install_checkbox_behaviors(
        &general.play_sounds_check,
        &general.shutter_sound_input,
        &after_capture.screenshot_after_capture_checks[0],
        &after_capture.screenshot_after_capture_checks[1],
        &after_capture.screenshot_after_capture_checks[2],
        &after_capture.screenshot_after_capture_checks[3],
        &quick_access.auto_close_enabled_check,
        &quick_access.auto_close_action_input,
        &quick_access.auto_close_interval_input,
    );

    window.present();
}

/// Rebuild the window in place after a change the widget tree cannot re-apply:
/// the light/dark theme and the reading direction both need fresh widgets.
pub(crate) fn rebuild_settings_window(window: &ApplicationWindow) {
    if let Some(app) = window.application() {
        window.close();
        build_settings_window(&app);
    }
}

pub(crate) fn install_window_css() {
    use gtk4::CssProvider;
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };

    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round() as i32;
    let mut css = format!(
        "window.editor-window {{ background-color:transparent; border:none; box-shadow:none; }} \
         .editor-root {{ border:1px solid rgba(255,255,255,0.12); border-radius:12px; font-size:{}px; }} \
         .editor-root.editor-theme-light {{ border:1px solid rgba(17,24,39,0.12); }} \
         .settings-sidebar-wrapper {{ border-radius:12px 0 0 12px; }} \
         .settings-window-controls {{ border-bottom:none; border-radius:0 12px 0 0; }} \
         .settings-nav-label {{ font-size:{}px; }} \
         .settings-nav-item {{ min-height:{}px; padding:{}px 10px; }} \
         .settings-group-title {{ font-size:{}px; margin-top:{}px; margin-bottom:{}px; }} \
         .settings-sub-option {{ font-size:{}px; }} \
         .settings-sub-option-hint {{ font-size:{}px; }} \
         .settings-table-header {{ font-size:{}px; }} \
         .settings-table-row {{ padding:{}px 14px; }} \
         .settings-sidebar-title {{ font-size:{}px; }} \
         button {{ min-height:{}px; }} \
         .settings-save-btn {{ font-size:{}px; min-height:{}px; }} \
         .settings-primary-btn {{ font-size:{}px; min-height:{}px; }} \
         button.settings-select {{ min-height:{}px; padding:{}px 10px; }} \
         button.settings-select label {{ font-size:{}px; }}",
        px(13.0),
        px(13.0),
        px(20.0),
        px(3.0),
        px(11.0),
        px(4.0),
        px(4.0),
        px(12.5),
        px(12.0),
        px(13.0),
        px(9.0),
        px(17.0),
        px(22.0),
        px(13.0),
        px(22.0),
        px(13.0),
        px(22.0),
        px(26.0),
        px(1.0),
        px(12.0),
    );
    css.push_str(&format!(
        " window.editor-window.history-window {{ padding:1px; border:1px solid rgba(255,255,255,0.12); border-radius:12px; }} \
         window.editor-window.history-window.editor-theme-light {{ border:1px solid rgba(17,24,39,0.12); }} \
         .history-root {{ border:none; border-radius:12px; font-family:'Inter','Noto Sans',system-ui,sans-serif; }} \
         .history-root .history-header-search {{ font-size:{}px; min-height:{}px; }} \
         .history-root .history-header-refresh {{ min-width:{}px; min-height:{}px; margin-right:{}px; }} \
         .history-root .recent-captures-title {{ font-size:{}px; font-weight:600; letter-spacing:-0.1px; opacity:0.85; }} \
         .history-root .history-page-subtitle {{ font-size:{}px; }} \
         .history-root .recent-captures-card-title {{ font-size:{}px; font-weight:500; }} \
         .history-root .recent-captures-empty-title {{ font-size:{}px; }} \
         .history-root .recent-captures-empty-detail {{ font-size:{}px; }}",
        px(13.0),
        px(26.0),
        px(26.0),
        px(26.0),
        px(4.0),
        px(14.0),
        px(13.0),
        px(13.0),
        px(18.0),
        px(14.0),
    ));

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Caption buttons: the flat, full-height targets the system draws in a
    // title bar's top-right corner, packed against each other at the bar's
    // height. Installed last so the editor sheets' pill styling cannot win.
    let caption_css = format!(
        " .settings-window-controls {{ min-height:{}px; padding:0; }} \
         button.caption-btn {{ \
             min-width:{}px; min-height:{}px; padding:0; margin:0; \
             border:none; border-radius:0; box-shadow:none; \
             background:transparent; background-image:none; \
             color:alpha(white, 0.90); }} \
         button.caption-btn:hover {{ \
             background:rgba(255,255,255,0.08); background-image:none; \
             border:none; color:#ffffff; }} \
         button.caption-btn:active {{ \
             background:rgba(255,255,255,0.12); background-image:none; \
             border:none; color:#ffffff; }} \
         button.caption-btn:focus, button.caption-btn:hover:focus {{ \
             background-image:none; border:none; box-shadow:none; outline:none; }} \
         button.caption-btn-close {{ border-radius:0 12px 0 0; }} \
         button.caption-btn-close:hover, button.caption-btn-close:hover:focus {{ \
             background:#e81123; border-radius:0 12px 0 0; color:#ffffff; }} \
         button.caption-btn-close:active {{ \
             background:#c50f1f; border-radius:0 12px 0 0; color:#ffffff; }} \
         .editor-theme-light button.caption-btn {{ color:alpha(#111827, 0.80); }} \
         .editor-theme-light button.caption-btn:hover {{ \
             background:alpha(#111827, 0.10); color:alpha(#111827, 0.92); }} \
         .editor-theme-light button.caption-btn:active {{ \
             background:alpha(#111827, 0.14); color:alpha(#111827, 0.92); }} \
         .editor-theme-light button.caption-btn-close:hover, \
         .editor-theme-light button.caption-btn-close:hover:focus, \
         .editor-theme-light button.caption-btn-close:active {{ \
             background:#e81123; color:#ffffff; }}",
        px(TITLE_BAR_HEIGHT),
        px(CAPTION_BUTTON_WIDTH),
        px(TITLE_BAR_HEIGHT),
    );
    let caption_provider = CssProvider::new();
    caption_provider.load_from_data(&caption_css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &caption_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 8,
    );
}

/// Wire every settings control so any edit reaches the auto-saver.
fn install_save_dirty_tracking(inputs: &Rc<SaveInputs>, mark_dirty: Rc<dyn Fn()>) {
    let wire_check = |check: &gtk4::CheckButton| {
        let mark_dirty = Rc::clone(&mark_dirty);
        check.connect_toggled(move |_| mark_dirty());
    };
    let wire_combo = |combo: &select::SettingsSelect| {
        let mark_dirty = Rc::clone(&mark_dirty);
        combo.connect_changed(move || mark_dirty());
    };
    let wire_entry = |entry: &gtk4::Entry| {
        let mark_dirty = Rc::clone(&mark_dirty);
        entry.connect_changed(move |_| mark_dirty());
    };
    let wire_scale = |scale: &gtk4::Scale| {
        let mark_dirty = Rc::clone(&mark_dirty);
        scale.connect_value_changed(move |_| mark_dirty());
    };
    let wire_theme = |picker: &theme_picker::ThemePicker| {
        let mark_dirty = Rc::clone(&mark_dirty);
        picker.connect_changed(move || mark_dirty());
    };
    let wire_shortcut_btn = |button: &Button| {
        let mark_dirty = Rc::clone(&mark_dirty);
        button.connect_notify_local(Some("label"), move |_, _| mark_dirty());
    };

    wire_check(&inputs.start_at_login);
    wire_check(&inputs.play_sounds);
    wire_combo(&inputs.shutter_sound);
    wire_combo(&inputs.ui_language);
    wire_check(&inputs.show_menu_bar_icon);
    wire_entry(&inputs.screenshot_export_location);
    wire_combo(&inputs.screenshot_format);
    wire_entry(&inputs.video_export_location);
    wire_entry(&inputs.rec_filename_pattern);
    wire_check(&inputs.rec_remember_export_folder);
    wire_check(&inputs.rec_controls);
    wire_check(&inputs.rec_hidpi);
    wire_check(&inputs.rec_notifications);
    wire_check(&inputs.rec_countdown);
    wire_combo(&inputs.rec_video_max_res);
    wire_combo(&inputs.rec_video_fps);
    wire_combo(&inputs.rec_video_quality);
    wire_check(&inputs.rec_video_mono);
    inputs.webcam.install_dirty_tracking(Rc::clone(&mark_dirty));
    wire_check(&inputs.screenshot_quick_access);
    wire_check(&inputs.screenshot_copy_to_clipboard);
    wire_check(&inputs.screenshot_save);
    wire_check(&inputs.screenshot_open_annotate);
    wire_check(&inputs.rec_copy_to_clipboard);
    wire_check(&inputs.rec_save);
    wire_check(&inputs.rec_open_video_editor);
    wire_combo(&inputs.quick_access_position);
    wire_scale(&inputs.quick_access_overlay_size);
    wire_check(&inputs.quick_access_auto_close_enabled);
    wire_combo(&inputs.quick_access_auto_close_action);
    wire_combo(&inputs.quick_access_auto_close_interval);
    wire_check(&inputs.quick_access_close_after_dragging);
    wire_check(&inputs.quick_access_close_after_uploading);
    wire_combo(&inputs.screenshot_timer_interval);
    wire_check(&inputs.screenshot_capture_cursor);
    wire_check(&inputs.annotate_inverse_arrow);
    wire_check(&inputs.annotate_smooth_drawing);
    wire_check(&inputs.annotate_draw_shadow);
    wire_check(&inputs.annotate_auto_expand);
    wire_check(&inputs.annotate_show_color_names);
    wire_check(&inputs.annotate_always_on_top);
    wire_check(&inputs.annotate_close_after_export);
    wire_check(&inputs.annotate_overwrite_original_image);
    wire_shortcut_btn(&inputs.shortcut_open_file);
    wire_shortcut_btn(&inputs.shortcut_open_from_clipboard);
    wire_shortcut_btn(&inputs.shortcut_restore_recently_closed);
    wire_shortcut_btn(&inputs.shortcut_toggle_overlays);
    wire_shortcut_btn(&inputs.shortcut_capture_menu);
    wire_shortcut_btn(&inputs.shortcut_capture_area);
    wire_shortcut_btn(&inputs.shortcut_capture_crosshair);
    wire_shortcut_btn(&inputs.shortcut_capture_previous_area);
    wire_shortcut_btn(&inputs.shortcut_capture_fullscreen);
    wire_shortcut_btn(&inputs.shortcut_capture_window);
    wire_shortcut_btn(&inputs.shortcut_show_last_preview);
    wire_shortcut_btn(&inputs.shortcut_save_last_capture);
    wire_shortcut_btn(&inputs.shortcut_record_screen);
    wire_shortcut_btn(&inputs.shortcut_recording_stop_save);
    wire_check(&inputs.adv_retina_suffix);
    wire_combo(&inputs.adv_clipboard_mode);
    wire_combo(&inputs.adv_ocr_language);
    wire_check(&inputs.adv_ocr_keep_line_breaks);
    wire_check(&inputs.telemetry_enabled);
    wire_check(&inputs.cloud_apexshot);
    wire_check(&inputs.cloud_xbackbone);
    wire_check(&inputs.cloud_auto_upload);
    wire_entry(&inputs.xbackbone_url);
    wire_entry(&inputs.xbackbone_api_token);
    wire_theme(&inputs.ui_theme);
}

#[cfg(test)]
mod tests {
    use super::page_title;

    #[test]
    fn selected_page_title_matches_the_stack_name() {
        let titles = vec!["General".to_string(), "Cloud".to_string()];
        assert_eq!(page_title(&titles, "0"), Some("General"));
        assert_eq!(page_title(&titles, "1"), Some("Cloud"));
        assert_eq!(page_title(&titles, "2"), None);
        assert_eq!(page_title(&titles, "invalid"), None);
    }
}

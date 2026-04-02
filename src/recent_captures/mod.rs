mod discovery;

pub use discovery::{RecentCaptureCollection, RecentCaptureEntry};

use anyhow::Result;
use chrono::{DateTime, Local};
use discovery::{discover_recent_captures, recent_capture_source_dir};
use gtk4::{
    gdk, gio, glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Grid,
    Label, Orientation, PolicyType, ScrolledWindow, ToggleButton,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::SystemTime;

use crate::capture::show_capture_preview_overlay;
use crate::settings::{
    ui_support::{install_settings_css, traffic_light_button},
    windowing::{
        install_edge_resize, install_window_drag, prefers_dark_glass_theme,
        prefers_reduced_transparency,
    },
};

const HERO_IMAGE_WIDTH: i32 = 640;
const HERO_IMAGE_HEIGHT: i32 = 380;
const CARD_IMAGE_WIDTH: i32 = 280;
const CARD_IMAGE_HEIGHT: i32 = 175;

pub fn show_recent_captures_window() -> Result<()> {
    let app = Application::builder()
        .application_id("com.apexshot.recentcaptures")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(build_recent_captures_window);
    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_recent_captures_window(app: &Application) {
    install_settings_css();

    let prefers_dark = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recent Captures")
        .default_width(1080)
        .default_height(860)
        .build();

    window.set_decorated(false);
    window.add_css_class("editor-window");

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("editor-root");
    root_box.add_css_class("recent-captures-root");
    if !prefers_dark {
        root_box.add_css_class("editor-theme-light");
    }
    if reduced_transparency {
        root_box.add_css_class("editor-reduced-transparency");
    }

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("editor-toolbar");
    toolbar.add_css_class("recent-captures-toolbar");

    let left_box = GtkBox::new(Orientation::Horizontal, 4);
    left_box.set_halign(Align::Start);
    left_box.set_valign(Align::Center);
    left_box.add_css_class("editor-toolbar-left");
    left_box.set_margin_start(8);

    let close_btn = traffic_light_button("traffic-light-red", "Close");
    let window_close = window.clone();
    close_btn.connect_clicked(move |_| window_close.close());

    let min_btn = traffic_light_button("traffic-light-yellow", "Minimize");
    let window_min = window.clone();
    min_btn.connect_clicked(move |_| window_min.minimize());

    let max_btn = traffic_light_button("traffic-light-green", "Maximize");
    let window_max = window.clone();
    max_btn.connect_clicked(move |_| {
        if window_max.is_maximized() {
            window_max.unmaximize();
        } else {
            window_max.maximize();
        }
    });

    left_box.append(&close_btn);
    left_box.append(&min_btn);
    left_box.append(&max_btn);
    toolbar.append(&left_box);

    let filter_state = Rc::new(std::cell::Cell::new(
        discovery::CaptureModeFilter::Screenshots,
    ));

    let drag_handle_left = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle_left.set_hexpand(true);
    toolbar.append(&drag_handle_left);

    let segmented_box = GtkBox::new(Orientation::Horizontal, 0);
    segmented_box.add_css_class("recent-captures-segmented-control");
    segmented_box.set_halign(Align::Center);

    let btn_screens = ToggleButton::with_label("Screenshots");
    let btn_records = ToggleButton::with_label("Recordings");

    btn_records.set_group(Some(&btn_screens));

    btn_screens.add_css_class("recent-captures-segmented-btn");
    btn_records.add_css_class("recent-captures-segmented-btn");

    segmented_box.append(&btn_screens);
    segmented_box.append(&btn_records);

    btn_screens.set_active(true);
    toolbar.append(&segmented_box);

    let drag_handle_right = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle_right.set_hexpand(true);
    toolbar.append(&drag_handle_right);

    let right_box = GtkBox::new(Orientation::Horizontal, 4);
    right_box.set_halign(Align::End);
    right_box.set_valign(Align::Center);
    right_box.add_css_class("editor-toolbar-right");

    let columns_state = Rc::new(std::cell::Cell::new(2u32));

    let list_btn = Button::builder()
        .icon_name("view-list-symbolic")
        .has_frame(false)
        .tooltip_text("View as List")
        .build();
    list_btn.add_css_class("recent-captures-icon-btn");

    let grid_btn = Button::builder()
        .icon_name("view-grid-symbolic")
        .has_frame(false)
        .tooltip_text("View as Grid")
        .build();
    grid_btn.add_css_class("recent-captures-icon-btn");

    let refresh_btn = Button::builder()
        .icon_name("view-refresh-symbolic")
        .has_frame(false)
        .tooltip_text("Refresh")
        .build();
    refresh_btn.add_css_class("recent-captures-icon-btn");

    right_box.append(&list_btn);
    right_box.append(&grid_btn);
    right_box.append(&refresh_btn);
    toolbar.append(&right_box);

    root_box.append(&toolbar);

    install_window_drag(&drag_handle_left, &window);
    install_window_drag(&drag_handle_right, &window);
    install_edge_resize(&root_box, &window);

    let scroller = ScrolledWindow::new();
    scroller.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 40);
    content.add_css_class("recent-captures-shell");
    content.set_margin_top(48);
    content.set_margin_bottom(64);
    content.set_margin_start(56);
    content.set_margin_end(56);
    scroller.set_child(Some(&content));
    root_box.append(&scroller);

    let status_bar = GtkBox::new(Orientation::Horizontal, 0);
    status_bar.add_css_class("recent-captures-statusbar");
    let status_label = Label::new(Some("Loading..."));
    status_label.add_css_class("recent-captures-toolbar-status");
    status_label.set_halign(Align::Start);
    status_label.set_margin_start(16);
    status_label.set_margin_end(16);
    status_bar.append(&status_label);
    root_box.append(&status_bar);

    window.set_child(Some(&root_box));
    window.present();

    let content_rc = Rc::new(content);
    let status_rc = Rc::new(status_label);
    let cols_rc = Rc::clone(&columns_state);
    let filter_rc = Rc::clone(&filter_state);

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        list_btn.connect_clicked(move |_| {
            columns.set(1);
            load_recent_capture_content(&content, &status, columns.get(), filter.get());
        });
    }

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        grid_btn.connect_clicked(move |_| {
            columns.set(2);
            load_recent_capture_content(&content, &status, columns.get(), filter.get());
        });
    }

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        refresh_btn.connect_clicked(move |_| {
            load_recent_capture_content(&content, &status, columns.get(), filter.get());
        });
    }

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        btn_screens.connect_toggled(move |btn| {
            if btn.is_active() {
                filter.set(discovery::CaptureModeFilter::Screenshots);
                load_recent_capture_content(&content, &status, columns.get(), filter.get());
            }
        });
    }

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        btn_records.connect_toggled(move |btn| {
            if btn.is_active() {
                filter.set(discovery::CaptureModeFilter::Recordings);
                load_recent_capture_content(&content, &status, columns.get(), filter.get());
            }
        });
    }

    {
        let content = Rc::clone(&content_rc);
        let status = Rc::clone(&status_rc);
        let columns = Rc::clone(&cols_rc);
        let filter = Rc::clone(&filter_rc);
        glib::idle_add_local_once(move || {
            load_recent_capture_content(&content, &status, columns.get(), filter.get());
        });
    }
}

fn load_recent_capture_content(
    content: &GtkBox,
    status_label: &Label,
    columns: u32,
    filter: discovery::CaptureModeFilter,
) {
    clear_box(content);

    let source_dir = recent_capture_source_dir();
    let collection = discover_recent_captures(filter);

    let header = build_header(source_dir.as_ref(), collection.featured.is_some(), filter);
    content.append(&header);

    let collection_name = match filter {
        discovery::CaptureModeFilter::Screenshots | discovery::CaptureModeFilter::All => {
            "screenshot"
        }
        discovery::CaptureModeFilter::Recordings => "recording",
    };

    if let Some(featured) = &collection.featured {
        status_label.set_text(&format!(
            "{} recent {}{}",
            1 + collection.remaining.len(),
            collection_name,
            if collection.remaining.is_empty() {
                ""
            } else {
                "s"
            }
        ));
        let hero = build_featured_section(featured, status_label);
        content.append(&hero);

        if !collection.remaining.is_empty() {
            let grid = build_recent_grid(&collection.remaining, status_label, columns);
            content.append(&grid);
        }
    } else {
        status_label.set_text(&format!("No {}s found yet", collection_name));
        let empty_state = build_empty_state(source_dir.as_ref(), filter);
        content.append(&empty_state);
    }
}

fn build_header(
    source_dir: Option<&PathBuf>,
    has_results: bool,
    filter: discovery::CaptureModeFilter,
) -> GtkBox {
    let header = GtkBox::new(Orientation::Vertical, 10);
    header.add_css_class("recent-captures-header");

    let title = Label::new(Some("Recent Captures"));
    title.add_css_class("recent-captures-title");
    title.set_wrap(true);
    title.set_halign(Align::Start);
    title.set_xalign(0.0);

    let subtitle_text = match (source_dir, has_results) {
        (Some(dir), true) => format!("Browsing {}", dir.display()),
        (Some(dir), false) => {
            let noun = match filter {
                discovery::CaptureModeFilter::Screenshots | discovery::CaptureModeFilter::All => {
                    "screenshots"
                }
                discovery::CaptureModeFilter::Recordings => "recordings",
            };
            format!("Watching {} for saved {}", dir.display(), noun)
        }
        (None, _) => "Capture save location is unavailable on this system".to_string(),
    };

    let subtitle = Label::new(Some(&subtitle_text));
    subtitle.add_css_class("recent-captures-subtitle");
    subtitle.set_wrap(true);
    subtitle.set_halign(Align::Start);
    subtitle.set_xalign(0.0);

    header.append(&title);
    header.append(&subtitle);
    header
}

fn build_featured_section(entry: &RecentCaptureEntry, status_label: &Label) -> GtkBox {
    let section = GtkBox::new(Orientation::Horizontal, 32);
    section.add_css_class("recent-captures-hero");

    let thumbnail = build_thumbnail(
        entry.path.clone(),
        HERO_IMAGE_WIDTH,
        HERO_IMAGE_HEIGHT,
        true,
    );

    let hero_image = GtkBox::new(Orientation::Vertical, 0);
    hero_image.add_css_class("recent-captures-hero-image");
    hero_image.append(&thumbnail);

    let gesture = gtk4::GestureClick::new();
    let path_clone = entry.path.clone();
    let status_clone = status_label.clone();
    gesture.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            if is_animated_media(&path_clone) {
                status_clone.set_text("Opened Video Editor (Coming Soon)");
            } else {
                match show_capture_preview_overlay(path_clone.clone()) {
                    Ok(_) => status_clone.set_text("Floating overlay launched"),
                    Err(e) => status_clone.set_text(&format!("Failed to launch overlay: {}", e)),
                }
            }
        }
    });
    hero_image.add_controller(gesture);

    let meta = GtkBox::new(Orientation::Vertical, 12);
    meta.add_css_class("recent-captures-hero-meta");
    meta.set_hexpand(true);

    let title = Label::new(Some(&entry.file_name));
    title.add_css_class("recent-captures-hero-title");
    title.set_wrap(true);
    title.set_halign(Align::Start);
    title.set_xalign(0.0);

    let timestamp = Label::new(Some(&format_capture_time(entry.modified_at)));
    timestamp.add_css_class("recent-captures-hero-timestamp");
    timestamp.set_halign(Align::Start);
    timestamp.set_xalign(0.0);

    let supporting = Label::new(Some(&format_capture_supporting_copy(entry)));
    supporting.add_css_class("recent-captures-hero-supporting");
    supporting.set_wrap(true);
    supporting.set_halign(Align::Start);
    supporting.set_xalign(0.0);

    let actions = GtkBox::new(Orientation::Horizontal, 14);
    actions.add_css_class("recent-captures-hero-actions");

    let overlay_button = Button::with_label("Float Overlay");
    overlay_button.add_css_class("recent-captures-secondary-button");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        overlay_button.connect_clicked(move |_| match show_capture_preview_overlay(path.clone()) {
            Ok(_) => status.set_text("Floating overlay launched"),
            Err(e) => status.set_text(&format!("Failed to launch overlay: {}", e)),
        });
    }

    let open_button = Button::with_label("Open File");
    open_button.add_css_class("recent-captures-secondary-button");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        open_button.connect_clicked(move |_| {
            open_path_with_default_app(&path, &status, "Opened file");
        });
    }

    let reveal_button = Button::with_label("Reveal in Folder");
    reveal_button.add_css_class("recent-captures-secondary-button");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        reveal_button.connect_clicked(move |_| {
            if let Some(parent) = path.parent() {
                open_path_with_default_app(parent, &status, "Opened containing folder");
            } else {
                status.set_text("Folder unavailable for this screenshot");
            }
        });
    }

    actions.append(&overlay_button);
    actions.append(&open_button);
    actions.append(&reveal_button);

    meta.append(&title);
    meta.append(&timestamp);
    meta.append(&supporting);
    meta.append(&actions);

    section.append(&hero_image);
    section.append(&meta);
    section
}

fn build_recent_grid(entries: &[RecentCaptureEntry], status_label: &Label, columns: u32) -> GtkBox {
    let wrapper = GtkBox::new(Orientation::Vertical, 14);
    wrapper.add_css_class("recent-captures-grid-section");

    let title = Label::new(Some("More Recent"));
    title.add_css_class("recent-captures-grid-title");
    title.set_halign(Align::Start);

    let grid = Grid::new();
    grid.add_css_class("recent-captures-grid");
    grid.set_column_spacing(24);
    grid.set_row_spacing(24);
    grid.set_hexpand(true);
    grid.set_column_homogeneous(true);

    for (index, entry) in entries.iter().enumerate() {
        let card = build_grid_card(entry, status_label, index % 2 == 1, columns);
        card.set_hexpand(true);
        grid.attach(
            &card,
            (index as u32 % columns) as i32,
            (index as u32 / columns) as i32,
            1,
            1,
        );
    }

    wrapper.append(&title);
    wrapper.append(&grid);
    wrapper
}

fn build_grid_card(
    entry: &RecentCaptureEntry,
    status_label: &Label,
    alternate: bool,
    columns: u32,
) -> GtkBox {
    let list_mode = columns == 1;

    // In list mode: horizontal row. In grid mode: vertical card.
    let card = if list_mode {
        GtkBox::new(Orientation::Horizontal, 16)
    } else {
        GtkBox::new(Orientation::Vertical, 10)
    };
    card.add_css_class("recent-captures-list-row");
    if alternate {
        card.add_css_class("recent-captures-card-alt");
    }

    // Thumbnail — smaller in list mode, full-width in grid mode
    let (thumb_w, thumb_h) = if list_mode {
        (140, 88)
    } else {
        (CARD_IMAGE_WIDTH, CARD_IMAGE_HEIGHT)
    };

    let preview_trigger = Button::new();
    preview_trigger.set_has_frame(false);
    if !list_mode {
        preview_trigger.set_hexpand(true);
    }
    preview_trigger.add_css_class("recent-captures-card");

    let thumbnail = build_thumbnail(entry.path.clone(), thumb_w, thumb_h, false);
    thumbnail.add_css_class("recent-captures-card-image");
    if !list_mode {
        thumbnail.set_hexpand(true);
    }
    preview_trigger.set_child(Some(&thumbnail));

    {
        let path = entry.path.clone();
        let status = status_label.clone();
        preview_trigger.connect_clicked(move |_| {
            if is_animated_media(&path) {
                status.set_text("Opened Video Editor (Coming Soon)");
            } else {
                match show_capture_preview_overlay(path.clone()) {
                    Ok(_) => status.set_text("Floating overlay launched"),
                    Err(e) => status.set_text(&format!("Failed to launch overlay: {}", e)),
                }
            }
        });
    }

    let title = Label::new(Some(&entry.file_name));
    title.add_css_class("recent-captures-card-title");
    title.set_wrap(true);
    title.set_halign(Align::Start);
    title.set_xalign(0.0);

    let timestamp = Label::new(Some(&format_capture_time(entry.modified_at)));
    timestamp.add_css_class("recent-captures-card-timestamp");
    timestamp.set_halign(Align::Start);
    timestamp.set_xalign(0.0);

    let meta = Label::new(Some(&format_capture_meta(entry)));
    meta.add_css_class("recent-captures-card-meta");
    meta.set_halign(Align::Start);
    meta.set_xalign(0.0);

    let actions = GtkBox::new(Orientation::Horizontal, 4);
    actions.set_margin_top(4);

    let btn_overlay = Button::builder()
        .icon_name("window-new-symbolic")
        .has_frame(false)
        .tooltip_text("Float Overlay")
        .build();
    btn_overlay.add_css_class("recent-captures-icon-btn");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        btn_overlay.connect_clicked(move |_| match show_capture_preview_overlay(path.clone()) {
            Ok(_) => status.set_text("Floating overlay launched"),
            Err(e) => status.set_text(&format!("Failed to launch overlay: {}", e)),
        });
    }

    let btn_open = Button::builder()
        .icon_name("document-open-symbolic")
        .has_frame(false)
        .tooltip_text("Open File")
        .build();
    btn_open.add_css_class("recent-captures-icon-btn");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        btn_open.connect_clicked(move |_| {
            open_path_with_default_app(&path, &status, "Opened capture externally");
        });
    }

    let btn_reveal = Button::builder()
        .icon_name("folder-open-symbolic")
        .has_frame(false)
        .tooltip_text("Reveal in Folder")
        .build();
    btn_reveal.add_css_class("recent-captures-icon-btn");
    {
        let path = entry.path.clone();
        let status = status_label.clone();
        btn_reveal.connect_clicked(move |_| {
            if let Some(parent) = path.parent() {
                open_path_with_default_app(parent, &status, "Opened containing folder");
            } else {
                status.set_text("Folder unavailable for this capture");
            }
        });
    }

    actions.append(&btn_overlay);
    actions.append(&btn_open);
    actions.append(&btn_reveal);

    let metadata_box = GtkBox::new(Orientation::Vertical, 4);
    if list_mode {
        metadata_box.set_hexpand(true);
        metadata_box.set_valign(Align::Center);
    }
    metadata_box.append(&title);
    metadata_box.append(&timestamp);
    metadata_box.append(&meta);
    metadata_box.append(&actions);

    card.append(&preview_trigger);
    card.append(&metadata_box);

    card
}

fn build_empty_state(source_dir: Option<&PathBuf>, filter: discovery::CaptureModeFilter) -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_valign(Align::Center);

    let (title_text, detail_text) = match filter {
        discovery::CaptureModeFilter::Recordings => (
            "Video Recording is Coming Soon!".to_string(),
            "The ability to screen record natively within ApexShot is currently in development and will arrive in a future update.".to_string(),
        ),
        discovery::CaptureModeFilter::Screenshots | discovery::CaptureModeFilter::All => (
            "No saved screenshots yet".to_string(),
            match source_dir {
                Some(dir) => format!("Save a screenshot and it will appear here. Current location: {}", dir.display()),
                None => "ApexShot could not resolve a screenshot location on this system.".to_string(),
            }
        ),
    };

    let title = Label::new(Some(&title_text));
    title.add_css_class("recent-captures-empty-title");
    title.set_halign(Align::Start);

    let detail = Label::new(Some(&detail_text));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_wrap(true);
    detail.set_halign(Align::Start);
    detail.set_xalign(0.0);

    empty.append(&title);
    empty.append(&detail);
    empty
}

fn is_animated_media(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "mov" | "gif"
    )
}

fn build_thumbnail(path: PathBuf, width: i32, height: i32, hero: bool) -> gtk4::Overlay {
    let overlay = gtk4::Overlay::new();
    if !hero {
        overlay.set_hexpand(true);
    }

    let picture = gtk4::Picture::new();
    picture.set_size_request(width, height);
    picture.set_can_shrink(true);
    if !hero {
        picture.set_hexpand(true);
    }
    if hero {
        picture.add_css_class("recent-captures-hero-picture");
    } else {
        picture.add_css_class("recent-captures-grid-picture");
    }

    let picture_weak = picture.downgrade();
    let path_clone = path.clone();
    glib::idle_add_local_once(move || {
        let Some(picture) = picture_weak.upgrade() else {
            return;
        };
        // For animated media, standard Pixbuf scaling will gracefully fail and fall back to "missing"
        if let Some(texture) = load_texture(&path_clone, width, height) {
            picture.set_paintable(Some(&texture));
        } else {
            picture.add_css_class("recent-captures-picture-missing");
        }
    });

    overlay.set_child(Some(&picture));

    if is_animated_media(&path) {
        let badge = gtk4::Image::from_icon_name("media-playback-start-symbolic");
        badge.set_pixel_size(if hero { 64 } else { 32 });
        badge.set_halign(Align::Center);
        badge.set_valign(Align::Center);
        badge.add_css_class("recent-captures-media-badge");
        overlay.add_overlay(&badge);
    }

    overlay
}

fn load_texture(path: &Path, width: i32, height: i32) -> Option<gdk::Texture> {
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(path, width, height, true).ok()?;
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

fn format_capture_time(modified_at: SystemTime) -> String {
    let timestamp: DateTime<Local> = modified_at.into();
    timestamp.format("%b %e, %Y at %H:%M").to_string()
}

fn format_capture_meta(entry: &RecentCaptureEntry) -> String {
    let is_anim = is_animated_media(&entry.path);
    let extension = entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_uppercase())
        .unwrap_or_else(|| "FILE".to_string());

    if is_anim {
        format!("{extension} recording")
    } else {
        format!("{extension} screenshot")
    }
}

fn format_capture_supporting_copy(entry: &RecentCaptureEntry) -> String {
    format!(
        "{}. Float it in the overlay, open the saved file directly, or reveal it in the folder.",
        format_capture_meta(entry)
    )
}

fn open_path_with_default_app(path: &Path, status_label: &Label, success_message: &str) {
    if !path.exists() {
        status_label.set_text("Path is no longer available");
        return;
    }

    match Command::new("xdg-open").arg(path).spawn() {
        Ok(_) => status_label.set_text(success_message),
        Err(err) => status_label.set_text(&format!("Open failed: {err}")),
    }
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

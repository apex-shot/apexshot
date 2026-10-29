//! Tool-specific side panels for the Windows video editor.
//!
//! The Cursor and Background panels own their presentation and local settings.
//! The Zoom panel is a view over the timeline: it reads the selected zoom clip
//! and the camera settings, and every control writes straight back into them,
//! so the preview and the export both see the edit.

use super::background::{self, Background};
use super::timeline::{nearest_zoom_preset, ZoomMode, ZOOM_SCALE_PRESETS};
use super::webcam::{CameraLayout, LayerSettings, LayoutCue, LayoutSettings};
use crate::capture::editor::window::icon_names;
use crate::i18n::{t, tfmt};
use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{
    ClickEffect, CursorMotionStyle, CursorSettings, CursorTheme,
};
use gtk4::cairo::{FontSlant, FontWeight};
use gtk4::gdk;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea,
    EventControllerMotion, FileChooserAction, FileChooserNative, FileFilter, GestureClick,
    GestureDrag, Grid, Image, Label, Orientation, PolicyType, ResponseType, ScrolledWindow,
    SpinButton, Stack, StackTransitionType, Switch, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(super) const TOOL_COLUMN_WIDTH: i32 = 288;

pub(super) fn build_tools_column(
    scale: f64,
    window: &ApplicationWindow,
    background: Background,
    cursor: super::cursor::CursorModel,
    cursor_repaint: Rc<dyn Fn()>,
    timeline: &super::timeline::Timeline,
) -> GtkBox {
    let column = GtkBox::new(Orientation::Vertical, 0);
    column.add_css_class("recording-editor-tools-column");
    column.set_hexpand(false);
    column.set_halign(Align::End);
    column.set_size_request((TOOL_COLUMN_WIDTH as f64 * scale).round() as i32, -1);

    let rail = GtkBox::new(Orientation::Horizontal, 4);
    rail.add_css_class("recording-editor-tool-section");

    let pages = Stack::new();
    pages.set_transition_type(StackTransitionType::Crossfade);
    pages.set_transition_duration(140);
    pages.set_hexpand(false);
    pages.set_vexpand(true);
    pages.set_size_request((TOOL_COLUMN_WIDTH as f64 * scale).round() as i32, -1);
    pages.add_named(
        &build_background_panel(scale, window, background, &column),
        Some("background"),
    );
    pages.add_named(
        &build_cursor_panel(scale, cursor, cursor_repaint),
        Some("cursor"),
    );
    pages.add_named(&build_frames_panel(scale), Some("frames"));
    let (webcam_page, refresh_webcam) = build_webcam_panel(scale, timeline.clone());
    pages.add_named(&webcam_page, Some("webcam"));

    let buttons = Rc::new(RefCell::new(Vec::<ToggleButton>::new()));
    let webcam_button = Rc::new(RefCell::new(None::<ToggleButton>));
    // Selecting a zoom pill reveals its own panel. There is no rail icon for
    // it, because the panel belongs to the selection rather than to a mode:
    // clicking a rail icon afterwards still wins, since only a change of
    // selection reveals it again.
    let reveal_zoom: Rc<dyn Fn()> = {
        let pages = pages.clone();
        let buttons = buttons.clone();
        Rc::new(move || {
            for button in buttons.borrow().iter() {
                button.set_active(false);
            }
            pages.set_visible_child_name("zoom");
        })
    };
    let (zoom_page, refresh_zoom) = build_zoom_panel(scale, timeline.clone(), reveal_zoom);
    pages.add_named(&zoom_page, Some("zoom"));
    pages.set_visible_child_name("background");
    timeline.set_zoom_listener(refresh_zoom);

    let tools = [
        (
            "background",
            icon_names::custom::IMAGE_ALT_SYMBOLIC,
            t("Background"),
        ),
        ("cursor", icon_names::POINTER_PRIMARY_CLICK, t("Cursor")),
        (
            "frames",
            icon_names::custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            t("Frames"),
        ),
        ("webcam", icon_names::custom::WEBCAM_SYMBOLIC, t("Webcam")),
    ];

    for (index, (page_name, icon_name, label)) in tools.into_iter().enumerate() {
        let button = ToggleButton::new();
        button.add_css_class("recording-editor-tool-section-btn");
        button.set_has_frame(false);
        button.set_hexpand(true);
        button.set_tooltip_text(Some(&label));
        button.set_active(index == 0);
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size((18.0 * scale).round() as i32);
        icon.set_halign(Align::Center);
        icon.set_valign(Align::Center);
        button.set_child(Some(&icon));

        let buttons_for_click = buttons.clone();
        let pages_for_click = pages.clone();
        button.connect_clicked(move |clicked| {
            if !clicked.is_active() {
                clicked.set_active(true);
            }
            for (other_index, other) in buttons_for_click.borrow().iter().enumerate() {
                if other_index != index {
                    other.set_active(false);
                }
            }
            pages_for_click.set_visible_child_name(page_name);
        });

        buttons.borrow_mut().push(button.clone());
        if page_name == "webcam" {
            *webcam_button.borrow_mut() = Some(button.clone());
        }
        rail.append(&button);
    }

    let reveal_webcam: Rc<dyn Fn()> = {
        let pages = pages.clone();
        let buttons = buttons.clone();
        let webcam_button = webcam_button.clone();
        Rc::new(move || {
            let all: Vec<ToggleButton> = buttons.borrow().clone();
            for button in all {
                button.set_active(false);
            }
            if let Some(button) = webcam_button.borrow().as_ref() {
                button.set_active(true);
            }
            pages.set_visible_child_name("webcam");
        })
    };
    let webcam_epoch = Rc::new(Cell::new(None::<u64>));
    let webcam_refresh: Rc<dyn Fn()> = {
        let refresh = refresh_webcam;
        let timeline = timeline.clone();
        let reveal_webcam = reveal_webcam;
        Rc::new(move || {
            refresh();
            let state = timeline.webcam_panel_state();
            if webcam_selection_reveal(
                webcam_epoch.get(),
                state.selection_epoch,
                state.selected.is_some(),
            ) {
                reveal_webcam();
            }
            webcam_epoch.set(Some(state.selection_epoch));
        })
    };
    timeline.set_webcam_listener(webcam_refresh);

    column.append(&rail);
    column.append(&pages);
    column
}

fn build_background_panel(
    scale: f64,
    window: &ApplicationWindow,
    background: Background,
    column: &GtkBox,
) -> GtkBox {
    let body = panel_body();
    body.set_vexpand(false);
    body.set_valign(Align::Start);
    let choice_buttons = Rc::new(RefCell::new(Vec::<ToggleButton>::new()));

    let source_tabs = GtkBox::new(Orientation::Horizontal, 0);
    source_tabs.add_css_class("recording-editor-bg-tabs");
    source_tabs.set_homogeneous(true);
    let wallpaper_tab = tab_button(&t("Wallpaper"), "recording-editor-bg-tab");
    let custom_tab = tab_button(&t("Custom"), "recording-editor-bg-tab");
    let image_tab = tab_button(&t("Image"), "recording-editor-bg-tab");
    custom_tab.set_group(Some(&wallpaper_tab));
    image_tab.set_group(Some(&wallpaper_tab));
    wallpaper_tab.set_active(true);
    source_tabs.append(&wallpaper_tab);
    source_tabs.append(&custom_tab);
    source_tabs.append(&image_tab);
    body.append(&source_tabs);

    let none = Button::with_label(&t("None"));
    none.add_css_class("recording-editor-bg-none-row");
    none.add_css_class("video-editor-background-none");
    none.set_has_frame(false);
    body.append(&none);

    let source_pages = GtkBox::new(Orientation::Vertical, 0);
    source_pages.set_vexpand(false);
    source_pages.set_valign(Align::Start);
    source_pages.set_hexpand(true);
    let (wallpaper_page, wallpaper_scroll) =
        build_wallpaper_page(scale, choice_buttons.clone(), background.clone());
    wallpaper_page.set_visible(true);
    let select_custom = {
        let choices = choice_buttons.clone();
        let background = background.clone();
        Rc::new(move || {
            for card in choices.borrow().iter() {
                card.set_active(false);
            }
            background.select(None);
        }) as Rc<dyn Fn()>
    };
    let custom_page =
        super::custom_fill::build_custom_page(scale, column, &background, select_custom);
    custom_page.widget.set_visible(false);
    let (image_page, reset_image) = build_image_page(scale, window, &background);
    image_page.set_visible(false);
    source_pages.append(&wallpaper_page);
    source_pages.append(&custom_page.widget);
    source_pages.append(&image_page);

    wallpaper_tab.connect_toggled({
        let wallpaper_page = wallpaper_page.clone();
        let custom_page = custom_page.widget.clone();
        let image_page = image_page.clone();
        let wallpaper_scroll = wallpaper_scroll.clone();
        move |tab| {
            if tab.is_active() {
                wallpaper_page.set_visible(true);
                custom_page.set_visible(false);
                image_page.set_visible(false);
                let adjustment = wallpaper_scroll.vadjustment();
                adjustment.set_value(adjustment.lower());
            }
        }
    });
    custom_tab.connect_toggled({
        let wallpaper_page = wallpaper_page.clone();
        let custom_page = custom_page.widget.clone();
        let image_page = image_page.clone();
        move |tab| {
            if tab.is_active() {
                wallpaper_page.set_visible(false);
                custom_page.set_visible(true);
                image_page.set_visible(false);
            }
        }
    });
    image_tab.connect_toggled({
        let wallpaper_page = wallpaper_page.clone();
        let custom_page = custom_page.widget.clone();
        let image_page = image_page.clone();
        move |tab| {
            if tab.is_active() {
                wallpaper_page.set_visible(false);
                custom_page.set_visible(false);
                image_page.set_visible(true);
            }
        }
    });
    body.append(&source_pages);

    for card in choice_buttons.borrow().iter() {
        let clear_custom = custom_page.clear.clone();
        card.connect_clicked(move |card| {
            if card.is_active() {
                clear_custom();
            }
        });
    }

    // None is an option of its own, not just a reset: it carries the panel's
    // selected tint exactly while no fill is picked, and the model is what says
    // so — a wallpaper, a custom color, or a gradient each clear it.
    let background_for_none_state = background.state();
    let none_for_selection = none.clone();
    let sync_none: Rc<dyn Fn()> = Rc::new(move || {
        if background_for_none_state.has_fill() {
            none_for_selection.remove_css_class("active-background-option");
        } else {
            none_for_selection.add_css_class("active-background-option");
        }
    });
    {
        let sync_none = sync_none.clone();
        background.connect(Rc::new(move || sync_none()));
    }

    let choices_for_none = choice_buttons.clone();
    let clear_custom = custom_page.clear.clone();
    let wallpaper_scroll_for_none = wallpaper_scroll.clone();
    let background_for_none = background.clone();
    let sync_none_for_click = sync_none.clone();
    let reset_image_for_none = reset_image.clone();
    none.connect_clicked(move |_| {
        for card in choices_for_none.borrow().iter() {
            card.set_active(false);
        }
        clear_custom();
        reset_image_for_none();
        background_for_none.select(None);
        sync_none_for_click();
        let adjustment = wallpaper_scroll_for_none.vadjustment();
        adjustment.set_value(adjustment.lower());
    });

    let options = GtkBox::new(Orientation::Vertical, 8);
    options.add_css_class("recording-editor-bg-options");
    let padding = background.padding();
    options.append(&value_slider_row_with(
        &t("Padding"),
        0.0,
        80.0,
        padding,
        1.0,
        0,
        scale,
        {
            let background = background.clone();
            move |value| background.set_padding(value)
        },
    ));
    options.append(&value_slider_row_with(
        &t("Radius"),
        0.0,
        40.0,
        background.radius(),
        1.0,
        0,
        scale,
        {
            let background = background.clone();
            move |value| background.set_radius(value)
        },
    ));
    body.append(&options);
    panel_frame(&t("Background"), body)
}

fn build_wallpaper_page(
    scale: f64,
    choices: Rc<RefCell<Vec<ToggleButton>>>,
    background: Background,
) -> (GtkBox, ScrolledWindow) {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    let grid = Grid::new();
    grid.add_css_class("video-editor-wallpaper-grid");
    grid.set_column_spacing((6.0 * scale).round() as u32);
    grid.set_row_spacing((6.0 * scale).round() as u32);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);

    let card_size = (56.0 * scale).round() as i32;
    let mut first_button = None::<ToggleButton>;
    for (index, file_name) in background::WALLPAPERS.into_iter().enumerate() {
        let card = ToggleButton::new();
        card.add_css_class("video-editor-wallpaper-card");
        card.set_has_frame(false);
        card.set_hexpand(true);
        card.set_halign(Align::Fill);
        card.set_valign(Align::Fill);
        card.set_size_request(card_size, card_size);
        let preview = DrawingArea::new();
        preview.set_size_request(card_size, card_size);
        preview.set_content_width(card_size);
        preview.set_content_height(card_size);
        preview.set_hexpand(true);
        preview.set_vexpand(true);
        preview.set_halign(Align::Fill);
        preview.set_valign(Align::Fill);
        preview.set_draw_func(move |_, context, width, height| {
            draw_wallpaper_fallback(context, width, height);
        });
        // Decoding sixty previews up front would stall the panel; each card
        // fills in on the next idle instead.
        let pixbuf = background::thumb_path(file_name).and_then(|path| {
            Pixbuf::from_file_at_scale(&path, card_size * 2, card_size * 2, true).ok()
        });
        if let Some(pixbuf) = pixbuf {
            let preview = preview.clone();
            gtk4::glib::idle_add_local_once(move || {
                preview.set_draw_func(move |_, context, width, height| {
                    draw_wallpaper_thumbnail(context, width, height, &pixbuf);
                });
                preview.queue_draw();
            });
        }
        card.set_child(Some(&preview));
        if let Some(first) = &first_button {
            card.set_group(Some(first));
        } else {
            first_button = Some(card.clone());
        }
        let selected = file_name.to_owned();
        card.connect_clicked({
            let background = background.clone();
            move |card| {
                if card.is_active() {
                    background.select(Some(&selected));
                }
            }
        });
        choices.borrow_mut().push(card.clone());
        grid.attach(&card, (index % 4) as i32, (index / 4) as i32, 1, 1);
    }
    let grid_scroll = ScrolledWindow::new();
    grid_scroll.add_css_class("video-editor-wallpaper-scroll");
    grid_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    grid_scroll.set_propagate_natural_height(true);
    grid_scroll.set_max_content_height((320.0 * scale).round() as i32);
    grid_scroll.set_hexpand(true);
    grid_scroll.set_child(Some(&grid));
    {
        let adjustment = grid_scroll.vadjustment();
        gtk4::glib::idle_add_local_once(move || adjustment.set_value(adjustment.lower()));
    }
    page.append(&grid_scroll);
    (page, grid_scroll)
}

fn build_image_page(
    scale: f64,
    window: &ApplicationWindow,
    background: &Background,
) -> (GtkBox, Rc<dyn Fn()>) {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_hexpand(true);
    let picker = Button::new();
    picker.add_css_class("recording-editor-bg-image-row");
    picker.set_has_frame(false);
    picker.set_hexpand(true);
    let inner = GtkBox::new(Orientation::Horizontal, 8);
    let icon = Image::from_icon_name(icon_names::custom::IMAGE_ALT_SYMBOLIC);
    icon.set_pixel_size((15.0 * scale).round() as i32);
    let label = Label::new(Some(&t("Select image…")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    let browse = Image::from_icon_name("folder-open-symbolic");
    browse.set_pixel_size((14.0 * scale).round() as i32);
    inner.append(&icon);
    inner.append(&label);
    inner.append(&browse);
    picker.set_child(Some(&inner));
    page.append(&picker);

    let reset: Rc<dyn Fn()> = {
        let label = label.clone();
        let icon = icon.clone();
        let scale = scale;
        Rc::new(move || {
            label.set_text(&t("Select image…"));
            icon.set_icon_name(Some(icon_names::custom::IMAGE_ALT_SYMBOLIC));
            icon.set_pixel_size((15.0 * scale).round() as i32);
        })
    };
    let choose = {
        let window = window.clone();
        let label = label.clone();
        let icon = icon.clone();
        let background = background.clone();
        let scale = scale;
        Rc::new(move || {
            let label = label.clone();
            let icon = icon.clone();
            let background = background.clone();
            let scale = scale;
            choose_background_image(&window, move |picked| {
                let pixbuf = super::background::load_picture(&picked);
                let Some(pixbuf) = pixbuf else {
                    return;
                };
                label.set_text(&picked.file_name().map_or_else(
                    || t("Background image"),
                    |name| name.to_string_lossy().to_string(),
                ));
                icon.set_from_pixbuf(Some(&thumbnail_pixbuf(&pixbuf, scale)));
                // The stage paints the picture itself from here: the same slot the
                // Custom tab uses, so a picture and a color never both apply.
                background.set_custom_fill(Some(Rc::new(move |context, width, height| {
                    super::background::paint_cover(context, width, height, &pixbuf);
                })));
            });
        })
    };
    picker.connect_clicked(move |_| choose());
    (page, reset)
}

/// The row's own preview of the picked picture, small enough to sit where the
/// image glyph was.
fn thumbnail_pixbuf(pixbuf: &gtk4::gdk_pixbuf::Pixbuf, scale: f64) -> gtk4::gdk_pixbuf::Pixbuf {
    let size = (15.0 * scale).round() as i32;
    pixbuf
        .scale_simple(size, size, gtk4::gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or_else(|| pixbuf.clone())
}

fn update_cursor_settings(
    cursor: &super::cursor::CursorModel,
    repaint: &Rc<dyn Fn()>,
    update: impl FnOnce(&mut CursorSettings),
) {
    let mut settings = cursor.settings();
    update(&mut settings);
    cursor.set_settings(settings);
    repaint();
}

fn build_cursor_panel(
    scale: f64,
    cursor: super::cursor::CursorModel,
    repaint: Rc<dyn Fn()>,
) -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);
    panel.append(&panel_header(&t("Cursor")));

    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-cursor-tabs");
    tabs.set_hexpand(true);
    tabs.set_homogeneous(true);
    let style_tab = tab_button(&t("Style"), "recording-editor-cursor-tab");
    let follow_tab = tab_button(&t("Follow"), "recording-editor-cursor-tab");
    let effects_tab = tab_button(&t("Effects"), "recording-editor-cursor-tab");
    follow_tab.set_group(Some(&style_tab));
    effects_tab.set_group(Some(&style_tab));
    style_tab.set_active(true);
    tabs.append(&style_tab);
    tabs.append(&follow_tab);
    tabs.append(&effects_tab);
    panel.append(&tabs);

    let pages = Stack::new();
    pages.set_transition_type(StackTransitionType::Crossfade);
    pages.set_transition_duration(120);
    let (style_page, size_slider) = build_cursor_style_page(scale, cursor.clone(), repaint.clone());
    pages.add_named(&style_page, Some("style"));
    pages.add_named(
        &build_cursor_follow_page(scale, cursor.clone(), repaint.clone(), size_slider),
        Some("follow"),
    );
    pages.add_named(
        &build_cursor_effects_page(scale, cursor, repaint),
        Some("effects"),
    );
    pages.set_visible_child_name("style");
    connect_tabs(
        &pages,
        [
            (&style_tab, "style"),
            (&follow_tab, "follow"),
            (&effects_tab, "effects"),
        ],
    );

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.add_css_class("video-editor-side-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&pages));
    panel.append(&scroll);
    panel
}

fn build_cursor_style_page(
    scale: f64,
    cursor: super::cursor::CursorModel,
    repaint: Rc<dyn Fn()>,
) -> (GtkBox, FilledSlider) {
    let page = tab_body();
    let hint = Label::new(Some(&t("Shown over the recording in preview and export")));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    page.append(&hint);

    let grid = Grid::new();
    grid.add_css_class("recording-editor-cursor-grid");
    grid.set_column_spacing((8.0 * scale).round() as u32);
    grid.set_row_spacing((8.0 * scale).round() as u32);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);
    let settings = cursor.settings();
    let mut first = None::<ToggleButton>;
    for (index, theme) in CursorTheme::ALL.into_iter().enumerate() {
        let card = cursor_card(theme, scale);
        if let Some(first) = &first {
            card.set_group(Some(first));
        } else {
            first = Some(card.clone());
        }
        card.set_active(theme == settings.theme);
        card.connect_clicked({
            let cursor = cursor.clone();
            let repaint = repaint.clone();
            move |card| {
                if card.is_active() {
                    update_cursor_settings(&cursor, &repaint, |settings| settings.theme = theme);
                }
            }
        });
        grid.attach(&card, (index % 2) as i32, (index / 2) as i32, 1, 1);
    }
    page.append(&grid);
    // The sprite art is 48 pixels across, so anything past 2x is a magnified
    // picture; the shared model still clamps to 3.0 for platforms with finer
    // art.
    let size_slider = cursor_slider(
        &t("Size"),
        0.5,
        2.0,
        settings.size,
        0.05,
        1,
        scale,
        cursor.clone(),
        repaint.clone(),
        |settings, value| settings.size = value,
    );
    page.append(&size_slider.widget());
    page.append(
        &cursor_slider(
            &t("Shadow"),
            0.0,
            1.0,
            settings.shadow,
            0.05,
            1,
            scale,
            cursor,
            repaint,
            |settings, value| settings.shadow = value,
        )
        .widget(),
    );
    (page, size_slider)
}

fn build_cursor_follow_page(
    scale: f64,
    cursor: super::cursor::CursorModel,
    repaint: Rc<dyn Fn()>,
    size_slider: FilledSlider,
) -> GtkBox {
    let page = tab_body();
    let settings = cursor.settings();
    let preset_label = Label::new(Some(&t("PRESET")));
    preset_label.add_css_class("recording-editor-zoom-kicker");
    preset_label.set_xalign(0.0);
    page.append(&preset_label);

    let presets = GtkBox::new(Orientation::Horizontal, 8);
    presets.set_homogeneous(true);
    let focused = preset_card(&t("Focused"), &t("Tighter, faster"), scale);
    let smooth = preset_card(&t("Smooth"), &t("Softer follow"), scale);
    smooth.set_group(Some(&focused));
    let active_preset = settings.matching_motion_preset();
    focused.set_active(active_preset == Some(CursorMotionStyle::Focused));
    smooth.set_active(active_preset == Some(CursorMotionStyle::Smooth));
    presets.append(&focused);
    presets.append(&smooth);
    page.append(&presets);
    let smoothing_slider = cursor_slider(
        &t("Smoothing"),
        0.0,
        1.0,
        settings.smooth,
        0.05,
        1,
        scale,
        cursor.clone(),
        repaint.clone(),
        |settings, value| settings.smooth = value,
    );
    page.append(&smoothing_slider.widget());
    let speed_slider = cursor_slider(
        &t("Speed"),
        0.25,
        3.0,
        settings.speed,
        0.05,
        1,
        scale,
        cursor.clone(),
        repaint.clone(),
        |settings, value| settings.speed = value,
    );
    page.append(&speed_slider.widget());

    for (button, style) in [
        (focused, CursorMotionStyle::Focused),
        (smooth, CursorMotionStyle::Smooth),
    ] {
        button.connect_clicked({
            let cursor = cursor.clone();
            let repaint = repaint.clone();
            let size_slider = size_slider.clone();
            let smoothing_slider = smoothing_slider.clone();
            let speed_slider = speed_slider.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                let mut settings = cursor.settings();
                settings.apply_motion_preset(style);
                cursor.set_settings(settings);
                size_slider.set_value(settings.size);
                smoothing_slider.set_value(settings.smooth);
                speed_slider.set_value(settings.speed);
                repaint();
            }
        });
    }

    let idle_row = GtkBox::new(Orientation::Horizontal, 8);
    idle_row.add_css_class("recording-editor-zoom-classic");
    let idle_label = Label::new(Some(&t("Hide when idle")));
    idle_label.add_css_class("recording-editor-zoom-classic-label");
    idle_label.set_hexpand(true);
    idle_label.set_xalign(0.0);
    let idle = Switch::new();
    idle.add_css_class("recording-editor-zoom-switch");
    idle.set_active(settings.hide_idle);
    idle.set_halign(Align::End);
    idle.connect_state_set({
        let cursor = cursor.clone();
        let repaint = repaint.clone();
        move |_, hide_idle| {
            update_cursor_settings(&cursor, &repaint, |settings| settings.hide_idle = hide_idle);
            gtk4::glib::Propagation::Proceed
        }
    });
    idle_row.append(&idle_label);
    idle_row.append(&idle);
    page.append(&idle_row);
    page.append(
        &cursor_slider(
            &t("Idle delay"),
            120.0,
            4000.0,
            settings.idle_ms,
            40.0,
            0,
            scale,
            cursor,
            repaint,
            |settings, value| settings.idle_ms = value,
        )
        .widget(),
    );
    page
}

fn build_cursor_effects_page(
    scale: f64,
    cursor: super::cursor::CursorModel,
    repaint: Rc<dyn Fn()>,
) -> GtkBox {
    let page = tab_body();

    let anim_t = Rc::new(Cell::new(0.0));
    let live = DrawingArea::new();
    live.add_css_class("recording-editor-click-live-preview");
    live.set_content_width((240.0 * scale).round() as i32);
    live.set_content_height((88.0 * scale).round() as i32);
    live.set_hexpand(true);
    {
        let cursor = cursor.clone();
        let anim_t = anim_t.clone();
        live.set_draw_func(move |widget, context, width, height| {
            draw_click_live_preview(
                widget,
                context,
                f64::from(width),
                f64::from(height),
                cursor.settings(),
                anim_t.get(),
            );
        });
    }
    page.append(&live);

    let label = Label::new(Some(&t("CLICK EFFECT")));
    label.add_css_class("recording-editor-zoom-kicker");
    label.set_xalign(0.0);
    page.append(&label);

    let click_row = GtkBox::new(Orientation::Horizontal, (6.0 * scale).round() as i32);
    click_row.add_css_class("recording-editor-click-effect-row");
    click_row.set_hexpand(true);
    click_row.set_homogeneous(true);
    let none_btn = click_effect_card(ClickEffect::None, scale);
    let ripple_btn = click_effect_card(ClickEffect::Ripple, scale);
    let circle_btn = click_effect_card(ClickEffect::Circle, scale);
    ripple_btn.set_group(Some(&none_btn));
    circle_btn.set_group(Some(&none_btn));
    match cursor.settings().click_effect {
        ClickEffect::None => none_btn.set_active(true),
        ClickEffect::Ripple => ripple_btn.set_active(true),
        ClickEffect::Circle => circle_btn.set_active(true),
        ClickEffect::Spotlight | ClickEffect::Echo => {}
    }
    click_row.append(&none_btn);
    click_row.append(&ripple_btn);
    click_row.append(&circle_btn);
    page.append(&click_row);

    for (button, choice) in [
        (none_btn, ClickEffect::None),
        (ripple_btn, ClickEffect::Ripple),
        (circle_btn, ClickEffect::Circle),
    ] {
        let cursor = cursor.clone();
        let repaint = repaint.clone();
        let live = live.clone();
        button.connect_clicked(move |button| {
            if !button.is_active() {
                return;
            }
            update_cursor_settings(&cursor, &repaint, |settings| settings.click_effect = choice);
            live.queue_draw();
        });
    }

    let preview = live.downgrade();
    let anim_t = anim_t.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        let Some(preview) = preview.upgrade() else {
            return gtk4::glib::ControlFlow::Break;
        };
        if preview.is_mapped() {
            anim_t.set((anim_t.get() + 0.016) % 1.35);
            preview.queue_draw();
        }
        gtk4::glib::ControlFlow::Continue
    });

    page
}

/// The Zoom panel: how the selected clip frames, and the camera settings that
/// apply to every zoom. Every control writes into the timeline, so the preview
/// and the export read the change from the same place.
fn build_zoom_panel(
    scale: f64,
    timeline: super::timeline::Timeline,
    reveal: Rc<dyn Fn()>,
) -> (GtkBox, Rc<dyn Fn()>) {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);
    panel.append(&panel_header(&t("Zoom")));

    let body = panel_body();
    body.add_css_class("recording-editor-zoom-slider-spacing");
    let syncing = Rc::new(Cell::new(false));

    let mode_row = GtkBox::new(Orientation::Horizontal, 0);
    mode_row.add_css_class("recording-editor-zoom-mode");
    mode_row.set_hexpand(true);
    mode_row.set_homogeneous(true);
    let auto_button = tab_button(&t("Auto"), "recording-editor-zoom-mode-btn");
    let manual_button = tab_button(&t("Manual"), "recording-editor-zoom-mode-btn");
    manual_button.set_group(Some(&auto_button));
    mode_row.append(&auto_button);
    mode_row.append(&manual_button);
    body.append(&mode_row);

    let hint = Label::new(None);
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.set_max_width_chars(34);
    body.append(&hint);

    let chips = Grid::new();
    chips.add_css_class("recording-editor-zoom-chips");
    chips.set_column_spacing((4.0 * scale).round() as u32);
    chips.set_row_spacing((4.0 * scale).round() as u32);
    chips.set_column_homogeneous(true);
    chips.set_hexpand(true);
    let chip_buttons: Vec<Button> = ZOOM_SCALE_PRESETS
        .iter()
        .enumerate()
        .map(|(index, &(label, preset))| {
            let chip = Button::with_label(label);
            chip.add_css_class("recording-editor-zoom-chip");
            chip.set_hexpand(true);
            chip.set_has_frame(false);
            chip.connect_clicked({
                let timeline = timeline.clone();
                let syncing = syncing.clone();
                move |_| {
                    if syncing.get() {
                        return;
                    }
                    timeline.set_zoom_scale(preset);
                }
            });
            chips.attach(&chip, (index % 3) as i32, (index / 3) as i32, 1, 1);
            chip
        })
        .collect();
    body.append(&chips);

    let animation_header = GtkBox::new(Orientation::Horizontal, 8);
    animation_header.add_css_class("recording-editor-zoom-section-row");
    animation_header.set_hexpand(true);
    let animation_label = Label::new(Some(&t("Animation")));
    animation_label.add_css_class("recording-editor-zoom-kicker");
    animation_label.set_xalign(0.0);
    animation_label.set_hexpand(true);
    let reset = Button::with_label(&t("Reset"));
    reset.add_css_class("recording-editor-zoom-reset");
    reset.set_has_frame(false);
    reset.set_halign(Align::End);
    animation_header.append(&animation_label);
    animation_header.append(&reset);
    body.append(&animation_header);

    let instant_row = GtkBox::new(Orientation::Horizontal, 8);
    instant_row.add_css_class("recording-editor-zoom-classic");
    instant_row.set_hexpand(true);
    let instant_label = Label::new(Some(&t("Instant animation")));
    instant_label.add_css_class("recording-editor-zoom-classic-label");
    instant_label.set_xalign(0.0);
    instant_label.set_hexpand(true);
    let instant = Switch::new();
    instant.add_css_class("recording-editor-zoom-switch");
    instant.set_valign(Align::Center);
    instant.set_halign(Align::End);
    instant.set_tooltip_text(Some(&t(
        "If enabled, the zoom is applied instantly without any animation.",
    )));
    instant_row.append(&instant_label);
    instant_row.append(&instant);
    body.append(&instant_row);

    let camera_hint = Label::new(Some(&t("These settings apply to every zoom.")));
    camera_hint.add_css_class("recording-editor-zoom-hint");
    camera_hint.set_wrap(true);
    camera_hint.set_xalign(0.0);
    camera_hint.set_max_width_chars(34);
    body.append(&camera_hint);

    let camera = timeline.zoom_panel_state().camera;
    let speed_slider = value_slider_control(
        &t("Camera speed"),
        0.25,
        3.0,
        camera.speed,
        0.05,
        2,
        scale,
        {
            let timeline = timeline.clone();
            move |value| timeline.update_zoom_camera(|settings| settings.speed = value)
        },
    );
    body.append(&speed_slider.widget());
    let smoothness_slider = value_slider_control(
        &t("Smoothness"),
        0.0,
        1.0,
        camera.smoothness,
        0.01,
        2,
        scale,
        {
            let timeline = timeline.clone();
            move |value| timeline.update_zoom_camera(|settings| settings.smoothness = value)
        },
    );
    body.append(&smoothness_slider.widget());

    let early_row = GtkBox::new(Orientation::Horizontal, 8);
    early_row.add_css_class("recording-editor-zoom-classic");
    early_row.set_hexpand(true);
    let early_label = Label::new(Some(&t("Start animation early")));
    early_label.add_css_class("recording-editor-zoom-classic-label");
    early_label.set_xalign(0.0);
    early_label.set_hexpand(true);
    let start_early = Switch::new();
    start_early.add_css_class("recording-editor-zoom-switch");
    start_early.set_valign(Align::Center);
    start_early.set_halign(Align::End);
    start_early.set_active(camera.start_early);
    start_early.set_tooltip_text(Some(&t(
        "Begin the camera move 0.35 seconds before each zoom.",
    )));
    early_row.append(&early_label);
    early_row.append(&start_early);
    body.append(&early_row);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.add_css_class("video-editor-side-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));
    panel.append(&scroll);

    auto_button.connect_toggled({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            timeline.set_zoom_mode(ZoomMode::Auto);
        }
    });
    manual_button.connect_toggled({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            timeline.set_zoom_mode(ZoomMode::Manual);
        }
    });
    reset.connect_clicked({
        let timeline = timeline.clone();
        move |_| timeline.reset_zoom_animation()
    });
    instant.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.set_zoom_instant(active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    start_early.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.update_zoom_camera(|settings| settings.start_early = active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });

    // A refresh arrives on every repaint, so it compares first: playback
    // repaints twenty times a second and nothing here changes with it.
    let last = Rc::new(RefCell::new(None::<super::timeline::ZoomPanelState>));
    let refresh: Rc<dyn Fn()> = {
        let last = last.clone();
        let syncing = syncing.clone();
        let auto_button = auto_button.clone();
        let manual_button = manual_button.clone();
        let hint = hint.clone();
        let instant = instant.clone();
        let chip_buttons = chip_buttons.clone();
        let speed_slider = speed_slider.clone();
        let smoothness_slider = smoothness_slider.clone();
        let start_early = start_early.clone();
        let reset = reset.clone();
        Rc::new(move || {
            let state = timeline.zoom_panel_state();
            let previous = *last.borrow();
            if previous == Some(state) {
                return;
            }
            *last.borrow_mut() = Some(state);
            // Only a new selection reveals the panel, so a rail icon clicked
            // afterwards is not overridden by the next repaint.
            if state.selected.is_some()
                && previous.map(|state| state.selection_epoch) != Some(state.selection_epoch)
            {
                reveal();
            }
            let has_clip = state.clip.is_some();
            syncing.set(true);
            auto_button.set_sensitive(state.auto_available && has_clip);
            manual_button.set_sensitive(has_clip);
            match state.clip {
                Some(clip) => {
                    hint.set_text(&match clip.mode {
                        ZoomMode::Manual => t("Set a fixed focus point for this zoom"),
                        ZoomMode::Auto => t("Camera follows the recorded pointer"),
                    });
                    match clip.mode {
                        ZoomMode::Auto => auto_button.set_active(true),
                        ZoomMode::Manual => manual_button.set_active(true),
                    }
                    instant.set_active(clip.instant);
                    let selected_preset = nearest_zoom_preset(clip.scale);
                    for (chip, &(_, preset)) in chip_buttons.iter().zip(ZOOM_SCALE_PRESETS.iter()) {
                        if (preset - selected_preset).abs() < 1e-6 {
                            chip.add_css_class("recording-editor-zoom-chip-active");
                        } else {
                            chip.remove_css_class("recording-editor-zoom-chip-active");
                        }
                    }
                }
                None => {
                    manual_button.set_active(true);
                    hint.set_text(&t("Select a zoom clip on the timeline"));
                    instant.set_active(false);
                    for chip in chip_buttons.iter() {
                        chip.remove_css_class("recording-editor-zoom-chip-active");
                    }
                }
            }
            for chip in chip_buttons.iter() {
                chip.set_sensitive(has_clip);
            }
            instant.set_sensitive(has_clip);
            reset.set_sensitive(has_clip || state.camera != Default::default());
            speed_slider.set_value(state.camera.speed);
            smoothness_slider.set_value(state.camera.smoothness);
            start_early.set_active(state.camera.start_early);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    (panel, refresh)
}

/// The camera shapes the panel offers, named the way the recorder's Settings
/// page names them.
const WEBCAM_SHAPES: [&str; 4] = ["Circle", "Square", "Rectangle", "Vertical"];

/// The longest cross-fade the renderer draws. The slider mirrors it so a
/// project carrying a longer transition still shows its own value.
const WEBCAM_TRANSITION_MAX: f64 = 3.0;

/// The fixed height of a layout miniature, before scale.
const WEBCAM_PREVIEW_HEIGHT: f64 = 40.0;

/// The fixed height of one layout tile, before scale.
const WEBCAM_CARD_HEIGHT: f64 = 62.0;

/// The horizontal padding the camera body inherits from the shared panel sheet.
const WEBCAM_BODY_INSET: f64 = 28.0;

/// The six cards the layout chooser offers, in the order it draws them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WebcamLayoutCard {
    Screen,
    Camera,
    Bubble,
    Circle,
    Split,
    Cutout,
}

impl WebcamLayoutCard {
    const ALL: [Self; 6] = [
        Self::Screen,
        Self::Camera,
        Self::Bubble,
        Self::Circle,
        Self::Split,
        Self::Cutout,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Screen => "Screen",
            Self::Camera => "Camera",
            Self::Bubble => "Bubble",
            Self::Circle => "Circle",
            Self::Split => "Split",
            Self::Cutout => "Cutout",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Screen => "Show the screen on its own.",
            Self::Camera => "Fill the frame with the camera.",
            Self::Bubble => "Camera bubble on the screen.",
            Self::Circle => "Round camera bubble on the screen.",
            Self::Split => "Put the screen and camera side by side.",
            Self::Cutout => "Set the camera person on the screen.",
        }
    }

    fn layout(self) -> CameraLayout {
        match self {
            Self::Screen => CameraLayout::ScreenOnly,
            Self::Camera => CameraLayout::FullscreenCamera,
            Self::Bubble | Self::Circle => CameraLayout::CameraOverlay,
            Self::Split => CameraLayout::SplitScreen,
            Self::Cutout => CameraLayout::CameraCutout,
        }
    }

    fn overlay_shape(self) -> Option<u8> {
        match self {
            Self::Bubble => Some(2),
            Self::Circle => Some(0),
            _ => None,
        }
    }
}

/// The controls that belong to the layout being edited. A layout the playhead
/// only passes through does not decide them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct WebcamControlUse {
    overlay: bool,
    split: bool,
    cutout: bool,
    mirror: bool,
    transitions: bool,
}

fn webcam_selected_cue(state: &super::timeline::WebcamPanelState) -> Option<(usize, LayoutCue)> {
    let layouts = state.layouts.as_ref()?;
    let index = state.selected?;
    layouts.cues.get(index).copied().map(|cue| (index, cue))
}

fn webcam_editing_layout(state: &super::timeline::WebcamPanelState) -> CameraLayout {
    webcam_selected_cue(state)
        .map(|(_, cue)| cue.layout)
        .or_else(|| state.layouts.as_ref().map(|layouts| layouts.default_layout))
        .unwrap_or(CameraLayout::CameraOverlay)
}

fn webcam_editing_controls(state: &super::timeline::WebcamPanelState) -> WebcamControlUse {
    let editing = webcam_editing_layout(state);
    let camera_options = matches!(
        editing,
        CameraLayout::CameraOverlay | CameraLayout::SplitScreen | CameraLayout::CameraCutout
    );
    WebcamControlUse {
        overlay: editing == CameraLayout::CameraOverlay,
        split: editing == CameraLayout::SplitScreen,
        cutout: editing == CameraLayout::CameraCutout,
        mirror: camera_options,
        transitions: camera_options
            && state
                .layouts
                .as_ref()
                .is_some_and(|layouts| !layouts.cues.is_empty()),
    }
}

/// Whether the panel has to re-read the editing controls. Playback alone does
/// not change them, so a scrub or a frame tick never disturbs a control.
fn webcam_panel_needs_rebuild(
    previous: Option<&super::timeline::WebcamPanelState>,
    state: &super::timeline::WebcamPanelState,
) -> bool {
    previous.is_none_or(|previous| {
        previous.layer != state.layer
            || previous.layouts != state.layouts
            || previous.selected != state.selected
            || previous.duration != state.duration
            || previous.frame_aspect != state.frame_aspect
    })
}

/// A selection made anywhere, including a repeated click on the same change,
/// is the one thing that may bring the Webcam panel forward. Replacing the
/// document resets the epoch, so an absent selection never reveals.
fn webcam_selection_reveal(
    previous_epoch: Option<u64>,
    selection_epoch: u64,
    selected: bool,
) -> bool {
    selected && previous_epoch.is_some_and(|previous| previous != selection_epoch)
}

/// `m:ss.s`, the clock the layout change controls use.
fn format_cue_clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}

fn webcam_frame_aspect(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        16.0 / 9.0
    }
}

fn webcam_content_width(scale: f64) -> i32 {
    ((TOOL_COLUMN_WIDTH as f64 - WEBCAM_BODY_INSET) * scale).round() as i32
}

fn webcam_preview_aspect(value: f64) -> f64 {
    webcam_frame_aspect(value).clamp(0.75, 1.9)
}

fn webcam_alignment_step(value: f64) -> Option<usize> {
    if !value.is_finite() {
        return None;
    }
    [0.0, 0.5, 1.0]
        .iter()
        .position(|candidate| (value - candidate).abs() <= 1e-6)
}

fn webcam_cutout_position(step: usize) -> f64 {
    step.min(2) as f64 / 2.0
}

/// Which shape chip a layer matches, so a hand-tuned radius or aspect leaves
/// every chip clear instead of lighting the nearest one. The layer's height is
/// a fraction of the frame height, so a shape's physical aspect becomes
/// `frame_aspect * aspect_for_shape`.
fn webcam_shape_index(layer: &LayerSettings, frame_aspect: f64) -> Option<usize> {
    let frame_aspect = webcam_frame_aspect(frame_aspect);
    let aspect = if layer.w > 0.0 {
        layer.h / layer.w
    } else {
        1.0
    };
    let tolerance = 0.02 * frame_aspect.max(1.0);
    (0..WEBCAM_SHAPES.len()).find(|&index| {
        let shape = index as u8;
        (layer.radius - super::webcam::radius_for_shape(shape)).abs() < 0.02
            && (aspect - frame_aspect * super::webcam::aspect_for_shape(shape)).abs() < tolerance
    })
}

fn webcam_layout_card(
    layout: CameraLayout,
    layer: Option<&LayerSettings>,
    frame_aspect: f64,
) -> WebcamLayoutCard {
    match layout {
        CameraLayout::ScreenOnly => WebcamLayoutCard::Screen,
        CameraLayout::FullscreenCamera => WebcamLayoutCard::Camera,
        CameraLayout::CameraOverlay => {
            if layer.is_some_and(|layer| webcam_shape_index(layer, frame_aspect) == Some(0)) {
                WebcamLayoutCard::Circle
            } else {
                WebcamLayoutCard::Bubble
            }
        }
        CameraLayout::SplitScreen => WebcamLayoutCard::Split,
        CameraLayout::CameraCutout => WebcamLayoutCard::Cutout,
    }
}

type WebcamRefreshSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

fn request_refresh(slot: &WebcamRefreshSlot) {
    let refresh = slot.borrow().clone();
    if let Some(refresh) = refresh {
        refresh();
    }
}

type WebcamCardRect = (f64, f64, f64, f64);

fn webcam_card_fill(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    radius: f64,
    color: (f64, f64, f64, f64),
) {
    let (x, y, width, height) = rect;
    rounded_rect(context, x, y, width, height, radius);
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = context.fill();
}

fn webcam_card_outline(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    radius: f64,
    line_width: f64,
    color: (f64, f64, f64, f64),
) {
    let (x, y, width, height) = rect;
    rounded_rect(context, x, y, width, height, radius);
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.set_line_width(line_width);
    let _ = context.stroke();
}

fn webcam_card_frame(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    radius: f64,
    fill: (f64, f64, f64, f64),
    stroke: (f64, f64, f64, f64),
) {
    webcam_card_fill(context, rect, radius, fill);
    webcam_card_outline(context, rect, radius, 1.0, stroke);
}

fn webcam_card_ellipse_fill(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    color: (f64, f64, f64, f64),
) {
    let (x, y, width, height) = rect;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let _ = context.save();
    context.translate(x + width / 2.0, y + height / 2.0);
    context.scale(width / 2.0, height / 2.0);
    context.arc(0.0, 0.0, 1.0, 0.0, TAU);
    let _ = context.restore();
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = context.fill();
}

fn webcam_card_ellipse_outline(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    line_width: f64,
    color: (f64, f64, f64, f64),
) {
    let (x, y, width, height) = rect;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let _ = context.save();
    context.translate(x + width / 2.0, y + height / 2.0);
    context.scale(width / 2.0, height / 2.0);
    context.arc(0.0, 0.0, 1.0, 0.0, TAU);
    let _ = context.restore();
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.set_line_width(line_width);
    let _ = context.stroke();
}

fn webcam_card_person(
    context: &gtk4::cairo::Context,
    rect: WebcamCardRect,
    color: (f64, f64, f64, f64),
) {
    let (x, y, width, height) = rect;
    let head = width.min(height) * 0.40;
    webcam_card_ellipse_fill(context, (x + (width - head) / 2.0, y, head, head), color);
    webcam_card_ellipse_fill(context, (x, y + height * 0.46, width, height * 0.54), color);
}

fn draw_webcam_card_schematic(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    card: WebcamLayoutCard,
    selected: bool,
    light: bool,
) {
    if width < 16.0 || height < 12.0 {
        return;
    }
    let x = 3.0;
    let y = 3.0;
    let frame_width = width - 6.0;
    let frame_height = height - 6.0;
    let frame_radius = 3.0;
    let portrait = height > width;
    let (screen_fill, screen_stroke, camera_fill, camera_stroke, person) = if light {
        (
            (0.067, 0.094, 0.153, 0.06),
            (0.067, 0.094, 0.153, 0.18),
            (0.067, 0.094, 0.153, 0.28),
            (0.067, 0.094, 0.153, 0.42),
            (1.0, 1.0, 1.0, 0.92),
        )
    } else {
        (
            (1.0, 1.0, 1.0, 0.06),
            (1.0, 1.0, 1.0, 0.18),
            (1.0, 1.0, 1.0, 0.30),
            (1.0, 1.0, 1.0, 0.46),
            (0.07, 0.08, 0.10, 0.88),
        )
    };

    match card {
        WebcamLayoutCard::Screen => {
            webcam_card_frame(
                context,
                (x, y, frame_width, frame_height),
                frame_radius,
                screen_fill,
                screen_stroke,
            );
        }
        WebcamLayoutCard::Camera => {
            webcam_card_frame(
                context,
                (x, y, frame_width, frame_height),
                frame_radius,
                camera_fill,
                camera_stroke,
            );
            webcam_card_person(
                context,
                (
                    x + frame_width * 0.18,
                    y + frame_height * 0.12,
                    frame_width * 0.64,
                    frame_height * 0.76,
                ),
                person,
            );
        }
        WebcamLayoutCard::Bubble => {
            webcam_card_frame(
                context,
                (x, y, frame_width, frame_height),
                frame_radius,
                screen_fill,
                screen_stroke,
            );
            let bubble_width = frame_width * 0.34;
            let bubble_height = frame_height * 0.38;
            let bubble_x = x + frame_width - bubble_width - 2.0;
            let bubble_y = y + frame_height - bubble_height - 2.0;
            webcam_card_frame(
                context,
                (bubble_x, bubble_y, bubble_width, bubble_height),
                2.5,
                camera_fill,
                camera_stroke,
            );
            webcam_card_person(
                context,
                (
                    bubble_x + bubble_width * 0.18,
                    bubble_y + bubble_height * 0.14,
                    bubble_width * 0.64,
                    bubble_height * 0.72,
                ),
                person,
            );
        }
        WebcamLayoutCard::Circle => {
            webcam_card_frame(
                context,
                (x, y, frame_width, frame_height),
                frame_radius,
                screen_fill,
                screen_stroke,
            );
            let diameter = frame_height * 0.42;
            let bubble_x = x + frame_width - diameter - 2.0;
            let bubble_y = y + frame_height - diameter - 2.0;
            webcam_card_ellipse_fill(
                context,
                (bubble_x, bubble_y, diameter, diameter),
                camera_fill,
            );
            webcam_card_ellipse_outline(
                context,
                (bubble_x, bubble_y, diameter, diameter),
                1.0,
                camera_stroke,
            );
            webcam_card_person(
                context,
                (
                    bubble_x + diameter * 0.19,
                    bubble_y + diameter * 0.16,
                    diameter * 0.62,
                    diameter * 0.68,
                ),
                person,
            );
        }
        WebcamLayoutCard::Split => {
            if portrait {
                let gap = 1.5;
                let pane_height = (frame_height - gap) / 2.0;
                webcam_card_frame(
                    context,
                    (x, y, frame_width, pane_height),
                    2.5,
                    screen_fill,
                    screen_stroke,
                );
                let camera_y = y + pane_height + gap;
                webcam_card_frame(
                    context,
                    (x, camera_y, frame_width, pane_height),
                    2.5,
                    camera_fill,
                    camera_stroke,
                );
                webcam_card_person(
                    context,
                    (
                        x + frame_width * 0.20,
                        camera_y + pane_height * 0.14,
                        frame_width * 0.60,
                        pane_height * 0.72,
                    ),
                    person,
                );
            } else {
                let gap = 1.5;
                let pane_width = (frame_width - gap) / 2.0;
                webcam_card_frame(
                    context,
                    (x, y, pane_width, frame_height),
                    2.5,
                    screen_fill,
                    screen_stroke,
                );
                let camera_x = x + pane_width + gap;
                webcam_card_frame(
                    context,
                    (camera_x, y, pane_width, frame_height),
                    2.5,
                    camera_fill,
                    camera_stroke,
                );
                webcam_card_person(
                    context,
                    (
                        camera_x + pane_width * 0.20,
                        y + frame_height * 0.14,
                        pane_width * 0.60,
                        frame_height * 0.72,
                    ),
                    person,
                );
            }
        }
        WebcamLayoutCard::Cutout => {
            webcam_card_frame(
                context,
                (x, y, frame_width, frame_height),
                frame_radius,
                screen_fill,
                screen_stroke,
            );
            webcam_card_person(
                context,
                (
                    x + frame_width * 0.52,
                    y + frame_height * 0.20,
                    frame_width * 0.40,
                    frame_height * 0.72,
                ),
                camera_fill,
            );
        }
    }

    if selected {
        webcam_card_outline(
            context,
            (x - 1.5, y - 1.5, frame_width + 3.0, frame_height + 3.0),
            frame_radius + 1.5,
            1.0,
            (1.0, 0.404, 0.051, 1.0),
        );
    }
}

fn webcam_preview_height(scale: f64) -> i32 {
    (WEBCAM_PREVIEW_HEIGHT * scale).round() as i32
}

fn set_webcam_preview_geometry(preview: &DrawingArea, scale: f64, frame_aspect: f64) {
    let height = webcam_preview_height(scale);
    let width = (f64::from(height) * webcam_preview_aspect(frame_aspect)).round() as i32;
    preview.set_content_height(height);
    preview.set_content_width(width);
}

fn webcam_card_preview(
    scale: f64,
    card: WebcamLayoutCard,
    frame_aspect: f64,
    active: Rc<Cell<bool>>,
) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.add_css_class("recording-editor-webcam-preview");
    preview.set_hexpand(false);
    preview.set_halign(Align::Center);
    set_webcam_preview_geometry(&preview, scale, frame_aspect);
    preview.set_can_target(false);
    preview.set_draw_func(move |widget, context, width, height| {
        draw_webcam_card_schematic(
            context,
            f64::from(width),
            f64::from(height),
            card,
            active.get(),
            widget_is_light(widget),
        );
    });
    preview
}

struct WebcamLayoutTile {
    button: ToggleButton,
    preview: DrawingArea,
}

fn webcam_layout_tile(scale: f64, card: WebcamLayoutCard, frame_aspect: f64) -> WebcamLayoutTile {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-webcam-card");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_halign(Align::Fill);
    button.set_tooltip_text(Some(&t(card.tooltip())));
    let active = Rc::new(Cell::new(false));
    let preview = webcam_card_preview(scale, card, frame_aspect, active.clone());
    let preview_for_toggle = preview.clone();
    let label = Label::new(Some(&t(card.label())));
    label.add_css_class("recording-editor-webcam-card-label");
    label.set_xalign(0.5);
    label.set_single_line_mode(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(12);
    let body = GtkBox::new(Orientation::Vertical, (3.0 * scale).round() as i32);
    body.set_halign(Align::Fill);
    body.set_valign(Align::Center);
    body.append(&preview);
    body.append(&label);
    button.set_child(Some(&body));
    button.connect_toggled(move |button| {
        active.set(button.is_active());
        preview_for_toggle.queue_draw();
    });
    WebcamLayoutTile { button, preview }
}

struct WebcamLayoutGrid {
    root: Grid,
    tiles: Vec<WebcamLayoutTile>,
    scale: f64,
}

impl WebcamLayoutGrid {
    fn set_frame_aspect(&self, frame_aspect: f64) {
        for tile in &self.tiles {
            set_webcam_preview_geometry(&tile.preview, self.scale, frame_aspect);
            tile.preview.queue_draw();
        }
    }

    fn set_active(&self, active: WebcamLayoutCard) {
        for (tile, card) in self.tiles.iter().zip(WebcamLayoutCard::ALL) {
            tile.button.set_active(card == active);
        }
    }
}

fn build_webcam_layout_grid(scale: f64, frame_aspect: f64) -> WebcamLayoutGrid {
    let gap = (6.0 * scale).round() as i32;
    let width = ((webcam_content_width(scale) - 2 * gap) / 3).max(1);
    let height = (WEBCAM_CARD_HEIGHT * scale).round() as i32;
    let grid = Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_row_homogeneous(true);
    grid.set_column_spacing(gap as u32);
    grid.set_row_spacing(gap as u32);
    grid.set_hexpand(true);
    grid.set_halign(Align::Fill);
    let mut tiles = Vec::with_capacity(WebcamLayoutCard::ALL.len());
    let mut first = None::<ToggleButton>;
    for (index, card) in WebcamLayoutCard::ALL.into_iter().enumerate() {
        let tile = webcam_layout_tile(scale, card, frame_aspect);
        tile.button.set_size_request(width, height);
        if let Some(first) = &first {
            tile.button.set_group(Some(first));
        } else {
            first = Some(tile.button.clone());
        }
        grid.attach(&tile.button, (index % 3) as i32, (index / 3) as i32, 1, 1);
        tiles.push(tile);
    }
    WebcamLayoutGrid {
        root: grid,
        tiles,
        scale,
    }
}

fn webcam_icon_button(scale: f64, icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-webcam-icon");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size((14.0 * scale).round() as i32);
    button.set_child(Some(&icon));
    button
}

fn webcam_scope_button(label: &str) -> (ToggleButton, Label) {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-webcam-scope-btn");
    button.set_has_frame(false);
    button.set_hexpand(false);
    let text = Label::new(Some(label));
    text.set_single_line_mode(true);
    text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.set_max_width_chars(18);
    text.set_xalign(0.5);
    button.set_child(Some(&text));
    (button, text)
}

struct WebcamControlRows {
    root: GtkBox,
    default_scope: ToggleButton,
    change_scope: ToggleButton,
    add_change: Button,
    delete_change: Button,
    time_row: GtkBox,
    time_spin: SpinButton,
    status: Label,
}

fn build_webcam_control_rows(scale: f64) -> WebcamControlRows {
    let root = GtkBox::new(Orientation::Vertical, (6.0 * scale).round() as i32);
    root.set_hexpand(true);

    let top = GtkBox::new(Orientation::Horizontal, (6.0 * scale).round() as i32);
    top.set_hexpand(true);
    top.set_valign(Align::Center);
    let scope = GtkBox::new(Orientation::Horizontal, 0);
    scope.add_css_class("recording-editor-webcam-scope");
    scope.set_halign(Align::Fill);
    scope.set_hexpand(true);
    scope.set_homogeneous(true);
    let (default_scope, _default_label) = webcam_scope_button(&t("Default"));
    let (change_scope, _change_label) = webcam_scope_button(&t("Change"));
    default_scope.set_hexpand(true);
    change_scope.set_hexpand(true);
    change_scope.set_group(Some(&default_scope));
    default_scope.set_active(true);
    change_scope.set_sensitive(false);
    default_scope.set_tooltip_text(Some(&t("Edit the default layout for the whole recording.")));
    change_scope.set_tooltip_text(Some(&t(
        "Select a layout change in the timeline to edit it.",
    )));
    scope.append(&default_scope);
    scope.append(&change_scope);
    top.append(&scope);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    top.append(&spacer);
    let add_change = webcam_icon_button(
        scale,
        "list-add-symbolic",
        &t("Add a layout change at the playhead."),
    );
    let delete_change = webcam_icon_button(
        scale,
        "user-trash-symbolic",
        &t("Delete this layout change."),
    );
    delete_change.set_visible(false);
    top.append(&add_change);
    top.append(&delete_change);
    root.append(&top);

    let time_row = GtkBox::new(Orientation::Horizontal, 0);
    time_row.set_halign(Align::Fill);
    time_row.set_hexpand(true);
    time_row.set_visible(false);
    let time_spin = SpinButton::with_range(0.0, 0.1, 0.1);
    time_spin.add_css_class("recording-editor-webcam-time");
    time_spin.set_digits(1);
    time_spin.set_numeric(true);
    time_spin.set_width_chars(6);
    time_spin.set_hexpand(true);
    time_spin.set_size_request(-1, (28.0 * scale).round() as i32);
    time_spin.set_valign(Align::Center);
    time_spin.set_tooltip_text(Some(&t(
        "Move this change to another time. A time that collides with another change is rejected.",
    )));
    time_row.append(&time_spin);
    root.append(&time_row);

    let status = Label::new(None);
    status.add_css_class("recording-editor-webcam-status");
    status.set_xalign(0.0);
    status.set_single_line_mode(true);
    status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    status.set_max_width_chars(34);
    status.set_hexpand(true);
    status.set_halign(Align::Fill);
    root.append(&status);

    WebcamControlRows {
        root,
        default_scope,
        change_scope,
        add_change,
        delete_change,
        time_row,
        time_spin,
        status,
    }
}

fn webcam_hint(text: &str) -> Label {
    let hint = Label::new(Some(text));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    hint.set_lines(2);
    hint.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    hint.set_xalign(0.0);
    hint.set_max_width_chars(34);
    hint.set_hexpand(true);
    hint
}

fn webcam_kicker(label: &str) -> Label {
    let kicker = Label::new(Some(label));
    kicker.add_css_class("recording-editor-webcam-kicker");
    kicker.set_xalign(0.0);
    kicker
}

/// The camera's placement: a field you drag the dot around inside, with a
/// slider per axis underneath for the same two numbers.
///
/// The pad states position, not size: the layer's own width and height belong
/// to the Size slider, so the field only has to show where the camera sits in
/// the frame, and the sliders keep the exact value readable and typeable.
#[derive(Clone)]
struct WebcamPositionPad {
    area: DrawingArea,
    alignment: Rc<Cell<(f64, f64)>>,
    scale: f64,
    on_move: Rc<dyn Fn(f64, f64)>,
}

impl WebcamPositionPad {
    fn new(
        scale: f64,
        frame_aspect: f64,
        alignment: (f64, f64),
        on_move: Rc<dyn Fn(f64, f64)>,
    ) -> Self {
        let area = DrawingArea::new();
        area.add_css_class("recording-editor-webcam-pad");
        area.set_hexpand(true);
        // The field keeps the frame's proportions, so the dot's place in it
        // reads the way it will in the video, inside bounds that still leave a
        // comfortable drag target at every aspect.
        let height = (f64::from(webcam_content_width(scale)) / webcam_preview_aspect(frame_aspect))
            .round()
            .clamp(72.0 * scale, 168.0 * scale)
            .round() as i32;
        area.set_size_request(-1, height);
        let pad = Self {
            area: area.clone(),
            alignment: Rc::new(Cell::new(alignment)),
            scale,
            on_move,
        };
        area.set_draw_func({
            let pad = pad.clone();
            move |widget, context, width, height| pad.draw(widget, context, width, height)
        });

        let click = GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let pad = pad.clone();
            move |_, n_press, x, y| {
                if n_press == 2 {
                    pad.place((0.5, 0.5));
                } else {
                    pad.apply_point(x, y);
                }
            }
        });
        area.add_controller(click);

        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let pad = pad.clone();
            move |gesture, _, _| {
                if let Some((x, y)) = gesture.start_point() {
                    pad.apply_point(x, y);
                }
            }
        });
        drag.connect_drag_update({
            let pad = pad.clone();
            move |gesture, offset_x, offset_y| {
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                pad.apply_point(start_x + offset_x, start_y + offset_y);
            }
        });
        area.add_controller(drag);
        area.set_cursor(gdk::Cursor::from_name("move", None).as_ref());
        area.set_tooltip_text(Some(&t(
            "Drag to set the camera's position in the frame; double-click to centre it",
        )));
        pad
    }

    fn refresh(&self, alignment: (f64, f64)) {
        self.alignment.set(alignment);
        self.area.queue_draw();
    }

    /// Put the dot on an alignment and report it. The panel coalesces the
    /// edit, so a drag is one undo step rather than one per motion event.
    fn place(&self, alignment: (f64, f64)) {
        self.alignment.set(alignment);
        (self.on_move)(alignment.0, alignment.1);
        self.area.queue_draw();
    }

    fn apply_point(&self, x: f64, y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let dot = webcam_pad_dot_radius(width, height, self.scale) * 2.0;
        self.place((
            webcam_pad_alignment(x, width, dot),
            webcam_pad_alignment(y, height, dot),
        ));
    }

    fn draw(&self, widget: &DrawingArea, context: &gtk4::cairo::Context, width: i32, height: i32) {
        let width = f64::from(width.max(1));
        let height = f64::from(height.max(1));
        let light = widget_is_light(widget);
        rounded_rect(context, 0.0, 0.0, width, height, 10.0 * self.scale);
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.10);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.07);
        }
        context.fill_preserve().ok();
        // The second pass matches the idle fill of the sliders below, so the
        // field reads as the same surface as the rows it belongs to.
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        context.fill().ok();

        // The centre, so the field states its middle without a grid.
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
        }
        context.set_line_width(1.0);
        let tick = 5.0 * self.scale;
        for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
            context.move_to(width * 0.5 - dx * tick, height * 0.5 - dy * tick);
            context.line_to(width * 0.5 + dx * tick, height * 0.5 + dy * tick);
        }
        context.stroke().ok();

        let radius = webcam_pad_dot_radius(width, height, self.scale);
        let (alignment_x, alignment_y) = self.alignment.get();
        let centre_x = webcam_pad_offset(alignment_x, width, radius * 2.0) + radius;
        let centre_y = webcam_pad_offset(alignment_y, height, radius * 2.0) + radius;
        // One flat dot: the field already says where the frame's edges are, so
        // the marker only has to state the chosen point, and white on the dark
        // field or the field's own ink on the light one carries it.
        if light {
            context.set_source_rgb(0.11, 0.13, 0.16);
        } else {
            context.set_source_rgb(1.0, 1.0, 1.0);
        }
        context.arc(centre_x, centre_y, radius, 0.0, TAU);
        context.fill().ok();
    }
}

/// The dot's radius in the pad: a fixed 12 * scale, the size of the handle
/// the Motion pads paint, shrunk on a field too small to hold it so the dot's
/// edge still stops at the field's edge.
fn webcam_pad_dot_radius(pad_width: f64, pad_height: f64, scale: f64) -> f64 {
    (12.0 * scale).min(pad_width.min(pad_height) * 0.2)
}

/// Where the dot's bounding box starts for an alignment: the fraction of the
/// travel its diameter leaves, so 1.0 puts the dot's outside edge against the
/// pad's.
fn webcam_pad_offset(alignment: f64, pad_extent: f64, dot_extent: f64) -> f64 {
    let travel = pad_extent - dot_extent;
    if travel <= 0.0 {
        return 0.0;
    }
    alignment.clamp(0.0, 1.0) * travel
}

/// The alignment a pointer lands on: the dot's centre follows the pointer, so
/// a drag picks the dot up instead of pushing its corner.
fn webcam_pad_alignment(point: f64, pad_extent: f64, dot_extent: f64) -> f64 {
    let travel = pad_extent - dot_extent;
    if travel <= 0.0 {
        return 0.0;
    }
    ((point - dot_extent * 0.5) / travel).clamp(0.0, 1.0)
}

struct WebcamPositionContext {
    pad: WebcamPositionPad,
    horizontal: FilledSlider,
    vertical: FilledSlider,
}

impl WebcamPositionContext {
    fn refresh(&self, layer: &LayerSettings) {
        let alignment = layer.overlay_alignment();
        self.pad.refresh(alignment);
        self.horizontal.set_value(alignment.0 * 100.0);
        self.vertical.set_value(alignment.1 * 100.0);
    }
}

fn build_webcam_position(
    scale: f64,
    layer: &LayerSettings,
    frame_aspect: f64,
    move_axis: Rc<dyn Fn(bool, f64)>,
) -> (GtkBox, WebcamPositionContext) {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    let on_move = {
        let move_axis = move_axis.clone();
        Rc::new(move |x: f64, y: f64| {
            move_axis(true, x);
            move_axis(false, y);
        })
    };
    let alignment = layer.overlay_alignment();
    let pad = WebcamPositionPad::new(scale, frame_aspect, alignment, on_move);
    root.append(&pad.area);
    let horizontal_change = move_axis.clone();
    let horizontal = value_slider_control(
        &t("Horizontal"),
        0.0,
        100.0,
        alignment.0 * 100.0,
        1.0,
        0,
        scale,
        move |value| horizontal_change(true, value / 100.0),
    );
    horizontal
        .widget()
        .set_tooltip_text(Some(&t("Percentage of the free space around the bubble.")));
    let vertical = value_slider_control(
        &t("Vertical"),
        0.0,
        100.0,
        alignment.1 * 100.0,
        1.0,
        0,
        scale,
        move |value| move_axis(false, value / 100.0),
    );
    vertical
        .widget()
        .set_tooltip_text(Some(&t("Percentage of the free space around the bubble.")));
    root.append(&horizontal.widget());
    root.append(&vertical.widget());
    let context = WebcamPositionContext {
        pad,
        horizontal,
        vertical,
    };
    context.refresh(layer);
    (root, context)
}

fn webcam_shape_button(label: &str, height: i32) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-zoom-chip");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_size_request(-1, height);
    let text = Label::new(Some(&t(label)));
    text.set_single_line_mode(true);
    text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.set_max_width_chars(10);
    text.set_xalign(0.5);
    button.set_child(Some(&text));
    button
}

struct WebcamShapeContext {
    root: GtkBox,
    buttons: Vec<ToggleButton>,
    radius: FilledSlider,
}

impl WebcamShapeContext {
    fn refresh(&self, shape: Option<usize>, radius: f64) {
        for (index, button) in self.buttons.iter().enumerate() {
            button.set_active(shape == Some(index));
        }
        self.radius.set_value(radius * 100.0);
    }
}

fn build_webcam_shape(
    scale: f64,
    shape: Option<usize>,
    radius: f64,
    set_shape: Rc<dyn Fn(u8)>,
    set_radius: Rc<dyn Fn(f64)>,
) -> WebcamShapeContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_kicker(&t("SHAPE")));
    let gap = (4.0 * scale).round() as i32;
    let height = (28.0 * scale).round() as i32;
    let grid = Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_row_homogeneous(true);
    grid.set_column_spacing(gap as u32);
    grid.set_row_spacing(gap as u32);
    grid.set_hexpand(true);
    grid.set_halign(Align::Fill);
    let initial_radius = radius;
    let mut buttons = Vec::with_capacity(WEBCAM_SHAPES.len());
    let mut first = None::<ToggleButton>;
    for (index, label) in WEBCAM_SHAPES.iter().enumerate() {
        let button = webcam_shape_button(label, height);
        if let Some(first) = &first {
            button.set_group(Some(first));
        } else {
            first = Some(button.clone());
        }
        let set_shape = set_shape.clone();
        button.connect_clicked(move |button| {
            if !button.is_active() {
                button.set_active(true);
            }
            set_shape(index as u8);
        });
        grid.attach(&button, (index % 2) as i32, (index / 2) as i32, 1, 1);
        buttons.push(button);
    }
    root.append(&grid);
    let radius_slider = value_slider_control(
        &t("Radius"),
        0.0,
        100.0,
        initial_radius * 100.0,
        1.0,
        0,
        scale,
        move |value| set_radius(value / 100.0),
    );
    root.append(&radius_slider.widget());
    let context = WebcamShapeContext {
        root,
        buttons,
        radius: radius_slider,
    };
    context.refresh(shape, initial_radius);
    context
}

struct WebcamOverlayContext {
    root: GtkBox,
    position: WebcamPositionContext,
    size: FilledSlider,
    shape: WebcamShapeContext,
}

impl WebcamOverlayContext {
    fn refresh(&self, layer: &LayerSettings, frame_aspect: f64) {
        self.position.refresh(layer);
        self.size.set_value(layer.w * 100.0);
        self.shape
            .refresh(webcam_shape_index(layer, frame_aspect), layer.radius);
    }
}

fn build_webcam_overlay(
    scale: f64,
    layer: &LayerSettings,
    frame_aspect: f64,
    actions: &WebcamContextActions,
) -> WebcamOverlayContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_hint(&t("Shared by every overlay.")));
    root.append(&webcam_kicker(&t("POSITION")));
    let (position_box, position) =
        build_webcam_position(scale, layer, frame_aspect, actions.move_alignment.clone());
    root.append(&position_box);
    root.append(&webcam_kicker(&t("SIZE")));
    let resize = actions.resize.clone();
    let size = value_slider_control(
        &t("Size"),
        super::webcam::MIN_SIZE * 100.0,
        super::webcam::MAX_SIZE * 100.0,
        layer.w * 100.0,
        1.0,
        0,
        scale,
        move |value| resize(value),
    )
    .with_suffix("%");
    size.widget().set_tooltip_text(Some(&t(
        "The bubble's width, as a percentage of the frame.",
    )));
    root.append(&size.widget());
    let shape = build_webcam_shape(
        scale,
        webcam_shape_index(layer, frame_aspect),
        layer.radius,
        actions.set_shape.clone(),
        actions.set_radius.clone(),
    );
    root.append(&shape.root);
    let context = WebcamOverlayContext {
        root,
        position,
        size,
        shape,
    };
    context.refresh(layer, frame_aspect);
    context
}

struct WebcamSplitContext {
    root: GtkBox,
    first: ToggleButton,
    first_label: Label,
    second: ToggleButton,
    second_label: Label,
    width: FilledSlider,
}

impl WebcamSplitContext {
    fn refresh(&self, ratio: f64, flip: bool, portrait: bool) {
        let (first, second) = if portrait {
            ("Top", "Bottom")
        } else {
            ("Left", "Right")
        };
        self.first_label.set_text(&t(first));
        self.second_label.set_text(&t(second));
        if flip {
            self.first.set_active(true);
        } else {
            self.second.set_active(true);
        }
        self.width.set_value(ratio * 100.0);
    }
}

fn build_webcam_split(
    scale: f64,
    ratio: f64,
    flip: bool,
    portrait: bool,
    set_flip: Rc<dyn Fn(bool)>,
    set_ratio: Rc<dyn Fn(f64)>,
) -> WebcamSplitContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_hint(&t("Shared by every split layout.")));
    let segment = GtkBox::new(Orientation::Horizontal, 0);
    segment.add_css_class("recording-editor-webcam-scope");
    segment.set_hexpand(true);
    segment.set_homogeneous(true);
    let (first, first_label) = webcam_scope_button(&t("Left"));
    let (second, second_label) = webcam_scope_button(&t("Right"));
    first.set_hexpand(true);
    second.set_hexpand(true);
    second.set_group(Some(&first));
    segment.append(&first);
    segment.append(&second);

    root.append(&webcam_kicker(&t("CAMERA")));
    root.append(&segment);
    root.append(&webcam_kicker(&t("SCREEN SHARE")));
    let width = value_slider_control(
        &t("Width"),
        10.0,
        90.0,
        ratio * 100.0,
        1.0,
        0,
        scale,
        move |value| set_ratio(value / 100.0),
    )
    .with_suffix("%");
    width.widget().set_tooltip_text(Some(&t(
        "The screen's share of the split, as a percentage.",
    )));
    root.append(&width.widget());
    let set_first = set_flip.clone();
    first.connect_clicked(move |button| {
        if !button.is_active() {
            button.set_active(true);
        }
        set_first(true);
    });
    second.connect_clicked(move |button| {
        if !button.is_active() {
            button.set_active(true);
        }
        set_flip(false);
    });
    let context = WebcamSplitContext {
        root,
        first,
        first_label,
        second,
        second_label,
        width,
    };
    context.refresh(ratio, flip, portrait);
    context
}

struct WebcamCutoutContext {
    root: GtkBox,
    sides: Vec<ToggleButton>,
    horizontal: FilledSlider,
}

impl WebcamCutoutContext {
    fn refresh(&self, position: f64) {
        let step = webcam_alignment_step(position);
        for (index, button) in self.sides.iter().enumerate() {
            button.set_active(step == Some(index));
        }
        self.horizontal.set_value(position * 100.0);
    }
}

fn build_webcam_cutout(
    scale: f64,
    position: f64,
    set_position: Rc<dyn Fn(f64)>,
    move_position: Rc<dyn Fn(f64)>,
) -> WebcamCutoutContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_hint(&t("Shared by every cutout layout.")));
    root.append(&webcam_kicker(&t("POSITION")));
    let segment = GtkBox::new(Orientation::Horizontal, 0);
    segment.add_css_class("recording-editor-webcam-scope");
    segment.set_hexpand(true);
    segment.set_homogeneous(true);
    let mut sides = Vec::with_capacity(3);
    let mut first = None::<ToggleButton>;
    for (index, label) in ["Left", "Center", "Right"].iter().enumerate() {
        let (button, _) = webcam_scope_button(&t(label));
        button.set_hexpand(true);
        if let Some(first) = &first {
            button.set_group(Some(first));
        } else {
            first = Some(button.clone());
        }
        let set_position = set_position.clone();
        button.connect_clicked(move |button| {
            if !button.is_active() {
                button.set_active(true);
            }
            set_position(webcam_cutout_position(index));
        });
        segment.append(&button);
        sides.push(button);
    }
    root.append(&segment);
    let horizontal = value_slider_control(
        &t("Horizontal"),
        0.0,
        100.0,
        position * 100.0,
        1.0,
        0,
        scale,
        move |value| move_position(value / 100.0),
    );
    horizontal
        .widget()
        .set_tooltip_text(Some(&t("Percentage of the frame width.")));
    root.append(&horizontal.widget());
    let context = WebcamCutoutContext {
        root,
        sides,
        horizontal,
    };
    context.refresh(position);
    context
}

struct WebcamCameraContext {
    root: GtkBox,
    mirror: Switch,
}

impl WebcamCameraContext {
    fn refresh(&self, mirror: bool) {
        self.mirror.set_active(mirror);
    }
}

fn build_webcam_camera(
    scale: f64,
    mirror: bool,
    set_mirror: Rc<dyn Fn(bool)>,
) -> WebcamCameraContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_hint(&t("Shared by every camera layout.")));
    let (row, switch) = setting_switch_row(
        &t("Mirror camera"),
        &t("Flip the camera left to right, the way a mirror shows it."),
    );
    switch.set_active(mirror);
    switch.connect_state_set(move |_, active| {
        set_mirror(active);
        gtk4::glib::Propagation::Proceed
    });
    root.append(&row);
    WebcamCameraContext {
        root,
        mirror: switch,
    }
}

struct WebcamTransitionContext {
    root: GtkBox,
    duration: FilledSlider,
}

impl WebcamTransitionContext {
    fn refresh(&self, seconds: f64) {
        self.duration.set_value(seconds);
    }
}

fn build_webcam_transition(
    scale: f64,
    seconds: f64,
    set_duration: Rc<dyn Fn(f64)>,
) -> WebcamTransitionContext {
    let root = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.append(&webcam_hint(&t("Shared by every layout change.")));
    let duration = value_slider_control(
        &t("Duration"),
        0.0,
        WEBCAM_TRANSITION_MAX,
        seconds,
        0.05,
        2,
        scale,
        move |value| set_duration(value),
    );
    root.append(&duration.widget());
    WebcamTransitionContext { root, duration }
}

struct WebcamContextActions {
    move_alignment: Rc<dyn Fn(bool, f64)>,
    resize: Rc<dyn Fn(f64)>,
    set_shape: Rc<dyn Fn(u8)>,
    set_radius: Rc<dyn Fn(f64)>,
    set_split_flip: Rc<dyn Fn(bool)>,
    set_split_ratio: Rc<dyn Fn(f64)>,
    set_cutout_position: Rc<dyn Fn(f64)>,
    move_cutout_position: Rc<dyn Fn(f64)>,
    set_mirror: Rc<dyn Fn(bool)>,
    set_transition: Rc<dyn Fn(f64)>,
}

#[cfg(test)]
impl WebcamContextActions {
    fn idle() -> Self {
        Self {
            move_alignment: Rc::new(|_, _| {}),
            resize: Rc::new(|_| {}),
            set_shape: Rc::new(|_| {}),
            set_radius: Rc::new(|_| {}),
            set_split_flip: Rc::new(|_| {}),
            set_split_ratio: Rc::new(|_| {}),
            set_cutout_position: Rc::new(|_| {}),
            move_cutout_position: Rc::new(|_| {}),
            set_mirror: Rc::new(|_| {}),
            set_transition: Rc::new(|_| {}),
        }
    }
}

struct WebcamContext {
    root: GtkBox,
    overlay: WebcamOverlayContext,
    split: WebcamSplitContext,
    cutout: WebcamCutoutContext,
    camera: WebcamCameraContext,
    transition: WebcamTransitionContext,
}

impl WebcamContext {
    fn refresh(
        &self,
        layer: &LayerSettings,
        layouts: &LayoutSettings,
        frame_aspect: f64,
        controls: WebcamControlUse,
    ) {
        self.overlay.root.set_visible(controls.overlay);
        self.split.root.set_visible(controls.split);
        self.cutout.root.set_visible(controls.cutout);
        self.camera.root.set_visible(controls.mirror);
        self.transition.root.set_visible(controls.transitions);
        if controls.overlay {
            self.overlay.refresh(layer, frame_aspect);
        }
        if controls.split {
            self.split.refresh(
                layouts.split_ratio,
                layouts.split_flip,
                webcam_frame_aspect(frame_aspect) < 1.0,
            );
        }
        if controls.cutout {
            self.cutout.refresh(layouts.cutout_position_x);
        }
        if controls.mirror {
            self.camera.refresh(layer.mirror);
        }
        if controls.transitions {
            self.transition.refresh(layouts.transition_seconds);
        }
    }
}

fn build_webcam_context(
    scale: f64,
    layer: LayerSettings,
    layouts: &LayoutSettings,
    frame_aspect: f64,
    actions: WebcamContextActions,
) -> WebcamContext {
    let overlay = build_webcam_overlay(scale, &layer, frame_aspect, &actions);
    let split = build_webcam_split(
        scale,
        layouts.split_ratio,
        layouts.split_flip,
        webcam_frame_aspect(frame_aspect) < 1.0,
        actions.set_split_flip.clone(),
        actions.set_split_ratio.clone(),
    );
    let cutout = build_webcam_cutout(
        scale,
        layouts.cutout_position_x,
        actions.set_cutout_position.clone(),
        actions.move_cutout_position.clone(),
    );
    let camera = build_webcam_camera(scale, layer.mirror, actions.set_mirror.clone());
    let transition = build_webcam_transition(
        scale,
        layouts.transition_seconds,
        actions.set_transition.clone(),
    );
    let root = GtkBox::new(Orientation::Vertical, (10.0 * scale).round() as i32);
    root.set_hexpand(true);
    root.set_valign(Align::Start);
    root.append(&overlay.root);
    root.append(&split.root);
    root.append(&cutout.root);
    root.append(&camera.root);
    root.append(&transition.root);
    WebcamContext {
        root,
        overlay,
        split,
        cutout,
        camera,
        transition,
    }
}

/// Build the Webcam panel: the recording's default layout, the changes that
/// switch it while the composition plays, and the camera's shared placement.
/// Every control writes into the timeline, so the preview and the export read
/// the same layer and the same cue list.
fn build_webcam_panel(scale: f64, timeline: super::timeline::Timeline) -> (GtkBox, Rc<dyn Fn()>) {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = panel_header(&t("Webcam"));
    let reset = Button::with_label(&t("Reset"));
    reset.add_css_class("recording-editor-zoom-reset");
    reset.set_has_frame(false);
    reset.set_halign(Align::End);
    reset.set_tooltip_text(Some(&t(
        "Put the camera back where the recording captured it.",
    )));
    header.append(&reset);
    panel.append(&header);

    let body = panel_body();
    body.set_vexpand(true);
    body.set_valign(Align::Start);
    let syncing = Rc::new(Cell::new(false));
    let refresh_slot: WebcamRefreshSlot = Rc::new(RefCell::new(None));
    let initial = timeline.webcam_panel_state();
    let initial_layer = initial.layer.unwrap_or_default();
    let initial_layouts = initial.layouts.clone().unwrap_or_default();

    let empty = webcam_hint(&t("This recording has no camera track."));
    empty.set_visible(false);
    body.append(&empty);

    let error_hint = webcam_hint("");
    error_hint.set_visible(false);
    body.append(&error_hint);

    let editor = GtkBox::new(Orientation::Vertical, (8.0 * scale).round() as i32);
    editor.set_hexpand(true);
    editor.set_vexpand(true);
    body.append(&editor);

    // The layer's own switch sits above the layouts: off paints the screen
    // alone and leaves every layout change where it is.
    let (camera_row, camera_switch) = setting_switch_row(
        &t("Camera layer"),
        &t("Draw the camera over the screen. Off shows the screen alone and keeps your layout changes."),
    );
    camera_switch.set_active(initial_layer.enabled);
    camera_switch.connect_state_set({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |_, active| {
            if !syncing.get() {
                timeline.set_webcam_enabled(active);
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    editor.append(&camera_row);

    let rows = build_webcam_control_rows(scale);
    editor.append(&rows.root);

    let cards = build_webcam_layout_grid(scale, initial.frame_aspect);
    editor.append(&cards.root);

    let actions = WebcamContextActions {
        move_alignment: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |horizontal, value| {
                if syncing.get() {
                    return;
                }
                timeline.update_webcam(|layer| {
                    let (mut x, mut y) = layer.overlay_alignment();
                    if horizontal {
                        x = value;
                    } else {
                        y = value;
                    }
                    layer.set_overlay_alignment(x, y);
                });
            })
        },
        resize: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |value| {
                if !syncing.get() {
                    timeline.update_webcam(|layer| {
                        let aspect = layer.h / layer.w;
                        layer.resize_overlay(value / 100.0, aspect);
                    });
                }
            })
        },
        set_shape: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |shape| {
                if !syncing.get() {
                    timeline.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(shape));
                }
            })
        },
        set_radius: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |value| {
                if !syncing.get() {
                    timeline.update_webcam(|layer| layer.radius = value);
                }
            })
        },
        set_split_flip: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |flip| {
                if !syncing.get() {
                    timeline.update_webcam_layouts(|layouts| layouts.split_flip = flip);
                }
            })
        },
        set_split_ratio: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |value| {
                if !syncing.get() {
                    timeline.update_webcam_layouts_continuous(|layouts| {
                        layouts.split_ratio = value;
                    });
                }
            })
        },
        set_cutout_position: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |position| {
                if !syncing.get() {
                    timeline.update_webcam_layouts(|layouts| {
                        layouts.cutout_position_x = position;
                    });
                }
            })
        },
        move_cutout_position: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |position| {
                if !syncing.get() {
                    timeline.update_webcam_layouts_continuous(|layouts| {
                        layouts.cutout_position_x = position;
                    });
                }
            })
        },
        set_mirror: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |mirror| {
                if !syncing.get() {
                    timeline.update_webcam(|layer| layer.mirror = mirror);
                }
            })
        },
        set_transition: {
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            Rc::new(move |seconds| {
                if !syncing.get() {
                    timeline.update_webcam_layouts_continuous(|layouts| {
                        layouts.transition_seconds = seconds;
                    });
                }
            })
        },
    };
    let context = build_webcam_context(
        scale,
        initial_layer,
        &initial_layouts,
        initial.frame_aspect,
        actions,
    );
    editor.append(&context.root);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.add_css_class("video-editor-side-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));
    panel.append(&scroll);

    rows.default_scope.connect_clicked({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        let refresh_slot = refresh_slot.clone();
        move |button| {
            if !button.is_active() {
                button.set_active(true);
            }
            if syncing.get() {
                return;
            }
            if timeline.webcam_panel_state().selected.is_some() {
                timeline.clear_webcam_layout_cue_selection();
                request_refresh(&refresh_slot);
            }
        }
    });
    rows.change_scope.connect_clicked({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        move |button| {
            if !button.is_active() {
                button.set_active(true);
            }
            if syncing.get() {
                return;
            }
            if let Some((index, _)) = webcam_selected_cue(&timeline.webcam_panel_state()) {
                timeline.select_webcam_layout_cue(index);
            }
        }
    });
    rows.time_spin.connect_value_changed({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        let refresh_slot = refresh_slot.clone();
        move |spin| {
            if syncing.get() {
                return;
            }
            let state = timeline.webcam_panel_state();
            let Some((index, _)) = webcam_selected_cue(&state) else {
                return;
            };
            timeline.set_webcam_layout_cue_time(index, spin.value());
            let actual = timeline.webcam_panel_state();
            if let Some((_, cue)) = webcam_selected_cue(&actual) {
                syncing.set(true);
                spin.set_value(cue.at);
                syncing.set(false);
            }
            request_refresh(&refresh_slot);
        }
    });
    rows.delete_change.connect_clicked({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        let refresh_slot = refresh_slot.clone();
        move |_| {
            if syncing.get() {
                return;
            }
            if let Some((index, _)) = webcam_selected_cue(&timeline.webcam_panel_state()) {
                timeline.remove_webcam_layout_cue(index);
                request_refresh(&refresh_slot);
            }
        }
    });
    rows.add_change.connect_clicked({
        let timeline = timeline.clone();
        let syncing = syncing.clone();
        let refresh_slot = refresh_slot.clone();
        move |_| {
            if syncing.get() {
                return;
            }
            let state = timeline.webcam_panel_state();
            match state.cue_at_playhead {
                Some(index) => timeline.select_webcam_layout_cue(index),
                None => timeline.add_webcam_layout_cue(),
            }
            request_refresh(&refresh_slot);
        }
    });
    for (tile, card) in cards.tiles.iter().zip(WebcamLayoutCard::ALL) {
        tile.button.connect_clicked({
            let timeline = timeline.clone();
            let syncing = syncing.clone();
            let refresh_slot = refresh_slot.clone();
            move |button| {
                if !button.is_active() {
                    button.set_active(true);
                }
                if syncing.get() {
                    return;
                }
                timeline.set_webcam_editing_layout(card.layout(), card.overlay_shape());
                request_refresh(&refresh_slot);
            }
        });
    }
    reset.connect_clicked({
        let timeline = timeline.clone();
        move |_| timeline.reset_webcam()
    });

    let last = Rc::new(RefCell::new(None::<super::timeline::WebcamPanelState>));
    let refresh: Rc<dyn Fn()> = {
        let last = last.clone();
        let syncing = syncing.clone();
        let empty = empty.clone();
        let error_hint = error_hint.clone();
        let editor = editor.clone();
        let camera_switch = camera_switch.clone();
        let reset = reset.clone();
        let default_scope = rows.default_scope.clone();
        let change_scope = rows.change_scope.clone();
        let add_change = rows.add_change.clone();
        let delete_change = rows.delete_change.clone();
        let time_row = rows.time_row.clone();
        let time_spin = rows.time_spin.clone();
        let status = rows.status.clone();
        let cards = Rc::new(cards);
        let context = Rc::new(context);
        Rc::new(move || {
            let state = timeline.webcam_panel_state();
            let previous = last.borrow().clone();
            if previous.as_ref() == Some(&state) {
                return;
            }
            *last.borrow_mut() = Some(state.clone());
            syncing.set(true);

            match state.error.as_deref() {
                Some(error) => {
                    error_hint.set_text(&tfmt(
                        "The camera layer reported: {error}",
                        &[("error", error)],
                    ));
                    error_hint.set_visible(true);
                }
                None => error_hint.set_visible(false),
            }

            let (Some(layouts), Some(layer)) = (state.layouts.clone(), state.layer) else {
                empty.set_visible(true);
                editor.set_visible(false);
                reset.set_visible(false);
                syncing.set(false);
                return;
            };

            empty.set_visible(false);
            editor.set_visible(true);
            camera_switch.set_active(layer.enabled);
            reset.set_visible(true);
            reset.set_sensitive(true);

            let selected = webcam_selected_cue(&state);
            if let Some((_, cue)) = selected {
                default_scope.set_active(false);
                change_scope.set_sensitive(true);
                change_scope.set_active(true);
                change_scope.set_tooltip_text(Some(&t("Edit the selected layout change.")));
                // The tab keeps its own name; the field below holds the time.
                time_row.set_visible(true);
                delete_change.set_visible(true);
                time_spin.adjustment().set_upper(state.duration.max(0.1));
                time_spin.set_value(cue.at);
            } else {
                change_scope.set_sensitive(false);
                change_scope.set_active(false);
                default_scope.set_active(true);
                change_scope.set_tooltip_text(Some(&t(
                    "Select a layout change in the timeline to edit it.",
                )));
                // Nothing to rename: the tab is scope, not a value.
                time_row.set_visible(false);
                delete_change.set_visible(false);
            }

            if state
                .cue_at_playhead
                .is_some_and(|index| index < layouts.cues.len())
            {
                add_change.set_tooltip_text(Some(&t("Select the layout change at the playhead.")));
            } else {
                add_change.set_tooltip_text(Some(&t("Add a layout change at the playhead.")));
            }
            add_change.set_sensitive(state.duration > 0.0);

            let layout_name =
                t(webcam_layout_card(state.effective, Some(&layer), state.frame_aspect).label());
            // While a change is selected its field already shows the time, so
            // the line underneath only names the layout. Editing the default
            // keeps the clock, because then the field is not there.
            if selected.is_some() {
                status.set_text(&layout_name);
            } else {
                status.set_text(&tfmt(
                    "{time} · {layout}",
                    &[
                        ("time", &format_cue_clock(state.playhead)),
                        ("layout", &layout_name),
                    ],
                ));
            }

            let controls = webcam_editing_controls(&state);
            if webcam_panel_needs_rebuild(previous.as_ref(), &state) {
                cards.set_frame_aspect(state.frame_aspect);
                cards.set_active(webcam_layout_card(
                    webcam_editing_layout(&state),
                    Some(&layer),
                    state.frame_aspect,
                ));
                context.refresh(&layer, &layouts, state.frame_aspect, controls);
                if let Some((_, cue)) = selected {
                    time_spin.adjustment().set_upper(state.duration.max(0.1));
                    time_spin.set_value(cue.at);
                }
            }

            syncing.set(false);
        }) as Rc<dyn Fn()>
    };
    *refresh_slot.borrow_mut() = Some(refresh.clone());
    refresh();

    (panel, refresh)
}

/// A labelled switch row, the shape every panel uses for an on/off setting.
fn setting_switch_row(label: &str, tooltip: &str) -> (GtkBox, Switch) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-zoom-classic");
    row.set_hexpand(true);
    let text = Label::new(Some(label));
    text.add_css_class("recording-editor-zoom-classic-label");
    text.set_xalign(0.0);
    text.set_hexpand(true);
    let switch = Switch::new();
    switch.add_css_class("recording-editor-zoom-switch");
    switch.set_valign(Align::Center);
    switch.set_halign(Align::End);
    switch.set_tooltip_text(Some(tooltip));
    row.append(&text);
    row.append(&switch);
    (row, switch)
}

fn build_frames_panel(scale: f64) -> GtkBox {
    let body = panel_body();

    let kicker = Label::new(Some(&t("FRAME")));
    kicker.add_css_class("recording-editor-zoom-kicker");
    kicker.set_xalign(0.0);
    body.append(&kicker);

    let aspect = GtkBox::new(Orientation::Horizontal, 0);
    aspect.add_css_class("recording-editor-zoom-mode");
    aspect.set_homogeneous(true);
    let original = tab_button(&t("Original"), "recording-editor-zoom-mode-btn");
    let wide = tab_button(&t("16:9"), "recording-editor-zoom-mode-btn");
    let portrait = tab_button(&t("9:16"), "recording-editor-zoom-mode-btn");
    wide.set_group(Some(&original));
    portrait.set_group(Some(&original));
    original.set_active(true);
    aspect.append(&original);
    aspect.append(&wide);
    aspect.append(&portrait);
    body.append(&aspect);

    let hint = Label::new(Some(&t(
        "Frame changes keep the source footage centred in the preview.",
    )));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    body.append(&hint);

    let zoom_label = Label::new(Some(&t("ZOOM")));
    zoom_label.add_css_class("recording-editor-zoom-kicker");
    zoom_label.add_css_class("recording-editor-zoom-easing-kicker");
    zoom_label.set_xalign(0.0);
    body.append(&zoom_label);

    let zoom = GtkBox::new(Orientation::Horizontal, 6);
    zoom.add_css_class("recording-editor-zoom-chips");
    zoom.set_homogeneous(true);
    let zoom_1 = tab_button("1×", "recording-editor-zoom-chip");
    let zoom_15 = tab_button("1.5×", "recording-editor-zoom-chip");
    let zoom_2 = tab_button("2×", "recording-editor-zoom-chip");
    zoom_15.set_group(Some(&zoom_1));
    zoom_2.set_group(Some(&zoom_1));
    zoom_1.set_active(true);
    zoom.append(&zoom_1);
    zoom.append(&zoom_15);
    zoom.append(&zoom_2);
    body.append(&zoom);
    body.append(&value_slider_row(
        &t("Strength"),
        1.0,
        3.0,
        1.5,
        0.1,
        1,
        scale,
    ));

    let easing_label = Label::new(Some(&t("TRANSITION")));
    easing_label.add_css_class("recording-editor-zoom-kicker");
    easing_label.add_css_class("recording-editor-zoom-easing-kicker");
    easing_label.set_xalign(0.0);
    body.append(&easing_label);

    let easing = GtkBox::new(Orientation::Horizontal, 6);
    easing.set_homogeneous(true);
    let smooth = tab_button(&t("Smooth"), "recording-editor-zoom-easing-btn");
    let snappy = tab_button(&t("Snappy"), "recording-editor-zoom-easing-btn");
    let instant = tab_button(&t("Instant"), "recording-editor-zoom-easing-btn");
    snappy.set_group(Some(&smooth));
    instant.set_group(Some(&smooth));
    smooth.set_active(true);
    easing.append(&smooth);
    easing.append(&snappy);
    easing.append(&instant);
    body.append(&easing);
    body.append(&value_slider_row(
        &t("Transition"),
        0.0,
        1.2,
        0.45,
        0.05,
        2,
        scale,
    ));
    panel_frame(&t("Frames"), body)
}

fn panel_header(title: &str) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let label = Label::new(Some(title));
    label.add_css_class("recording-editor-zoom-title");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    header.append(&label);
    header
}

fn panel_frame(title: &str, body: GtkBox) -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);
    panel.append(&panel_header(title));

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.add_css_class("video-editor-side-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));
    panel.append(&scroll);
    panel
}

fn panel_body() -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.set_hexpand(true);
    body
}

fn tab_body() -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);
    body
}

fn tab_button(label: &str, class: &str) -> ToggleButton {
    let button = ToggleButton::with_label(label);
    button.add_css_class(class);
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn connect_tabs(stack: &Stack, tabs: [(&ToggleButton, &str); 3]) {
    for (tab, name) in tabs {
        let stack = stack.clone();
        let name = name.to_owned();
        tab.connect_toggled(move |tab| {
            if tab.is_active() {
                stack.set_visible_child_name(&name);
            }
        });
    }
}

fn value_slider_row(
    label: &str,
    min: f64,
    max: f64,
    initial: f64,
    step: f64,
    decimals: usize,
    scale: f64,
) -> DrawingArea {
    FilledSlider::new(label, min, max, initial, step, decimals, scale).widget()
}

/// A slider row whose value is reported as it is dragged, for controls the
/// preview reacts to immediately.
fn value_slider_row_with(
    label: &str,
    min: f64,
    max: f64,
    initial: f64,
    step: f64,
    decimals: usize,
    scale: f64,
    on_change: impl Fn(f64) + 'static,
) -> DrawingArea {
    value_slider_control(label, min, max, initial, step, decimals, scale, on_change).widget()
}

fn value_slider_control(
    label: &str,
    min: f64,
    max: f64,
    initial: f64,
    step: f64,
    decimals: usize,
    scale: f64,
    on_change: impl Fn(f64) + 'static,
) -> FilledSlider {
    let slider = FilledSlider::new(label, min, max, initial, step, decimals, scale);
    slider.on_change(Rc::new(on_change));
    slider
}

fn cursor_slider(
    label: &str,
    min: f64,
    max: f64,
    initial: f64,
    step: f64,
    decimals: usize,
    scale: f64,
    cursor: super::cursor::CursorModel,
    repaint: Rc<dyn Fn()>,
    update: impl Fn(&mut CursorSettings, f64) + 'static,
) -> FilledSlider {
    value_slider_control(
        label,
        min,
        max,
        initial,
        step,
        decimals,
        scale,
        move |value| {
            update_cursor_settings(&cursor, &repaint, |settings| update(settings, value));
        },
    )
}

fn format_value(value: f64, decimals: usize) -> String {
    match decimals {
        0 => format!("{value:.0}"),
        1 => format!("{value:.1}"),
        _ => format!("{value:.2}"),
    }
}

#[derive(Clone)]
struct FilledSlider {
    area: DrawingArea,
    value: Rc<Cell<f64>>,
    min: f64,
    max: f64,
    step: f64,
    decimals: usize,
    scale: f64,
    hovered: Rc<Cell<bool>>,
    dragging: Rc<Cell<bool>>,
    suffix: &'static str,
    on_change: Rc<RefCell<Option<Rc<dyn Fn(f64)>>>>,
}

impl FilledSlider {
    fn new(
        label: &str,
        min: f64,
        max: f64,
        initial: f64,
        step: f64,
        decimals: usize,
        scale: f64,
    ) -> Self {
        let area = DrawingArea::new();
        area.add_css_class("recording-editor-fill-slider");
        area.set_hexpand(true);
        area.set_size_request(-1, (32.0 * scale).round() as i32);
        let slider = Self {
            area: area.clone(),
            value: Rc::new(Cell::new(initial.clamp(min, max))),
            min,
            max: max.max(min + 1e-9),
            step,
            decimals,
            scale,
            hovered: Rc::new(Cell::new(false)),
            dragging: Rc::new(Cell::new(false)),
            suffix: "",
            on_change: Rc::new(RefCell::new(None)),
        };
        area.set_draw_func({
            let slider = slider.clone();
            let label = label.to_owned();
            move |widget, context, width, height| {
                slider.draw(widget, context, width, height, &label);
            }
        });

        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let slider = slider.clone();
            move |gesture, x, _| {
                slider.dragging.set(true);
                slider.area.queue_draw();
                slider.apply_x(gesture, x);
            }
        });
        drag.connect_drag_update({
            let slider = slider.clone();
            move |gesture, dx, _| {
                let Some((start, _)) = gesture.start_point() else {
                    return;
                };
                slider.apply_x(gesture, start + dx);
            }
        });
        drag.connect_drag_end({
            let slider = slider.clone();
            move |_, _, _| {
                slider.dragging.set(false);
                slider.area.queue_draw();
            }
        });
        area.add_controller(drag);

        let motion = EventControllerMotion::new();
        motion.connect_enter({
            let slider = slider.clone();
            move |_, _, _| {
                slider.hovered.set(true);
                slider.area.queue_draw();
            }
        });
        motion.connect_leave({
            let slider = slider.clone();
            move |_| {
                slider.hovered.set(false);
                slider.area.queue_draw();
            }
        });
        area.add_controller(motion);
        area.set_cursor(gdk::Cursor::from_name("ew-resize", None).as_ref());
        slider
    }

    fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    fn set_value(&self, value: f64) {
        self.value.set(value.clamp(self.min, self.max));
        self.area.queue_draw();
    }

    fn with_suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    fn on_change(&self, callback: Rc<dyn Fn(f64)>) {
        *self.on_change.borrow_mut() = Some(callback);
    }

    fn apply_x(&self, gesture: &GestureDrag, x: f64) {
        let width = gesture
            .widget()
            .map(|widget| widget.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let t = (x / width).clamp(0.0, 1.0);
        let raw = self.min + t * (self.max - self.min);
        let value = if self.step > 1e-9 {
            ((raw - self.min) / self.step).round() * self.step + self.min
        } else {
            raw
        };
        let value = value.clamp(self.min, self.max);
        if (self.value.get() - value).abs() < 1e-9 {
            return;
        }
        self.value.set(value);
        let callback = self.on_change.borrow().clone();
        if let Some(callback) = callback {
            callback(value);
        }
        self.area.queue_draw();
    }

    fn draw(
        &self,
        widget: &DrawingArea,
        context: &gtk4::cairo::Context,
        width: i32,
        height: i32,
        label: &str,
    ) {
        let width = f64::from(width);
        let height = f64::from(height);
        if width < 8.0 || height < 8.0 {
            return;
        }
        let light = widget_is_light(widget);
        let hot = self.hovered.get() || self.dragging.get();
        let active = self.dragging.get();
        let progress =
            ((self.value.get() - self.min) / (self.max - self.min).max(1e-9)).clamp(0.0, 1.0);
        let radius = 5.0 * self.scale;
        let (track_alpha, fill_alpha, text_alpha, handle_alpha) = if light {
            if active {
                (0.16, 0.28, 0.88, 0.95)
            } else if hot {
                (0.13, 0.22, 0.82, 0.92)
            } else {
                (0.10, 0.16, 0.72, 0.82)
            }
        } else if active {
            (0.12, 0.22, 0.92, 1.0)
        } else if hot {
            (0.10, 0.18, 0.86, 0.95)
        } else {
            (0.07, 0.12, 0.72, 0.82)
        };

        rounded_rect(context, 0.0, 0.0, width, height, radius);
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, track_alpha);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, track_alpha);
        }
        let _ = context.fill();

        let fill_width = progress * width;
        if fill_width > 0.0 {
            rounded_rect(context, 0.0, 0.0, fill_width, height, radius);
            if light {
                context.set_source_rgba(0.11, 0.13, 0.16, fill_alpha);
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, fill_alpha);
            }
            let _ = context.fill();
        }

        context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
        context.set_font_size(12.0 * self.scale);
        if light {
            context.set_source_rgba(0.15, 0.16, 0.18, text_alpha);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, text_alpha);
        }
        context.move_to(14.0 * self.scale, height * 0.66);
        let _ = context.show_text(label);

        let value_text = format!(
            "{}{}",
            format_value(self.value.get(), self.decimals),
            self.suffix
        );
        if let Ok(extents) = context.text_extents(&value_text) {
            context.move_to(width - 14.0 * self.scale - extents.width(), height * 0.66);
            let _ = context.show_text(&value_text);
        }

        let handle_x = (fill_width - radius).clamp(radius, (width - radius).max(radius));
        context.set_line_width(if hot { 2.0 } else { 1.5 });
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        if light {
            context.set_source_rgba(0.15, 0.16, 0.18, handle_alpha);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, handle_alpha);
        }
        context.move_to(handle_x, height * 0.28);
        context.line_to(handle_x, height * 0.72);
        let _ = context.stroke();
    }
}

fn widget_is_light(widget: &impl IsA<gtk4::Widget>) -> bool {
    let mut current = Some(widget.clone().upcast::<gtk4::Widget>());
    while let Some(widget) = current {
        if widget.has_css_class("editor-theme-light") {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn cursor_card(theme: CursorTheme, scale: f64) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-cursor-card");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let body = GtkBox::new(Orientation::Vertical, (6.0 * scale).round() as i32);
    body.set_halign(Align::Fill);
    let preview = DrawingArea::new();
    preview.add_css_class("recording-editor-cursor-preview");
    preview.set_content_width((72.0 * scale).round() as i32);
    preview.set_content_height((56.0 * scale).round() as i32);
    preview.set_hexpand(true);
    let settings = CursorSettings {
        theme,
        ..CursorSettings::default()
    };
    preview.set_draw_func(move |_, context, width, height| {
        cursor_sprite::draw_centered(context, f64::from(width), f64::from(height), settings, 1.0);
    });
    let label = Label::new(Some(&t(theme.label())));
    label.add_css_class("recording-editor-cursor-card-label");
    label.set_xalign(0.5);
    body.append(&preview);
    body.append(&label);
    card.set_child(Some(&body));
    card
}

fn click_effect_card(effect: ClickEffect, scale: f64) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-click-effect-card");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, (4.0 * scale).round() as i32);
    column.set_halign(Align::Center);
    column.set_valign(Align::Center);
    let preview = DrawingArea::new();
    preview.set_content_width((40.0 * scale).round() as i32);
    preview.set_content_height((30.0 * scale).round() as i32);
    preview.set_can_target(false);
    preview.set_draw_func(move |widget, context, width, height| {
        draw_click_effect_icon(widget, context, f64::from(width), f64::from(height), effect);
    });
    column.append(&preview);
    let name = Label::new(Some(&t(effect.label())));
    name.add_css_class("recording-editor-click-effect-label");
    name.set_xalign(0.5);
    column.append(&name);
    card.set_child(Some(&column));
    card
}

fn draw_click_effect_icon(
    widget: &DrawingArea,
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    effect: ClickEffect,
) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let light = widget_is_light(widget);
    let (r, g, b) = if light {
        (0.15, 0.16, 0.18)
    } else {
        (1.0, 1.0, 1.0)
    };
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let s = width.min(height) / 48.0;
    match effect {
        ClickEffect::None => {
            let s = width.min(height) / 40.0;
            context.set_line_width(1.8 * s);
            context.set_source_rgba(r, g, b, 0.75);
            context.arc(cx, cy, 11.5 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.set_line_width(2.2 * s);
            context.set_source_rgba(r, g, b, 0.92);
            context.move_to(cx - 7.5 * s, cy + 7.5 * s);
            context.line_to(cx + 7.5 * s, cy - 7.5 * s);
            let _ = context.stroke();
        }
        ClickEffect::Spotlight => {
            context.set_line_width(1.5 * s);
            context.set_source_rgba(r, g, b, 0.3);
            context.arc(cx, cy, 13.5 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.set_line_width(1.7 * s);
            context.set_source_rgba(r, g, b, 0.56);
            context.arc(cx, cy, 9.75 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
        }
        ClickEffect::Ripple => {
            context.set_line_width(2.0 * s);
            context.set_source_rgba(r, g, b, 0.72);
            context.arc(cx, cy, 13.0 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
        }
        ClickEffect::Circle => {
            context.set_source_rgba(r, g, b, 0.22);
            context.arc(cx, cy, 12.0 * s, 0.0, PI * 2.0);
            let _ = context.fill();
            context.set_line_width(1.6 * s);
            context.set_source_rgba(r, g, b, 0.72);
            context.arc(cx, cy, 12.0 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
        }
        ClickEffect::Echo => {
            context.set_line_width(1.8 * s);
            context.set_source_rgba(r, g, b, 0.72);
            context.arc(cx, cy, 9.0 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.set_line_width(1.5 * s);
            context.set_source_rgba(r, g, b, 0.4);
            context.arc(cx, cy, 14.5 * s, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.set_source_rgba(r, g, b, 0.22);
            context.arc(cx, cy, 4.25 * s, 0.0, PI * 2.0);
            let _ = context.fill();
        }
    }
}

fn draw_click_live_preview(
    widget: &DrawingArea,
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    settings: CursorSettings,
    t: f64,
) {
    let settings = settings.clamped();
    let light = widget_is_light(widget);
    rounded_rect(context, 0.0, 0.0, width, height, 10.0);
    if light {
        context.set_source_rgba(0.11, 0.13, 0.16, 0.08);
    } else {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.05);
    }
    let _ = context.fill();
    let (hotspot_x, hotspot_y) = cursor_sprite::centered_preview_hotspot(width, height, settings);
    let duration = settings.click_window_seconds().max(0.2);
    let local = t % (duration * 1.35);
    if local <= duration {
        cursor_sprite::draw_click(
            context,
            hotspot_x,
            hotspot_y,
            (local / duration).clamp(0.0, 1.0),
            settings,
            1.0,
            width,
        );
    }
    cursor_sprite::draw_centered(context, width, height, settings, 0.95);
}

fn preset_card(title: &str, hint: &str, scale: f64) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-motion-preset");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let body = GtkBox::new(Orientation::Vertical, (3.0 * scale).round() as i32);
    body.set_valign(Align::Center);
    let title = Label::new(Some(title));
    title.add_css_class("recording-editor-motion-preset-label");
    title.set_xalign(0.5);
    let hint = Label::new(Some(hint));
    hint.add_css_class("recording-editor-motion-preset-hint");
    hint.set_xalign(0.5);
    body.append(&title);
    body.append(&hint);
    card.set_child(Some(&body));
    card
}

/// Pick a background picture and hand its path to `picked`.
///
/// Pictures only: the filter lists the formats the bundled gdk-pixbuf loaders
/// decode, and the extension is checked again on the way out because a file
/// chooser will take a typed path whatever the filter says.
fn choose_background_image(window: &ApplicationWindow, picked: impl Fn(PathBuf) + 'static) {
    let chooser = FileChooserNative::new(
        Some(&t("Select background image")),
        Some(window),
        FileChooserAction::Open,
        Some(&t("Select")),
        Some(&t("Cancel")),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&t("Images")));
    for pattern in [
        "*.png", "*.PNG", "*.jpg", "*.JPG", "*.jpeg", "*.JPEG", "*.bmp", "*.BMP",
    ] {
        filter.add_pattern(pattern);
    }
    chooser.set_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response == ResponseType::Accept {
            if let Some(path) = chooser.file().and_then(|file| file.path()) {
                if is_picture(&path) {
                    picked(path);
                }
            }
        }
        chooser.hide();
    });
    chooser.show();
}

fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp"
            )
        })
}

fn draw_wallpaper_fallback(context: &gtk4::cairo::Context, width: i32, height: i32) {
    let width = f64::from(width);
    let height = f64::from(height);
    rounded_rect(context, 0.5, 0.5, width - 1.0, height - 1.0, 8.0);
    context.set_source_rgb(0.18, 0.20, 0.24);
    let _ = context.fill();
}

fn draw_wallpaper_thumbnail(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    thumbnail: &gtk4::gdk_pixbuf::Pixbuf,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    rounded_rect(context, 0.0, 0.0, width, height, 7.0);
    context.clip();
    let source_width = f64::from(thumbnail.width().max(1));
    let source_height = f64::from(thumbnail.height().max(1));
    let scale = (width / source_width).max(height / source_height);
    let draw_width = source_width * scale;
    let draw_height = source_height * scale;
    context.translate((width - draw_width) * 0.5, (height - draw_height) * 0.5);
    context.scale(scale, scale);
    context.set_source_pixbuf(thumbnail, 0.0, 0.0);
    let _ = context.paint();
}

fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let right = x + width;
    let bottom = y + height;
    context.new_sub_path();
    context.arc(right - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    context.arc(right - radius, bottom - radius, radius, 0.0, FRAC_PI_2);
    context.arc(x + radius, bottom - radius, radius, FRAC_PI_2, PI);
    context.arc(x + radius, y + radius, radius, PI, PI * 1.5);
    context.close_path();
}

pub(super) fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round() as i32;
    let css = format!(
        r#"
        .video-editor-side-scroll {{
            background: transparent;
        }}
        .recording-editor-custom-popover {{
            min-width: {custom_popover_min_w}px;
        }}
        .recording-editor-custom-body {{
            min-height: {custom_body_min_h}px;
        }}
        .recording-editor-gradient-picker-card {{
            min-width: {custom_popover_min_w}px;
        }}
        /* The row's corners follow the chip and hex inside it, and the list keeps
         * the card's own radius clear at the bottom: at the shared 6px inset the
         * card's 10px corner curve cut into the last row's corners. */
        .recording-editor-gradient-step {{
            border-radius: {stop_row_radius}px;
        }}
        .recording-editor-gradient-steps {{
            margin: 0 8px {stop_list_bottom}px 8px;
        }}
        button.recording-editor-gradient-remove {{
            min-width: {stop_remove_size}px;
            min-height: {stop_remove_size}px;
            padding: 0;
            border: none;
            border-radius: 6px;
            background: transparent;
            color: alpha(white, 0.50);
        }}
        button.recording-editor-gradient-remove:hover {{
            background: alpha(white, 0.10);
            color: alpha(white, 0.92);
        }}
        .recording-editor-gradient-editor {{
            min-width: {custom_gradient_editor_min_w}px;
        }}
        .recording-editor-custom-value-row {{
            min-height: {custom_value_h}px;
        }}
        .video-editor-wallpaper-scroll {{
            margin: {grid_top}px 0 {grid_bottom}px 0;
        }}
        .video-editor-wallpaper-grid {{
            padding: 1px;
        }}
        button.video-editor-wallpaper-card {{
            min-height: {card_h}px;
            padding: 0;
            border: 2px solid alpha(white, 0.10);
            box-shadow: none;
            border-radius: {card_radius}px;
            background: alpha(white, 0.04);
        }}
        button.video-editor-wallpaper-card:hover {{
            border-color: alpha(white, 0.26);
        }}
        button.video-editor-wallpaper-card:checked,
        button.video-editor-wallpaper-card:checked:hover {{
            border: 2px solid #ff670d;
            box-shadow: 0 0 0 1px alpha(#ff670d, 0.30);
        }}
        .editor-theme-light button.video-editor-wallpaper-card {{
            border-color: alpha(#111827, 0.10);
            background: alpha(#111827, 0.03);
        }}
        .video-editor-color-grid {{
            margin: {grid_top}px 0 {color_grid_bottom}px 0;
        }}
        button.video-editor-color-swatch {{
            min-height: {swatch_h}px;
            padding: 3px;
            border: 1px solid alpha(white, 0.10);
            border-radius: {swatch_radius}px;
            background: alpha(white, 0.04);
        }}
        button.video-editor-color-swatch:checked,
        button.video-editor-color-swatch:checked:hover {{
            border: 2px solid #ff670d;
            background: alpha(#ff670d, 0.12);
        }}
        .video-editor-value-row {{
            min-height: {value_h}px;
            margin: {value_gap}px 0 0 0;
        }}
        .video-editor-value-number {{
            min-width: {number_w}px;
            color: alpha(white, 0.48);
            font-size: {value_text}px;
            font-variant-numeric: tabular-nums;
        }}
        .editor-theme-light .video-editor-value-number {{
            color: alpha(#111827, 0.52);
        }}
        .video-editor-secondary-button {{
            min-height: {secondary_h}px;
            padding: 0 {secondary_x}px;
            border-radius: {secondary_radius}px;
            background: alpha(white, 0.08);
            color: alpha(white, 0.72);
            font-size: {secondary_text}px;
            font-weight: 500;
        }}
        button.recording-editor-webcam-card {{
            min-height: {webcam_card_h}px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
        }}
        button.recording-editor-webcam-card:hover,
        button.recording-editor-webcam-card:active,
        button.recording-editor-webcam-card:checked,
        button.recording-editor-webcam-card:checked:hover,
        button.recording-editor-webcam-card:checked:active {{
            background: transparent;
            border: none;
            box-shadow: none;
        }}
        .recording-editor-webcam-card-label {{
            color: alpha(white, 0.62);
            font-size: {value_text}px;
            font-weight: 500;
        }}
        button.recording-editor-webcam-card:hover .recording-editor-webcam-card-label {{
            color: alpha(white, 0.82);
        }}
        button.recording-editor-webcam-card:checked .recording-editor-webcam-card-label,
        button.recording-editor-webcam-card:checked:hover .recording-editor-webcam-card-label {{
            color: #f0a07a;
        }}
        .recording-editor-webcam-scope {{
            padding: {webcam_scope_pad}px;
            border-radius: {webcam_scope_radius}px;
            background: alpha(white, 0.06);
        }}
        button.recording-editor-webcam-scope-btn {{
            min-height: {webcam_scope_h}px;
            padding: 0 {webcam_scope_x}px;
            border: none;
            border-radius: {webcam_scope_btn_radius}px;
            background: transparent;
            color: alpha(white, 0.62);
            font-size: {webcam_scope_text}px;
            font-weight: 500;
            box-shadow: none;
        }}
        button.recording-editor-webcam-scope-btn label {{
            color: inherit;
            font-size: {webcam_scope_text}px;
            font-weight: 500;
        }}
        button.recording-editor-webcam-scope-btn:hover {{
            background: alpha(white, 0.06);
            color: alpha(white, 0.90);
        }}
        button.recording-editor-webcam-scope-btn:checked,
        button.recording-editor-webcam-scope-btn:checked:hover {{
            background: alpha(white, 0.10);
            color: alpha(white, 0.96);
        }}
        button.recording-editor-webcam-scope-btn:checked label,
        button.recording-editor-webcam-scope-btn:checked:hover label {{
            color: inherit;
        }}
        button.recording-editor-webcam-scope-btn:disabled {{
            opacity: 0.35;
        }}
        .recording-editor-webcam-time {{
            min-height: {webcam_scope_h}px;
            padding: 0 2px;
            border-radius: {webcam_scope_btn_radius}px;
            background: alpha(white, 0.06);
            color: alpha(white, 0.92);
            font-size: {value_text}px;
            font-variant-numeric: tabular-nums;
        }}
        .recording-editor-webcam-time entry {{
            min-height: {webcam_scope_h}px;
            background: transparent;
            color: inherit;
            font-size: {value_text}px;
            box-shadow: none;
        }}
        .recording-editor-webcam-time button {{
            min-width: {webcam_scope_h}px;
            background: transparent;
            color: alpha(white, 0.55);
            box-shadow: none;
        }}
        .recording-editor-webcam-time button:hover {{
            background: alpha(white, 0.10);
            color: alpha(white, 0.94);
        }}
        button.recording-editor-webcam-icon {{
            min-width: {webcam_icon}px;
            min-height: {webcam_icon}px;
            padding: 0;
            border: none;
            border-radius: {webcam_icon_radius}px;
            background: transparent;
            color: alpha(white, 0.58);
            box-shadow: none;
        }}
        button.recording-editor-webcam-icon image {{
            color: inherit;
        }}
        button.recording-editor-webcam-icon:hover {{
            background: alpha(white, 0.08);
            color: alpha(white, 0.92);
        }}
        .recording-editor-webcam-kicker {{
            color: alpha(white, 0.55);
            font-size: {webcam_kicker_text}px;
            font-weight: 600;
            letter-spacing: 0.8px;
        }}
        .recording-editor-webcam-status {{
            color: alpha(white, 0.50);
            font-size: {webcam_status_text}px;
            font-variant-numeric: tabular-nums;
        }}
        .recording-editor-webcam-value {{
            color: alpha(white, 0.55);
            font-size: {value_text}px;
            font-variant-numeric: tabular-nums;
        }}
        .editor-theme-light .recording-editor-webcam-card-label {{
            color: alpha(#1d2129, 0.62);
        }}
        .editor-theme-light button.recording-editor-webcam-card:hover .recording-editor-webcam-card-label {{
            color: alpha(#1d2129, 0.82);
        }}
        .editor-theme-light button.recording-editor-webcam-card:checked .recording-editor-webcam-card-label,
        .editor-theme-light button.recording-editor-webcam-card:checked:hover .recording-editor-webcam-card-label {{
            color: #ff670d;
        }}
        .editor-theme-light .recording-editor-webcam-scope {{
            background: alpha(#111827, 0.06);
        }}
        .editor-theme-light button.recording-editor-webcam-scope-btn {{
            color: alpha(#1d2129, 0.62);
        }}
        .editor-theme-light button.recording-editor-webcam-scope-btn:hover {{
            background: alpha(#111827, 0.06);
            color: alpha(#1d2129, 0.90);
        }}
        .editor-theme-light button.recording-editor-webcam-scope-btn:checked,
        .editor-theme-light button.recording-editor-webcam-scope-btn:checked:hover {{
            background: alpha(#111827, 0.10);
            color: alpha(#1d2129, 0.96);
        }}
        .editor-theme-light .recording-editor-webcam-time {{
            background: alpha(#111827, 0.06);
            color: alpha(#1d2129, 0.92);
        }}
        .editor-theme-light .recording-editor-webcam-time entry {{
            color: inherit;
        }}
        .editor-theme-light .recording-editor-webcam-time button {{
            color: alpha(#1d2129, 0.55);
        }}
        .editor-theme-light button.recording-editor-webcam-icon {{
            color: alpha(#1d2129, 0.58);
        }}
        .editor-theme-light button.recording-editor-webcam-icon:hover {{
            background: alpha(#111827, 0.08);
            color: alpha(#1d2129, 0.92);
        }}
        .editor-theme-light .recording-editor-webcam-kicker {{
            color: alpha(#1d2129, 0.55);
        }}
        .editor-theme-light .recording-editor-webcam-status {{
            color: alpha(#1d2129, 0.50);
        }}
        .editor-theme-light .recording-editor-webcam-value {{
            color: alpha(#1d2129, 0.55);
        }}
        "#,
        grid_top = px(6.0),
        custom_popover_min_w = px(184.0),
        stop_row_radius = px(8.0),
        stop_list_bottom = px(10.0),
        stop_remove_size = px(22.0),
        custom_body_min_h = px(184.0),
        custom_gradient_editor_min_w = px(182.0),
        custom_value_h = px(30.0),
        grid_bottom = px(14.0),
        card_h = px(56.0),
        card_radius = px(9.0),
        swatch_h = px(42.0),
        swatch_radius = px(9.0),
        color_grid_bottom = px(14.0),
        value_h = px(34.0),
        value_gap = px(6.0),
        number_w = px(38.0),
        value_text = px(11.0),
        secondary_h = px(32.0),
        secondary_x = px(14.0),
        secondary_radius = px(8.0),
        secondary_text = px(12.0),
        webcam_card_h = px(62.0),
        webcam_scope_pad = px(2.0),
        webcam_scope_radius = px(7.0),
        webcam_scope_h = px(24.0),
        webcam_scope_x = px(7.0),
        webcam_scope_btn_radius = px(5.0),
        webcam_scope_text = px(11.0),
        webcam_icon = px(28.0),
        webcam_icon_radius = px(7.0),
        webcam_kicker_text = px(10.0),
        webcam_status_text = px(11.0),
    );
    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::super::timeline::WebcamPanelState;
    use super::super::webcam::{
        aspect_for_shape, radius_for_shape, CameraLayout, LayerSettings, LayoutCue, LayoutSettings,
    };
    use super::{
        build_webcam_context, build_webcam_control_rows, build_webcam_layout_grid,
        format_cue_clock, format_value, webcam_content_width, webcam_cutout_position,
        webcam_editing_controls, webcam_editing_layout, webcam_layout_card, webcam_pad_alignment,
        webcam_pad_dot_radius, webcam_pad_offset, webcam_panel_needs_rebuild,
        webcam_preview_aspect, webcam_selected_cue, webcam_selection_reveal, webcam_shape_index,
        WebcamContextActions, WebcamControlUse, WebcamLayoutCard,
    };
    use gtk4::prelude::*;
    use gtk4::Orientation;

    const WIDE: f64 = 16.0 / 9.0;

    fn cue(at: f64, layout: CameraLayout) -> LayoutCue {
        LayoutCue { at, layout }
    }

    fn panel_state(
        default_layout: CameraLayout,
        cues: Vec<LayoutCue>,
        selected: Option<usize>,
    ) -> WebcamPanelState {
        WebcamPanelState {
            layer: Some(LayerSettings::default()),
            layouts: Some(LayoutSettings {
                default_layout,
                cues,
                ..LayoutSettings::default()
            }),
            playhead: 0.0,
            duration: 20.0,
            selected,
            selection_epoch: 0,
            playing: false,
            effective: default_layout,
            cue_at_playhead: None,
            error: None,
            frame_aspect: WIDE,
        }
    }

    fn shaped(shape: u8) -> LayerSettings {
        LayerSettings {
            radius: radius_for_shape(shape),
            h: 0.2 * WIDE * aspect_for_shape(shape),
            ..LayerSettings::default()
        }
    }

    fn controls_for(card: WebcamLayoutCard) -> WebcamControlUse {
        let layout = card.layout();
        let camera = matches!(
            layout,
            CameraLayout::CameraOverlay | CameraLayout::SplitScreen | CameraLayout::CameraCutout
        );
        WebcamControlUse {
            overlay: layout == CameraLayout::CameraOverlay,
            split: layout == CameraLayout::SplitScreen,
            cutout: layout == CameraLayout::CameraCutout,
            mirror: camera,
            transitions: camera,
        }
    }

    fn measure_width(widget: &impl IsA<gtk4::Widget>) -> (i32, i32) {
        let (minimum, natural, _, _) = widget.measure(Orientation::Horizontal, -1);
        (minimum, natural)
    }

    #[test]
    fn value_labels_use_the_requested_precision() {
        assert_eq!(format_value(24.4, 0), "24");
        assert_eq!(format_value(0.35, 1), "0.3");
        assert_eq!(format_value(0.456, 2), "0.46");
    }

    #[test]
    fn layout_change_times_read_as_a_clock() {
        assert_eq!(format_cue_clock(0.0), "0:00.0");
        assert_eq!(format_cue_clock(12.34), "0:12.3");
        assert_eq!(format_cue_clock(59.96), "1:00.0");
        assert_eq!(format_cue_clock(125.0), "2:05.0");
    }

    #[test]
    fn six_cards_keep_the_chooser_order_and_cover_the_layouts() {
        let labels: Vec<&str> = WebcamLayoutCard::ALL
            .iter()
            .map(|card| card.label())
            .collect();
        assert_eq!(
            labels,
            vec!["Screen", "Camera", "Bubble", "Circle", "Split", "Cutout"]
        );

        let layouts: Vec<CameraLayout> = WebcamLayoutCard::ALL
            .iter()
            .map(|card| card.layout())
            .collect();
        assert_eq!(
            layouts,
            vec![
                CameraLayout::ScreenOnly,
                CameraLayout::FullscreenCamera,
                CameraLayout::CameraOverlay,
                CameraLayout::CameraOverlay,
                CameraLayout::SplitScreen,
                CameraLayout::CameraCutout,
            ]
        );
        assert_eq!(WebcamLayoutCard::Bubble.overlay_shape(), Some(2));
        assert_eq!(WebcamLayoutCard::Circle.overlay_shape(), Some(0));
        assert_eq!(WebcamLayoutCard::Screen.overlay_shape(), None);
        assert_eq!(WebcamLayoutCard::Camera.overlay_shape(), None);
        assert_eq!(WebcamLayoutCard::Split.overlay_shape(), None);
        assert_eq!(WebcamLayoutCard::Cutout.overlay_shape(), None);
    }

    #[test]
    fn circle_and_bubble_follow_the_frame_aspect() {
        let circle = shaped(0);
        assert_eq!(webcam_shape_index(&circle, WIDE), Some(0));
        assert_eq!(
            webcam_layout_card(CameraLayout::CameraOverlay, Some(&circle), WIDE),
            WebcamLayoutCard::Circle
        );

        let bubble = shaped(2);
        assert_eq!(webcam_shape_index(&bubble, WIDE), Some(2));
        assert_eq!(
            webcam_layout_card(CameraLayout::CameraOverlay, Some(&bubble), WIDE),
            WebcamLayoutCard::Bubble
        );

        let oval = LayerSettings {
            radius: 1.0,
            h: 0.2,
            ..LayerSettings::default()
        };
        assert_eq!(webcam_shape_index(&oval, WIDE), None);
        assert_eq!(
            webcam_layout_card(CameraLayout::CameraOverlay, Some(&oval), WIDE),
            WebcamLayoutCard::Bubble
        );

        let custom = LayerSettings {
            radius: 0.35,
            h: 0.31,
            ..LayerSettings::default()
        };
        assert_eq!(webcam_shape_index(&custom, WIDE), None);
        assert_eq!(
            webcam_layout_card(CameraLayout::CameraOverlay, Some(&custom), WIDE),
            WebcamLayoutCard::Bubble
        );

        let portrait = 9.0 / 16.0;
        let circle_portrait = LayerSettings {
            radius: radius_for_shape(0),
            h: 0.2 * portrait * aspect_for_shape(0),
            ..LayerSettings::default()
        };
        assert_eq!(webcam_shape_index(&circle_portrait, portrait), Some(0));
        assert_eq!(
            webcam_layout_card(
                CameraLayout::CameraOverlay,
                Some(&circle_portrait),
                portrait
            ),
            WebcamLayoutCard::Circle
        );
    }

    #[test]
    fn resizing_keeps_the_shape_aspect_inside_the_layer_bounds() {
        let aspects = [WIDE, 1.0, 0.75, 9.0 / 16.0, 2.31];
        for aspect in aspects {
            for requested in [0.01, 0.2, 0.6, 0.99] {
                let mut layer = LayerSettings::default();
                layer.resize_overlay(requested, aspect);
                assert!(layer.w >= super::super::webcam::MIN_SIZE - 1e-9);
                assert!(layer.w <= super::super::webcam::MAX_SIZE + 1e-9);
                assert!(layer.h >= super::super::webcam::MIN_SIZE - 1e-9);
                assert!(layer.h <= super::super::webcam::MAX_SIZE + 1e-9);
                assert!((layer.h / layer.w - aspect).abs() < 1e-9);
            }
        }

        let mut anchored = LayerSettings {
            x: 0.7,
            y: 0.55,
            w: 0.2,
            h: 0.25,
            ..LayerSettings::default()
        };
        let before = anchored.overlay_alignment();
        let aspect = anchored.h / anchored.w;
        anchored.resize_overlay(0.4, aspect);
        let after = anchored.overlay_alignment();
        assert!((after.0 - before.0).abs() < 1e-9);
        assert!((after.1 - before.1).abs() < 1e-9);
    }

    #[test]
    fn the_position_pad_keeps_the_dot_a_fixed_size_inside_the_field() {
        // The dot is 12 * scale whatever the camera's size is, and a field too
        // small to hold it shrinks the dot rather than letting it spill.
        assert_eq!(webcam_pad_dot_radius(200.0, 100.0, 1.0), 12.0);
        assert_eq!(webcam_pad_dot_radius(200.0, 200.0, 2.0), 24.0);
        // The fifth-of-the-smaller-side ceiling wins on a field that cannot
        // hold the scaled dot.
        assert_eq!(webcam_pad_dot_radius(200.0, 100.0, 2.0), 20.0);
        assert!((webcam_pad_dot_radius(20.0, 12.0, 1.0) - 2.4).abs() < 1e-9);

        // A 200-wide field with the 24-wide dot leaves 176 of travel, and the
        // dot's edge stops at the field's edge at either end.
        let dot = 24.0;
        assert_eq!(webcam_pad_offset(0.0, 200.0, dot), 0.0);
        assert_eq!(webcam_pad_offset(0.5, 200.0, dot), 88.0);
        assert_eq!(webcam_pad_offset(1.0, 200.0, dot), 176.0);
        // An alignment outside 0..1 stays against the edge it ran past.
        assert_eq!(webcam_pad_offset(-0.4, 200.0, dot), 0.0);
        assert_eq!(webcam_pad_offset(1.4, 200.0, dot), 176.0);
        // A field no larger than the dot has no travel to place it in.
        assert_eq!(webcam_pad_offset(0.5, 24.0, dot), 0.0);
        assert_eq!(webcam_pad_offset(0.5, 8.0, dot), 0.0);
    }

    #[test]
    fn the_position_pad_reads_the_pointer_at_the_dot_centre() {
        // The pointer grabs the dot's centre, so the field's edges land exactly
        // on 0 and 1 and the dot stays wholly inside the field.
        let radius = webcam_pad_dot_radius(200.0, 100.0, 1.0);
        let dot = radius * 2.0;
        assert_eq!(webcam_pad_alignment(radius, 200.0, dot), 0.0);
        assert_eq!(webcam_pad_alignment(100.0, 200.0, dot), 0.5);
        assert_eq!(webcam_pad_alignment(200.0 - radius, 200.0, dot), 1.0);
        assert_eq!(webcam_pad_alignment(-30.0, 200.0, dot), 0.0);
        assert_eq!(webcam_pad_alignment(260.0, 200.0, dot), 1.0);
        assert_eq!(webcam_pad_alignment(100.0, dot, dot), 0.0);
        for alignment in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let centre = webcam_pad_offset(alignment, 200.0, dot) + radius;
            assert!((webcam_pad_alignment(centre, 200.0, dot) - alignment).abs() < 1e-9);
            assert!(centre - radius >= -1e-9);
            assert!(centre + radius <= 200.0 + 1e-9);
        }
    }

    #[test]
    fn cutout_positions_split_the_frame_evenly() {
        assert_eq!(webcam_cutout_position(0), 0.0);
        assert_eq!(webcam_cutout_position(1), 0.5);
        assert_eq!(webcam_cutout_position(2), 1.0);
    }

    #[test]
    fn preview_aspect_follows_the_frame_within_bounds() {
        assert!((webcam_preview_aspect(WIDE) - WIDE).abs() < 1e-9);
        assert!((webcam_preview_aspect(2.4) - 1.9).abs() < 1e-9);
        assert!((webcam_preview_aspect(0.4) - 0.75).abs() < 1e-9);
        assert!((webcam_preview_aspect(f64::NAN) - WIDE).abs() < 1e-9);
    }

    #[test]
    fn editing_follows_the_selected_change_not_the_playhead() {
        let mut state = panel_state(
            CameraLayout::ScreenOnly,
            vec![cue(4.0, CameraLayout::SplitScreen)],
            Some(0),
        );
        state.playhead = 8.0;
        state.effective = CameraLayout::CameraOverlay;

        assert_eq!(
            webcam_selected_cue(&state).map(|(_, cue)| cue.at),
            Some(4.0)
        );
        assert_eq!(webcam_editing_layout(&state), CameraLayout::SplitScreen);
        let controls = webcam_editing_controls(&state);
        assert!(controls.split);
        assert!(controls.mirror);
        assert!(!controls.overlay);
        assert!(!controls.cutout);
        assert!(controls.transitions);
        assert_eq!(
            webcam_layout_card(state.effective, state.layer.as_ref(), state.frame_aspect),
            WebcamLayoutCard::Bubble
        );
    }

    #[test]
    fn an_invalid_selection_edits_the_default_layout() {
        let state = panel_state(
            CameraLayout::CameraCutout,
            vec![cue(2.0, CameraLayout::ScreenOnly)],
            Some(9),
        );
        assert_eq!(webcam_selected_cue(&state), None);
        assert_eq!(webcam_editing_layout(&state), CameraLayout::CameraCutout);
        let controls = webcam_editing_controls(&state);
        assert!(controls.mirror);
        assert!(controls.cutout);
        assert!(!controls.overlay);
        assert!(!controls.split);
        assert!(controls.transitions);

        let default = panel_state(CameraLayout::ScreenOnly, Vec::new(), None);
        assert_eq!(webcam_editing_layout(&default), CameraLayout::ScreenOnly);
        let controls = webcam_editing_controls(&default);
        assert!(!controls.overlay);
        assert!(!controls.split);
        assert!(!controls.cutout);
        assert!(!controls.mirror);
        assert!(!controls.transitions);

        let selected_before_removal = panel_state(
            CameraLayout::CameraOverlay,
            vec![cue(2.0, CameraLayout::SplitScreen)],
            Some(0),
        );
        let mut removed = selected_before_removal.clone();
        removed.layouts.as_mut().expect("layouts").cues.clear();
        assert!(webcam_panel_needs_rebuild(
            Some(&selected_before_removal),
            &removed
        ));
        assert_eq!(webcam_editing_layout(&removed), CameraLayout::CameraOverlay);
    }

    #[test]
    fn editing_controls_are_scoped_to_the_edited_layout() {
        let overlay = panel_state(
            CameraLayout::CameraOverlay,
            vec![cue(1.0, CameraLayout::CameraOverlay)],
            Some(0),
        );
        let controls = webcam_editing_controls(&overlay);
        assert!(controls.overlay);
        assert!(controls.mirror);
        assert!(!controls.split);
        assert!(!controls.cutout);
        assert!(controls.transitions);

        let split = panel_state(
            CameraLayout::SplitScreen,
            vec![cue(1.0, CameraLayout::SplitScreen)],
            Some(0),
        );
        let controls = webcam_editing_controls(&split);
        assert!(!controls.overlay);
        assert!(controls.split);
        assert!(controls.mirror);
        assert!(!controls.cutout);
        assert!(controls.transitions);

        let cutout = panel_state(
            CameraLayout::CameraCutout,
            vec![cue(1.0, CameraLayout::CameraCutout)],
            Some(0),
        );
        let controls = webcam_editing_controls(&cutout);
        assert!(!controls.overlay);
        assert!(!controls.split);
        assert!(controls.cutout);
        assert!(controls.mirror);
        assert!(controls.transitions);

        let fullscreen = panel_state(
            CameraLayout::FullscreenCamera,
            vec![cue(1.0, CameraLayout::FullscreenCamera)],
            Some(0),
        );
        let controls = webcam_editing_controls(&fullscreen);
        assert!(!controls.overlay);
        assert!(!controls.split);
        assert!(!controls.cutout);
        assert!(!controls.mirror);
        assert!(!controls.transitions);

        let screen = panel_state(
            CameraLayout::ScreenOnly,
            vec![cue(1.0, CameraLayout::ScreenOnly)],
            Some(0),
        );
        let controls = webcam_editing_controls(&screen);
        assert!(!controls.overlay);
        assert!(!controls.split);
        assert!(!controls.cutout);
        assert!(!controls.mirror);
        assert!(!controls.transitions);
    }

    #[test]
    fn refresh_ignores_playback_but_not_the_edit_target() {
        let state = panel_state(
            CameraLayout::CameraOverlay,
            vec![cue(2.0, CameraLayout::CameraOverlay)],
            None,
        );
        assert!(webcam_panel_needs_rebuild(None, &state));
        assert!(!webcam_panel_needs_rebuild(Some(&state), &state));

        let mut playback = state.clone();
        playback.playhead = 6.0;
        playback.playing = true;
        playback.effective = CameraLayout::SplitScreen;
        playback.cue_at_playhead = Some(0);
        playback.selection_epoch = 4;
        assert!(!webcam_panel_needs_rebuild(Some(&state), &playback));

        let mut selected = playback.clone();
        selected.selected = Some(0);
        assert!(webcam_panel_needs_rebuild(Some(&playback), &selected));

        let mut layer = selected.clone();
        layer.layer.as_mut().expect("layer").radius = 0.4;
        assert!(webcam_panel_needs_rebuild(Some(&selected), &layer));

        let mut aspect = layer.clone();
        aspect.frame_aspect = 9.0 / 16.0;
        assert!(webcam_panel_needs_rebuild(Some(&layer), &aspect));

        let mut duration = aspect.clone();
        duration.duration = 8.0;
        assert!(webcam_panel_needs_rebuild(Some(&aspect), &duration));

        let mut layout_change = duration.clone();
        layout_change
            .layouts
            .as_mut()
            .expect("layouts")
            .default_layout = CameraLayout::ScreenOnly;
        assert!(webcam_panel_needs_rebuild(Some(&duration), &layout_change));

        let mut epoch = state.clone();
        epoch.selection_epoch += 1;
        assert!(!webcam_panel_needs_rebuild(Some(&state), &epoch));
    }

    #[test]
    fn a_new_selection_reveals_the_panel_but_playback_does_not() {
        assert!(!webcam_selection_reveal(None, 0, true));
        assert!(!webcam_selection_reveal(Some(4), 4, true));
        assert!(webcam_selection_reveal(Some(4), 5, true));
        assert!(!webcam_selection_reveal(Some(4), 5, false));
        assert!(!webcam_selection_reveal(Some(4), 0, false));
    }

    #[test]
    fn camera_panel_controls_fit_the_tool_column() {
        let Some(()) = crate::test_support::with_gtk(|| {
            super::super::ui_support::install_recording_editor_css();
            super::install_css();
            let scale = crate::platform::ui_scale();
            let content_width = super::webcam_content_width(scale);
            let scroll_width = webcam_content_width(scale);
            let window = gtk4::Window::new();
            let rows = build_webcam_control_rows(scale);
            window.set_child(Some(&rows.root));
            let (minimum, natural) = measure_width(&rows.root);
            assert!(minimum > 0);
            assert!(
                minimum <= content_width,
                "scope row minimum {minimum} exceeds {content_width}"
            );
            assert!(
                natural <= content_width,
                "scope row natural {natural} exceeds {content_width}"
            );
            // The widest state the row reaches: a selected change, its time
            // field, and a status line naming the layout.
            rows.status.set_text("0:00.0 · Circle");
            rows.change_scope.set_sensitive(true);
            rows.time_row.set_visible(true);
            rows.delete_change.set_visible(true);
            let (minimum, natural) = measure_width(&rows.root);
            assert!(
                minimum <= content_width,
                "selected scope row minimum {minimum} exceeds {content_width}"
            );
            assert!(
                natural <= content_width,
                "selected scope row natural {natural} exceeds {content_width}"
            );

            let grid = build_webcam_layout_grid(scale, WIDE);
            window.set_child(Some(&grid.root));
            let card_width = grid.tiles[0].button.width_request();
            let card_height = grid.tiles[0].button.height_request();
            for (index, tile) in grid.tiles.iter().enumerate() {
                for (other, other_tile) in grid.tiles.iter().enumerate() {
                    if other != index {
                        other_tile.button.set_active(false);
                    }
                }
                tile.button.set_active(true);
                let (minimum, natural) = measure_width(&grid.root);
                assert!(
                    minimum <= content_width,
                    "chooser minimum {minimum} exceeds {content_width}"
                );
                assert!(
                    natural <= content_width,
                    "chooser natural {natural} exceeds {content_width}"
                );
                assert_eq!(tile.button.width_request(), card_width);
                assert_eq!(tile.button.height_request(), card_height);
            }
            grid.set_frame_aspect(9.0 / 16.0);
            let (minimum, natural) = measure_width(&grid.root);
            assert!(minimum <= content_width);
            assert!(natural <= content_width);

            for frame_aspect in [WIDE, 9.0 / 16.0] {
                let layer = LayerSettings::default();
                let layouts = LayoutSettings::default();
                let context = build_webcam_context(
                    scale,
                    layer,
                    &layouts,
                    frame_aspect,
                    WebcamContextActions::idle(),
                );
                window.set_child(Some(&context.root));
                for card in WebcamLayoutCard::ALL {
                    let controls = controls_for(card);
                    context.refresh(&layer, &layouts, frame_aspect, controls);
                    let (minimum, natural) = measure_width(&context.root);
                    assert!(
                        minimum <= scroll_width,
                        "{} context minimum {minimum} exceeds {scroll_width}",
                        card.label()
                    );
                    assert!(
                        natural <= scroll_width,
                        "{} context natural {natural} exceeds {scroll_width}",
                        card.label()
                    );
                    if controls.overlay || controls.split || controls.cutout {
                        assert!(natural > 0);
                    } else {
                        assert!(!context.overlay.root.is_visible());
                        assert!(!context.split.root.is_visible());
                        assert!(!context.cutout.root.is_visible());
                        assert!(!context.camera.root.is_visible());
                        assert!(!context.transition.root.is_visible());
                    }
                }
            }
        }) else {
            eprintln!(
                "test_support: camera panel width measurement skipped because GTK is unavailable"
            );
            return;
        };
    }
}

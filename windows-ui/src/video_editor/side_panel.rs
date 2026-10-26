//! Tool-specific side panels for the Windows video editor.
//!
//! The Cursor and Background panels own their presentation and local settings.
//! The Zoom panel is a view over the timeline: it reads the selected zoom clip
//! and the camera settings, and every control writes straight back into them,
//! so the preview and the export both see the edit.

use super::background::{self, Background};
use super::timeline::{nearest_zoom_preset, ZoomMode, ZOOM_SCALE_PRESETS};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
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
    EventControllerMotion, FileChooserAction, FileChooserNative, FileFilter, GestureDrag, Grid,
    Image, Label, Orientation, PolicyType, ResponseType, ScrolledWindow, Stack,
    StackTransitionType, Switch, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, PI};
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

    let buttons = Rc::new(RefCell::new(Vec::<ToggleButton>::new()));
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
            None,
        ),
        (
            "cursor",
            icon_names::POINTER_PRIMARY_CLICK,
            t("Cursor"),
            None,
        ),
        (
            "frames",
            icon_names::custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            t("Frames"),
            None,
        ),
        (
            "takes",
            icon_names::custom::MARKER_SYMBOLIC,
            t("Takes"),
            Some(t("Takes — coming soon")),
        ),
    ];

    for (index, (page_name, icon_name, label, unavailable_tooltip)) in tools.into_iter().enumerate()
    {
        let button = ToggleButton::new();
        button.add_css_class("recording-editor-tool-section-btn");
        button.set_has_frame(false);
        button.set_hexpand(true);
        button.set_tooltip_text(Some(unavailable_tooltip.as_deref().unwrap_or(&label)));
        // The take library is not built yet; the icon marks where it goes.
        if unavailable_tooltip.is_some() {
            button.set_sensitive(false);
        }
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
        rail.append(&button);
    }

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

        let value_text = format_value(self.value.get(), self.decimals);
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
    use super::format_value;

    #[test]
    fn value_labels_use_the_requested_precision() {
        assert_eq!(format_value(24.4, 0), "24");
        assert_eq!(format_value(0.35, 1), "0.3");
        assert_eq!(format_value(0.456, 2), "0.46");
    }
}

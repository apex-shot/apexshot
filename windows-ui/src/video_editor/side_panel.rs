//! Tool-specific side panels for the Windows video editor.
//!
//! These panels own their presentation and local control state. Connecting the
//! values to playback and export is the next backend slice.

use super::background::{self, Background};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{ClickEffect, CursorSettings, CursorTheme};
use gtk4::cairo::{FontSlant, FontWeight};
use gtk4::gdk;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea,
    EventControllerMotion, FileChooserAction, FileChooserNative, GestureDrag, Grid, Image, Label,
    Orientation, PolicyType, ResponseType, ScrolledWindow, Stack, StackTransitionType, Switch,
    ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, PI};
use std::rc::Rc;

pub(super) const TOOL_COLUMN_WIDTH: i32 = 288;

pub(super) fn build_tools_column(
    scale: f64,
    window: &ApplicationWindow,
    background: Background,
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
        &build_background_panel(scale, window, background),
        Some("background"),
    );
    pages.add_named(&build_cursor_panel(scale), Some("cursor"));
    pages.add_named(&build_frames_panel(scale), Some("frames"));
    pages.set_visible_child_name("background");

    let buttons = Rc::new(RefCell::new(Vec::<ToggleButton>::new()));
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
    let custom_page = super::custom_fill::build_custom_page(scale, select_custom);
    custom_page.widget.set_visible(false);
    let image_page = build_image_page(scale, window);
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

    let choices_for_none = choice_buttons.clone();
    let clear_custom = custom_page.clear.clone();
    let wallpaper_scroll_for_none = wallpaper_scroll.clone();
    let background_for_none = background.clone();
    none.connect_clicked(move |_| {
        for card in choices_for_none.borrow().iter() {
            card.set_active(false);
        }
        clear_custom();
        background_for_none.select(None);
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

fn build_image_page(scale: f64, window: &ApplicationWindow) -> GtkBox {
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

    let window = window.clone();
    let label_for_pick = label.clone();
    picker.connect_clicked(move |_| {
        choose_background_image(&window, &label_for_pick);
    });
    page
}

fn build_cursor_panel(scale: f64) -> GtkBox {
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
    pages.add_named(&build_cursor_style_page(scale), Some("style"));
    pages.add_named(&build_cursor_follow_page(scale), Some("follow"));
    pages.add_named(&build_cursor_effects_page(scale), Some("effects"));
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

fn build_cursor_style_page(scale: f64) -> GtkBox {
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
    let mut first = None::<ToggleButton>;
    for (index, theme) in CursorTheme::ALL.into_iter().enumerate() {
        let card = cursor_card(theme, scale);
        if let Some(first) = &first {
            card.set_group(Some(first));
        } else {
            first = Some(card.clone());
        }
        grid.attach(&card, (index % 2) as i32, (index / 2) as i32, 1, 1);
    }
    if let Some(first) = &first {
        first.set_active(true);
    }
    page.append(&grid);
    page.append(&value_slider_row(&t("Size"), 0.5, 3.0, 1.0, 0.05, 1, scale));
    page.append(&value_slider_row(
        &t("Shadow"),
        0.0,
        1.0,
        0.4,
        0.05,
        1,
        scale,
    ));
    page
}

fn build_cursor_follow_page(scale: f64) -> GtkBox {
    let page = tab_body();
    let preset_label = Label::new(Some(&t("PRESET")));
    preset_label.add_css_class("recording-editor-zoom-kicker");
    preset_label.set_xalign(0.0);
    page.append(&preset_label);

    let presets = GtkBox::new(Orientation::Horizontal, 8);
    presets.set_homogeneous(true);
    let focused = preset_card(&t("Focused"), &t("Tighter, faster"), scale);
    let smooth = preset_card(&t("Smooth"), &t("Softer follow"), scale);
    smooth.set_group(Some(&focused));
    focused.set_active(true);
    presets.append(&focused);
    presets.append(&smooth);
    page.append(&presets);
    page.append(&value_slider_row(
        &t("Smoothing"),
        0.0,
        1.0,
        0.35,
        0.05,
        1,
        scale,
    ));
    page.append(&value_slider_row(
        &t("Speed"),
        0.25,
        3.0,
        1.0,
        0.05,
        1,
        scale,
    ));

    let idle_row = GtkBox::new(Orientation::Horizontal, 8);
    idle_row.add_css_class("recording-editor-zoom-classic");
    let idle_label = Label::new(Some(&t("Hide when idle")));
    idle_label.add_css_class("recording-editor-zoom-classic-label");
    idle_label.set_hexpand(true);
    idle_label.set_xalign(0.0);
    let idle = Switch::new();
    idle.add_css_class("recording-editor-zoom-switch");
    idle.set_active(true);
    idle.set_halign(Align::End);
    idle_row.append(&idle_label);
    idle_row.append(&idle);
    page.append(&idle_row);
    page.append(&value_slider_row(
        &t("Idle delay"),
        120.0,
        4000.0,
        800.0,
        40.0,
        0,
        scale,
    ));
    page
}

fn build_cursor_effects_page(scale: f64) -> GtkBox {
    let page = tab_body();

    let effect = Rc::new(Cell::new(ClickEffect::Ripple));
    let anim_t = Rc::new(Cell::new(0.0));
    let live = DrawingArea::new();
    live.add_css_class("recording-editor-click-live-preview");
    live.set_content_width((240.0 * scale).round() as i32);
    live.set_content_height((88.0 * scale).round() as i32);
    live.set_hexpand(true);
    {
        let effect = effect.clone();
        let anim_t = anim_t.clone();
        live.set_draw_func(move |widget, context, width, height| {
            let settings = CursorSettings {
                click_effect: effect.get(),
                ..CursorSettings::default()
            };
            draw_click_live_preview(
                widget,
                context,
                f64::from(width),
                f64::from(height),
                settings,
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
    ripple_btn.set_active(true);
    click_row.append(&none_btn);
    click_row.append(&ripple_btn);
    click_row.append(&circle_btn);
    page.append(&click_row);

    for (button, choice) in [
        (none_btn, ClickEffect::None),
        (ripple_btn, ClickEffect::Ripple),
        (circle_btn, ClickEffect::Circle),
    ] {
        let effect = effect.clone();
        let live = live.clone();
        button.connect_clicked(move |button| {
            if !button.is_active() {
                return;
            }
            effect.set(choice);
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
    let slider = FilledSlider::new(label, min, max, initial, step, decimals, scale);
    slider.on_change(Rc::new(on_change));
    slider.widget()
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

fn choose_background_image(window: &ApplicationWindow, label: &Label) {
    let chooser = FileChooserNative::new(
        Some(&t("Select background image")),
        Some(window),
        FileChooserAction::Open,
        Some(&t("Select")),
        Some(&t("Cancel")),
    );
    let label = label.clone();
    chooser.connect_response(move |chooser, response| {
        if response == ResponseType::Accept {
            if let Some(name) = chooser
                .file()
                .and_then(|file| file.path())
                .and_then(|path| path.file_name().map(|name| name.to_owned()))
            {
                label.set_text(&name.to_string_lossy());
            }
        }
        chooser.hide();
    });
    chooser.show();
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

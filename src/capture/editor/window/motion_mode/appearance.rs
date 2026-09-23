use gtk4::cairo::Context;
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, Entry,
    FileChooserAction, FileChooserNative, FileFilter, GestureClick, Grid, Label, Orientation,
    Overlay, ResponseType, Revealer, Separator, Stack, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::capture::editor::types::FrameStyle;
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionBackgroundFillType, MotionFrame, MotionFramePreset, MotionSceneShadowPlacement,
    MotionSceneShadowPreset,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::widgets::{
    motion_appearance_slider, motion_color_control, motion_gradient_color_control,
    motion_reference_percent_slider, motion_rgba,
};
use super::MotionSession;

/// Two-stop gradient presets for Motion backgrounds. Selecting one
/// copies its stops into the editable gradient colors.
const MOTION_GRADIENT_PRESETS: [(&str, [f64; 4], [f64; 4]); 8] = [
    ("Dusk", [0.10, 0.14, 0.30, 1.0], [0.45, 0.22, 0.55, 1.0]),
    ("Sunset", [0.98, 0.55, 0.30, 1.0], [0.88, 0.28, 0.48, 1.0]),
    ("Ocean", [0.13, 0.62, 0.65, 1.0], [0.07, 0.22, 0.47, 1.0]),
    ("Forest", [0.20, 0.55, 0.34, 1.0], [0.05, 0.25, 0.18, 1.0]),
    ("Ember", [0.85, 0.25, 0.21, 1.0], [0.30, 0.07, 0.10, 1.0]),
    ("Slate", [0.55, 0.58, 0.64, 1.0], [0.16, 0.18, 0.22, 1.0]),
    ("Peach", [1.00, 0.85, 0.72, 1.0], [0.98, 0.55, 0.55, 1.0]),
    ("Violet", [0.58, 0.36, 0.90, 1.0], [0.20, 0.16, 0.50, 1.0]),
];

/// One gradient preset swatch, sized and styled exactly like the wallpaper
/// thumbnails so both catalogs fill the inspector width the same way.
fn motion_gradient_preset_button(
    index: usize,
    start: [f64; 4],
    end: [f64; 4],
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    apply_gradient_colors: &Rc<dyn Fn(gtk4::gdk::RGBA, gtk4::gdk::RGBA)>,
    selection_buttons: &Rc<RefCell<Vec<(usize, Button)>>>,
    on_interact: &Rc<dyn Fn()>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-gradient-preset");
    button.set_tooltip_text(Some(&t(MOTION_GRADIENT_PRESETS[index].0)));
    button.set_child(Some(&motion_gradient_preset_area(start, end)));
    selection_buttons.borrow_mut().push((index, button.clone()));
    button.connect_clicked({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        let apply_gradient_colors = apply_gradient_colors.clone();
        let on_interact = on_interact.clone();
        move |_| {
            on_interact();
            {
                let mut runtime = runtime.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.appearance.gradient_color_1 = start;
                runtime.motion.appearance.gradient_color_2 = end;
                runtime.motion.appearance.selected_gradient_preset_index = Some(index);
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
                runtime.backdrop_cache = None;
                runtime.preview_frame = None;
                none_button.remove_css_class("active-background-option");
                preview.queue_draw();
            }
            let [r1, g1, b1, a1] = start;
            let [r2, g2, b2, a2] = end;
            apply_gradient_colors(
                gtk4::gdk::RGBA::new(r1 as f32, g1 as f32, b1 as f32, a1 as f32),
                gtk4::gdk::RGBA::new(r2 as f32, g2 as f32, b2 as f32, a2 as f32),
            );
            for (candidate_index, candidate) in selection_buttons.borrow().iter() {
                if *candidate_index == index {
                    candidate.add_css_class("active-background-option");
                } else {
                    candidate.remove_css_class("active-background-option");
                }
            }
        }
    });
    button
}

/// The compact strip's expand affordance: a fifth preset rendered like a
/// wallpaper thumbnail with the catalog's stack glyph, opening the full grid.
fn motion_gradient_expand_tile(
    start: [f64; 4],
    end: [f64; 4],
    stack: &Stack,
    on_interact: &Rc<dyn Fn()>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-gradient-preset");
    button.set_tooltip_text(Some(&t("Show all gradients")));
    let overlay = Overlay::new();
    overlay.set_child(Some(&motion_gradient_preset_area(start, end)));
    let expand = Label::new(Some("⌄"));
    expand.set_halign(Align::Center);
    expand.set_valign(Align::End);
    expand.set_can_target(false);
    expand.add_css_class("editor-motion-wallpaper-stack-glyph");
    overlay.add_overlay(&expand);
    button.set_child(Some(&overlay));
    button.connect_clicked({
        let stack = stack.clone();
        let on_interact = on_interact.clone();
        move |_| {
            on_interact();
            stack.set_visible_child_name("all")
        }
    });
    button
}

fn motion_gradient_preset_area(start: [f64; 4], end: [f64; 4]) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(56);
    area.set_content_height(56);
    area.set_can_target(false);
    area.set_draw_func(move |_, cr, width, height| {
        let gradient =
            gtk4::cairo::LinearGradient::new(0.0, 0.0, f64::from(width), f64::from(height));
        let [r1, g1, b1, a1] = start;
        let [r2, g2, b2, a2] = end;
        gradient.add_color_stop_rgba(0.0, r1, g1, b1, a1);
        gradient.add_color_stop_rgba(1.0, r2, g2, b2, a2);
        cr.set_source(&gradient).ok();
        motion_thumbnail_rounded_rectangle(cr, 0.0, 0.0, f64::from(width), f64::from(height), 11.0);
        cr.fill().ok();
    });
    area
}

/// Motion appearance is a scene-level inspector rather than an
/// Mini preview for one frame-style preset: gray card with the preset's
/// outside border (and accent strokes) drawn around it, matching the static
/// and motion card renderers.
fn frame_style_preset_area(style: FrameStyle) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(56);
    area.set_content_height(44);
    area.set_can_target(false);
    area.set_hexpand(false);
    area.set_halign(Align::Center);
    area.set_valign(Align::Center);
    area.set_draw_func(move |_, cr, width, height| {
        let w = f64::from(width);
        let h = f64::from(height);
        // No tile backdrop: the chromeless button shows the panel behind the
        // preview, so the style swatch itself is the tile.
        let spec = style.spec();
        let card_w = 34.0;
        let card_h = 22.0;
        let card_x = (w - card_w) / 2.0;
        let card_y = (h - card_h) / 2.0;
        let card_r = 5.0;
        for backing in [spec.backing1, spec.backing2].into_iter().flatten() {
            let _ = cr.save();
            if backing.center_pivot {
                cr.translate(
                    card_x + card_w / 2.0 + backing.offset_x * 0.32,
                    card_y + card_h / 2.0 + backing.offset_y * 0.32,
                );
                cr.rotate(backing.rotation_deg.to_radians());
                cr.translate(-card_w / 2.0, -card_h / 2.0);
            } else {
                cr.translate(
                    card_x + backing.offset_x * 0.32 + card_w,
                    card_y + backing.offset_y * 0.32 + card_h,
                );
                cr.rotate(backing.rotation_deg.to_radians());
                cr.translate(-card_w, -card_h);
            }
            cr.set_source_rgba(
                backing.color.r,
                backing.color.g,
                backing.color.b,
                backing.color.a,
            );
            motion_thumbnail_rounded_rectangle(cr, 0.0, 0.0, card_w, card_h, card_r);
            cr.fill().ok();
            cr.restore().ok();
        }
        cr.set_source_rgba(0.82, 0.82, 0.84, 1.0);
        motion_thumbnail_rounded_rectangle(cr, card_x, card_y, card_w, card_h, card_r);
        cr.fill().ok();
        // Glass presets: same band + rim recipe as the renderers, scaled
        // down to the tile.
        if let Some(liquid) =
            crate::capture::editor::render::LiquidFrame::resolve(&spec, spec.border_thickness, 0.45)
        {
            let path = |path_context: &gtk4::cairo::Context, expand: f64| {
                crate::capture::editor::render::rounded_rect_path(
                    path_context,
                    card_x - expand,
                    card_y - expand,
                    card_w + expand * 2.0,
                    card_h + expand * 2.0,
                    if card_r <= 0.0 { 0.0 } else { card_r + expand },
                );
            };
            liquid.paint(cr, card_y, card_y + card_h, path);
        }
        let mut expand = 0.0;
        let draw_stroke = |thickness: f64, r: f64, g: f64, b: f64, a: f64, extra: f64| {
            let t = (thickness * 0.32).max(1.0);
            let e = extra + t / 2.0;
            cr.set_source_rgba(r, g, b, a);
            cr.set_line_width(t);
            motion_thumbnail_rounded_rectangle(
                cr,
                card_x - e,
                card_y - e,
                card_w + e * 2.0,
                card_h + e * 2.0,
                card_r + e,
            );
            cr.stroke().ok();
            e + t / 2.0
        };
        if !spec.liquid && spec.border_thickness > 0.0 && !spec.inset_border {
            let c = spec.border_color;
            expand = draw_stroke(spec.border_thickness, c.r, c.g, c.b, c.a, expand);
        }
        if !spec.liquid && spec.inset_border && spec.border_thickness > 0.0 {
            let t = (spec.border_thickness * 0.32).max(1.0);
            let c = spec.border_color;
            cr.set_source_rgba(c.r, c.g, c.b, c.a);
            cr.set_line_width(t);
            motion_thumbnail_rounded_rectangle(
                cr,
                card_x + t / 2.0,
                card_y + t / 2.0,
                (card_w - t).max(1.0),
                (card_h - t).max(1.0),
                (card_r - t / 2.0).max(0.0),
            );
            cr.stroke().ok();
        }
        for outer in [spec.outer1, spec.outer2]
            .into_iter()
            .flatten()
            .filter(|_| !spec.liquid)
        {
            expand += outer.gap * 0.32;
            let c = outer.color;
            expand = draw_stroke(outer.thickness, c.r, c.g, c.b, c.a, expand);
        }
    });
    area
}

/// animation clip. The five fill controls map one-to-one to the
/// `BackgroundFillType` cases and only mutate the compositor state.
pub(in crate::capture::editor::window) fn build_motion_appearance_panel(
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
    on_interact: Option<Rc<dyn Fn()>>,
) -> GtkBox {
    // Static image editor: interacting with Appearance must arm the Background
    // tool so a later canvas click previews instead of drawing with a stale
    // Pen/Arrow/etc. Motion callers pass None (no static toolbar to sync).
    let notify_interact: Rc<dyn Fn()> = on_interact.unwrap_or_else(|| Rc::new(|| {}));
    let (
        initial_background_color,
        initial_gradient_start,
        initial_gradient_end,
        initial_frame_style,
        initial_shadow_opacity,
        initial_shadow_blur,
        initial_shadow_position,
        initial_fill_type,
        initial_padding,
        initial_background_blur,
        initial_background_noise,
        initial_border_radius,
    ) = {
        let runtime = session.runtime.borrow();
        let appearance = &runtime.motion.appearance;
        (
            motion_rgba(appearance.background_color),
            motion_rgba(appearance.gradient_color_1),
            motion_rgba(appearance.gradient_color_2),
            appearance.frame_style,
            appearance.shadow_opacity,
            appearance.shadow_blur,
            appearance.shadow_position,
            appearance.background_fill_type.clone(),
            appearance.background_padding,
            appearance.background_blur,
            appearance.background_noise,
            appearance.border_radius,
        )
    };
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("editor-inspector-placeholder-shell");
    root.add_css_class("editor-motion-inspector");
    root.set_hexpand(false);
    root.set_vexpand(false);
    // Safety net for view-only toggles and future controls: any pointer press
    // inside Appearance arms Background. Mutating controls also call
    // notify_interact explicitly so keyboard/popover edits are covered.
    {
        let notify = notify_interact.clone();
        let capture = GestureClick::new();
        capture.set_propagation_phase(gtk4::PropagationPhase::Capture);
        capture.connect_pressed(move |_, _, _, _| notify());
        root.add_controller(capture);
    }

    let title = Label::new(Some(&t("Appearance")));
    title.add_css_class("editor-inspector-title");
    title.set_xalign(0.0);
    root.append(&title);

    let fill_section = GtkBox::new(Orientation::Vertical, 0);
    fill_section.add_css_class("editor-motion-background-picker");
    let background_section = motion_appearance_section("Background");
    let none_button = Button::with_label(&t("None"));
    none_button.set_has_frame(false);
    none_button.set_hexpand(true);
    none_button.add_css_class("editor-background-option-button");
    if initial_fill_type == MotionBackgroundFillType::None {
        none_button.add_css_class("active-background-option");
    }
    none_button.connect_clicked({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::None;
            runtime.backdrop_cache = None;
            runtime.preview_frame = None;
            none_button.add_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    fill_section.append(&none_button);

    let color_section = GtkBox::new(Orientation::Vertical, 6);
    let color_title = Label::new(Some(&t("Color")));
    color_title.add_css_class("editor-background-section-title");
    color_title.set_xalign(0.0);
    let color = motion_color_control(initial_background_color, "Background color", true, {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let notify = notify_interact.clone();
        move |rgba| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_color = [
                rgba.red().into(),
                rgba.green().into(),
                rgba.blue().into(),
                rgba.alpha().into(),
            ];
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
            runtime.backdrop_cache = None;
            runtime.preview_frame = None;
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    color_section.append(&color_title);
    color_section.append(&color);

    let gradient_section = GtkBox::new(Orientation::Vertical, 6);
    let gradient_title = Label::new(Some(&t("Gradient")));
    gradient_title.add_css_class("editor-background-section-title");
    gradient_title.set_xalign(0.0);
    let preset_buttons: Rc<RefCell<Vec<(usize, Button)>>> = Rc::new(RefCell::new(Vec::new()));
    let (gradient, apply_gradient_colors) =
        motion_gradient_color_control(initial_gradient_start, initial_gradient_end, {
            let runtime = session.runtime.clone();
            let preview = preview.clone();
            let none_button = none_button.clone();
            let preset_buttons = preset_buttons.clone();
            let notify = notify_interact.clone();
            move |stop, rgba| {
                notify();
                let mut runtime = runtime.borrow_mut();
                runtime.begin_motion_edit();
                let color = [
                    rgba.red().into(),
                    rgba.green().into(),
                    rgba.blue().into(),
                    rgba.alpha().into(),
                ];
                if stop == 0 {
                    runtime.motion.appearance.gradient_color_1 = color;
                } else {
                    runtime.motion.appearance.gradient_color_2 = color;
                }
                runtime.motion.appearance.selected_gradient_preset_index = None;
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
                none_button.remove_css_class("active-background-option");
                for (_, button) in preset_buttons.borrow().iter() {
                    button.remove_css_class("active-background-option");
                }
                preview.queue_draw();
            }
        });
    gradient_section.append(&gradient_title);
    gradient_section.append(&gradient);

    // Presets follow the wallpaper catalog's compact-strip → full-grid
    // progression, with swatches much smaller than wallpaper thumbnails so
    // the inspector width never grows.
    let initial_gradient_preset = {
        let runtime = session.runtime.borrow();
        runtime.motion.appearance.selected_gradient_preset_index
    };
    let presets_section = GtkBox::new(Orientation::Vertical, 5);
    let presets_stack = Stack::new();
    presets_stack.set_hhomogeneous(false);
    presets_stack.set_vhomogeneous(false);
    presets_section.append(&presets_stack);
    let compact_row = GtkBox::new(Orientation::Horizontal, 5);
    compact_row.add_css_class("editor-motion-gradient-presets");
    presets_stack.add_named(&compact_row, Some("compact"));
    let all_view = GtkBox::new(Orientation::Vertical, 5);
    let show_less = Button::with_label(&t("Show less"));
    show_less.set_has_frame(false);
    show_less.set_halign(Align::End);
    show_less.add_css_class("editor-background-section-action-button");
    all_view.append(&show_less);
    presets_stack.add_named(&all_view, Some("all"));

    let add_preset_button = |index: usize,
                             start: [f64; 4],
                             end: [f64; 4],
                             target: &GtkBox,
                             initial_active: Option<usize>| {
        let button = motion_gradient_preset_button(
            index,
            start,
            end,
            session,
            preview,
            &none_button,
            &apply_gradient_colors,
            &preset_buttons,
            &notify_interact,
        );
        if initial_active == Some(index) {
            button.add_css_class("active-background-option");
        }
        target.append(&button);
    };
    let expand_preset = &MOTION_GRADIENT_PRESETS[3];
    for (index, (_, start, end)) in MOTION_GRADIENT_PRESETS.iter().enumerate().take(3) {
        add_preset_button(index, *start, *end, &compact_row, initial_gradient_preset);
    }
    compact_row.append(&motion_gradient_expand_tile(
        expand_preset.1,
        expand_preset.2,
        &presets_stack,
        &notify_interact,
    ));
    for row_start in (0..MOTION_GRADIENT_PRESETS.len()).step_by(4) {
        let row = GtkBox::new(Orientation::Horizontal, 5);
        row.add_css_class("editor-motion-gradient-presets");
        for (index, (_, start, end)) in MOTION_GRADIENT_PRESETS
            .iter()
            .enumerate()
            .skip(row_start)
            .take(4)
        {
            add_preset_button(index, *start, *end, &row, initial_gradient_preset);
        }
        all_view.append(&row);
    }
    show_less.connect_clicked({
        let stack = presets_stack.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            stack.set_visible_child_name("compact")
        }
    });
    presets_stack.set_visible_child_name("compact");
    gradient_section.append(&presets_section);
    let (wallpaper_catalog, activate_wallpaper_catalog) =
        motion_wallpaper_catalog_section(session, preview, &none_button, &notify_interact);
    let image_section = motion_image_section(
        "Image",
        "Choose Background Image",
        MotionBackgroundFillType::Image,
        window,
        session,
        preview,
        &none_button,
        &notify_interact,
    );

    // Choices remain visible inside one background card. Clicking a choice
    // expands its controls below the list instead of replacing the picker.
    let selection_stack = Stack::new();
    selection_stack.set_hhomogeneous(false);
    selection_stack.set_vhomogeneous(false);
    let color_choice = Button::with_label(&t("Color"));
    let gradient_choice = Button::with_label(&t("Gradient"));
    let wallpapers_choice = Button::with_label(&t("Wallpapers"));
    let image_choice = Button::with_label(&t("Image"));
    for button in [
        &color_choice,
        &gradient_choice,
        &wallpapers_choice,
        &image_choice,
    ] {
        button.set_has_frame(false);
        button.set_halign(Align::Fill);
        button.set_hexpand(true);
        button.add_css_class("editor-background-option-button");
        fill_section.append(button);
    }
    let empty_selection = GtkBox::new(Orientation::Vertical, 0);
    selection_stack.add_named(&empty_selection, Some("empty"));
    selection_stack.add_named(&color_section, Some("color"));
    selection_stack.add_named(&gradient_section, Some("gradient"));
    selection_stack.add_named(&wallpaper_catalog, Some("wallpapers"));
    selection_stack.add_named(&image_section, Some("image"));
    selection_stack.set_visible_child_name("empty");
    fill_section.append(&selection_stack);
    // The Motion default selects a wallpaper before this panel is built, so
    // reflect it: Wallpapers active with its catalog already open.
    if initial_fill_type == MotionBackgroundFillType::Wallpaper {
        none_button.remove_css_class("active-background-option");
        wallpapers_choice.add_css_class("active-background-option");
        selection_stack.set_visible_child_name("wallpapers");
    }
    color_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
            none_button.remove_css_class("active-background-option");
            color_choice.add_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("color");
            preview.queue_draw();
        }
    });
    gradient_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.add_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("gradient");
            preview.queue_draw();
        }
    });
    wallpapers_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let activate_wallpaper_catalog = activate_wallpaper_catalog.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            activate_wallpaper_catalog();
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.add_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("wallpapers");
        }
    });
    image_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.add_css_class("active-background-option");
            selection_stack.set_visible_child_name("image");
        }
    });
    none_button.connect_clicked({
        let selection_stack = selection_stack.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let none_button = none_button.clone();
        let notify = notify_interact.clone();
        move |_| {
            notify();
            none_button.add_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("empty");
        }
    });
    background_section.append(&fill_section);

    let padding = motion_reference_percent_slider("Padding", 0.0, 200.0, initial_padding);
    padding.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_padding = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    background_section.append(&padding.widget());

    let blur = motion_appearance_slider("Background blur", 0.0, 1.0, initial_background_blur, "%");
    blur.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_blur = slider.value();
            runtime.backdrop_cache = None;
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    background_section.append(&blur.widget());

    let noise =
        motion_appearance_slider("Background noise", 0.0, 1.0, initial_background_noise, "%");
    noise.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.background_noise = slider.value();
            runtime.backdrop_cache = None;
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    background_section.append(&noise.widget());
    root.append(&background_section);

    let shadow_section = motion_appearance_section("Shadow");
    let shadow_opacity =
        motion_appearance_slider("Shadow opacity", 0.0, 1.0, initial_shadow_opacity, "%");
    shadow_opacity.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.shadow_opacity = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_opacity.widget());

    let shadow_blur =
        motion_appearance_slider("Shadow blur", 0.0, 120.0, initial_shadow_blur, "px");
    shadow_blur.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.shadow_blur = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_blur.widget());

    let shadow_x = motion_appearance_slider(
        "Shadow position X",
        -200.0,
        200.0,
        initial_shadow_position.0,
        "px",
    );
    shadow_x.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.shadow_position.0 = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_x.widget());

    let shadow_y = motion_appearance_slider(
        "Shadow position Y",
        -200.0,
        200.0,
        initial_shadow_position.1,
        "px",
    );
    shadow_y.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.shadow_position.1 = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_y.widget());
    root.append(&shadow_section);

    let border_section = motion_appearance_section("Style");
    let style_grid = Grid::new();
    style_grid.set_column_homogeneous(true);
    style_grid.set_column_spacing(6);
    style_grid.set_row_spacing(8);
    style_grid.set_hexpand(false);
    style_grid.set_halign(Align::Fill);
    let style_buttons: Rc<RefCell<Vec<(FrameStyle, Button)>>> = Rc::new(RefCell::new(Vec::new()));
    // Late-bound handle so picking Liquid can also lift a sharp-corner card
    // onto a radius the glass highlights can play on (filled in below, after
    // the radius slider exists).
    let radius_slider_slot: Rc<RefCell<Option<FillSlider>>> = Rc::new(RefCell::new(None));
    {
        let style_buttons = style_buttons.clone();
        let radius_slider_slot = radius_slider_slot.clone();
        for (index, style) in FrameStyle::ALL.iter().enumerate() {
            let style = *style;
            let cell = GtkBox::new(Orientation::Vertical, 4);
            cell.set_hexpand(false);
            cell.set_halign(Align::Center);
            let button = Button::new();
            button.set_has_frame(false);
            button.set_size_request(56, 44);
            button.add_css_class("editor-frame-style-tile");
            button.set_tooltip_text(Some(&t(style.label())));
            button.set_child(Some(&frame_style_preset_area(style)));
            if style == initial_frame_style {
                button.add_css_class("active-background-option");
            }
            style_buttons.borrow_mut().push((style, button.clone()));
            button.connect_clicked({
                let runtime = session.runtime.clone();
                let preview = preview.clone();
                let style_buttons = style_buttons.clone();
                let radius_slider_slot = radius_slider_slot.clone();
                let notify = notify_interact.clone();
                move |_| {
                    notify();
                    // Glass needs corners to catch the light: a sharp-corner
                    // card collapses the edge into flat hairlines (and the
                    // wide frost looks blunt). Lift a ~sharp card onto a
                    // glass-friendly radius; an explicit user radius is left
                    // alone. The slider sync must run AFTER the borrow below
                    // is released: set_value fires the radius listener
                    // synchronously, which borrows the same runtime.
                    let bump_radius = {
                        let mut runtime = runtime.borrow_mut();
                        runtime.begin_motion_edit();
                        let spec = style.spec();
                        runtime.motion.appearance.frame_style = style;
                        runtime.motion.appearance.border_thickness = spec.border_thickness;
                        runtime.motion.appearance.border_fill_color = [
                            spec.border_color.r,
                            spec.border_color.g,
                            spec.border_color.b,
                            spec.border_color.a,
                        ];
                        let bump = spec.liquid && runtime.motion.appearance.border_radius < 12.0;
                        if bump {
                            runtime.motion.appearance.border_radius = 20.0;
                        }
                        runtime.preview_frame = None;
                        preview.queue_draw();
                        bump
                    };
                    if bump_radius {
                        if let Some(slider) = radius_slider_slot.borrow().as_ref() {
                            slider.set_value(20.0);
                        }
                    }
                    for (candidate, btn) in style_buttons.borrow().iter() {
                        if *candidate == style {
                            btn.add_css_class("active-background-option");
                        } else {
                            btn.remove_css_class("active-background-option");
                        }
                    }
                }
            });
            cell.append(&button);
            let label = Label::new(Some(&t(style.label())));
            label.add_css_class("editor-frame-tile-label");
            label.set_xalign(0.5);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.set_max_width_chars(9);
            cell.append(&label);
            style_grid.attach(&cell, (index % 3) as i32, (index / 3) as i32, 1, 1);
        }
    }
    border_section.append(&style_grid);

    // The radius rounds the captured image card itself; the background
    // scene stays a full rectangle. It applies on top of the Style preset.
    let radius = motion_reference_percent_slider("Border Radius", 0.0, 40.0, initial_border_radius);
    *radius_slider_slot.borrow_mut() = Some(radius.clone());
    radius.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.border_radius = slider.value();
            runtime.preview_frame = None;
            preview.queue_draw();
        }
    });
    border_section.append(&radius.widget());
    root.append(&border_section);

    // Frame section: collapsed W/H inputs plus an expandable ratio grid.
    // Collapsed shows the current canvas size (original dims for Standard,
    // export size otherwise) and an arrow reveals aspect-correct shape tiles.
    // The 3-column grid is sized to the fixed sidebar width so expanding
    // never widens the panel. Selection paints only on the diagram itself.
    fn frame_ratio_shape(aspect: f64, selected: Rc<Cell<bool>>) -> DrawingArea {
        const MAX_W: f64 = 44.0;
        const MAX_H: f64 = 46.0;
        let (w, h) = if aspect >= 1.0 {
            let w = MAX_W;
            (w, (w / aspect).max(10.0))
        } else {
            let h = MAX_H;
            ((h * aspect).max(10.0), h)
        };
        let area = DrawingArea::new();
        area.set_content_width(w.ceil() as i32);
        area.set_content_height(h.ceil() as i32);
        area.set_can_target(false);
        area.set_hexpand(false);
        area.set_halign(Align::Center);
        area.set_valign(Align::Center);
        area.add_css_class("editor-frame-tile-shape");
        area.set_draw_func(move |_, cr, width, height| {
            let is_selected = selected.get();
            let w = f64::from(width);
            let h = f64::from(height);
            if is_selected {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.13);
            }
            motion_thumbnail_rounded_rectangle(cr, 0.5, 0.5, w - 1.0, h - 1.0, 8.0);
            cr.fill().ok();
            if is_selected {
                cr.set_source_rgb(0.75, 0.40, 0.25);
                cr.set_line_width(2.0);
                motion_thumbnail_rounded_rectangle(cr, 1.5, 1.5, w - 3.0, h - 3.0, 7.0);
                cr.stroke().ok();
            }
        });
        area
    }
    let frame_section = motion_appearance_section("Frame");
    frame_section.set_hexpand(false);
    frame_section.set_halign(Align::Fill);
    // Original canvas size backs Standard: W/H show the source image, not a
    // fixed export default, so there is no separate Original button.
    let (orig_w, orig_h) = {
        let runtime = session.runtime.borrow();
        if let Some(snap) = runtime.snapshot.as_ref() {
            let (w, h) = snap.dimensions();
            (w as i32, h as i32)
        } else if let Some(card) = runtime.card.as_ref() {
            (card.width(), card.height())
        } else {
            (1920, 1080)
        }
    };
    let initial_frame: MotionFrame = {
        let runtime = session.runtime.borrow();
        runtime.motion.frame.clone()
    };
    let (initial_out_w, initial_out_h) = if initial_frame.preset == MotionFramePreset::Standard {
        (orig_w, orig_h)
    } else {
        initial_frame.output_size()
    };
    // (preset, aspect, selected flag, shape area, button) for diagram-only sync.
    let frame_tiles: Rc<
        RefCell<Vec<(MotionFramePreset, f64, Rc<Cell<bool>>, DrawingArea, Button)>>,
    > = Rc::new(RefCell::new(Vec::new()));
    // Single-select by exact preset: each tile owns one preset (social sizes
    // carry fixed pixel dims), so only the picked tile strokes. Legacy files
    // that persist a bare ratio fall back to the matching generic tile.
    let sync_frame_selection = {
        let frame_tiles = frame_tiles.clone();
        Rc::new(move |frame: &MotionFrame| {
            let mut exact_hit = false;
            for (preset, _, _, _, button) in frame_tiles.borrow().iter() {
                button.remove_css_class("active-background-option");
                if *preset == frame.preset {
                    exact_hit = true;
                }
            }
            if exact_hit {
                for (preset, _, flag, area, _) in frame_tiles.borrow().iter() {
                    let active = *preset == frame.preset;
                    flag.set(active);
                    area.queue_draw();
                }
                return;
            }
            // No exact tile (Standard, Custom, or a legacy bare ratio):
            // highlight at most one generic twin so old files still read.
            let current_aspect = frame.effective_aspect();
            let is_fixed = !matches!(
                frame.preset,
                MotionFramePreset::Standard | MotionFramePreset::Custom
            );
            let mut fallback: Option<MotionFramePreset> = None;
            if is_fixed {
                for (preset, _, _, _, _) in frame_tiles.borrow().iter() {
                    let is_generic = matches!(
                        preset,
                        MotionFramePreset::SixteenNine
                            | MotionFramePreset::ThreeTwo
                            | MotionFramePreset::FourThree
                            | MotionFramePreset::FiveFour
                            | MotionFramePreset::OneOne
                            | MotionFramePreset::FourFive
                            | MotionFramePreset::ThreeFour
                            | MotionFramePreset::TwoThree
                            | MotionFramePreset::NineSixteen
                            | MotionFramePreset::TenTwentyOne
                    );
                    if !is_generic {
                        continue;
                    }
                    match (current_aspect, preset.aspect()) {
                        (Some(a), Some(b)) if (a - b).abs() < 0.01 => {
                            fallback = Some(*preset);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            for (preset, _, flag, area, _) in frame_tiles.borrow().iter() {
                flag.set(fallback == Some(*preset));
                area.queue_draw();
            }
        })
    };
    let w_entry = Entry::new();
    w_entry.set_text(&initial_out_w.to_string());
    w_entry.set_width_chars(4);
    w_entry.set_max_width_chars(4);
    w_entry.set_hexpand(true);
    w_entry.set_halign(Align::Fill);
    w_entry.add_css_class("editor-frame-dim-entry");
    w_entry.set_tooltip_text(Some(&t("Frame width in pixels")));
    let h_entry = Entry::new();
    h_entry.set_text(&initial_out_h.to_string());
    h_entry.set_width_chars(4);
    h_entry.set_max_width_chars(4);
    h_entry.set_hexpand(true);
    h_entry.set_halign(Align::Fill);
    h_entry.add_css_class("editor-frame-dim-entry");
    h_entry.set_tooltip_text(Some(&t("Frame height in pixels")));
    // Clicking the active tile toggles back to Standard (original canvas),
    // so no separate Original button is needed. Toggle is exact-preset:
    // same-aspect social sizes switch size instead of resetting.
    let apply_preset: Rc<dyn Fn(MotionFramePreset)> = {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let w_entry = w_entry.clone();
        let h_entry = h_entry.clone();
        let sync_frame_selection = sync_frame_selection.clone();
        let notify = notify_interact.clone();
        Rc::new(move |preset| {
            notify();
            let (frame, display) = {
                let mut runtime = runtime.borrow_mut();
                let should_reset = runtime.motion.frame.preset == preset;
                runtime.begin_motion_edit();
                if should_reset {
                    runtime.motion.frame.preset = MotionFramePreset::Standard;
                } else {
                    runtime.motion.frame.preset = preset;
                }
                runtime.backdrop_cache = None;
                // Instant feedback like background picks: don't show the old
                // worker frame while the new aspect renders.
                runtime.preview_frame = None;
                let frame = runtime.motion.frame.clone();
                let display = if frame.preset == MotionFramePreset::Standard {
                    (orig_w, orig_h)
                } else {
                    frame.output_size()
                };
                (frame, display)
            };
            preview.queue_draw();
            sync_frame_selection(&frame);
            let (w, h) = display;
            if w_entry.text().as_str() != w.to_string() {
                w_entry.set_text(&w.to_string());
            }
            if h_entry.text().as_str() != h.to_string() {
                h_entry.set_text(&h.to_string());
            }
        })
    };
    let apply_custom: Rc<dyn Fn()> = {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let w_entry = w_entry.clone();
        let h_entry = h_entry.clone();
        let sync_frame_selection = sync_frame_selection.clone();
        let notify = notify_interact.clone();
        Rc::new(move || {
            notify();
            fn parse_dim(text: &str) -> Option<u32> {
                text.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|v| *v >= 16 && *v <= 7680)
            }
            let w_text = w_entry.text().to_string();
            let h_text = h_entry.text().to_string();
            let (Some(w), Some(h)) = (parse_dim(&w_text), parse_dim(&h_text)) else {
                return;
            };
            let frame = {
                let mut runtime = runtime.borrow_mut();
                if runtime.motion.frame.preset == MotionFramePreset::Custom
                    && runtime.motion.frame.custom_width == w
                    && runtime.motion.frame.custom_height == h
                {
                    return;
                }
                runtime.begin_motion_edit();
                runtime.motion.frame.preset = MotionFramePreset::Custom;
                runtime.motion.frame.custom_width = w;
                runtime.motion.frame.custom_height = h;
                runtime.backdrop_cache = None;
                runtime.preview_frame = None;
                runtime.motion.frame.clone()
            };
            preview.queue_draw();
            sync_frame_selection(&frame);
            let (out_w, out_h) = frame.output_size();
            if w_entry.text().as_str() != out_w.to_string() {
                w_entry.set_text(&out_w.to_string());
            }
            if h_entry.text().as_str() != out_h.to_string() {
                h_entry.set_text(&out_h.to_string());
            }
        })
    };
    {
        let apply_custom = apply_custom.clone();
        w_entry.connect_activate(move |_| apply_custom());
    }
    {
        let apply_custom = apply_custom.clone();
        h_entry.connect_activate(move |_| apply_custom());
    }
    {
        let focus = gtk4::EventControllerFocus::new();
        let apply_custom = apply_custom.clone();
        focus.connect_leave(move |_| apply_custom());
        w_entry.add_controller(focus);
    }
    {
        let focus = gtk4::EventControllerFocus::new();
        let apply_custom = apply_custom.clone();
        focus.connect_leave(move |_| apply_custom());
        h_entry.add_controller(focus);
    }
    let dim_row = GtkBox::new(Orientation::Horizontal, 6);
    dim_row.add_css_class("editor-frame-dim-row");
    dim_row.set_hexpand(false);
    dim_row.set_halign(Align::Fill);
    let w_pill = GtkBox::new(Orientation::Horizontal, 6);
    w_pill.add_css_class("editor-frame-dim-pill");
    w_pill.set_hexpand(true);
    w_pill.set_halign(Align::Fill);
    let w_unit = Label::new(Some("W"));
    w_unit.add_css_class("editor-frame-dim-unit");
    w_unit.set_hexpand(false);
    w_pill.append(&w_unit);
    w_pill.append(&w_entry);
    let h_pill = GtkBox::new(Orientation::Horizontal, 6);
    h_pill.add_css_class("editor-frame-dim-pill");
    h_pill.set_hexpand(true);
    h_pill.set_halign(Align::Fill);
    let h_unit = Label::new(Some("H"));
    h_unit.add_css_class("editor-frame-dim-unit");
    h_unit.set_hexpand(false);
    h_pill.append(&h_unit);
    h_pill.append(&h_entry);
    let expand_button = Button::with_label("\u{2304}");
    expand_button.set_has_frame(false);
    expand_button.add_css_class("editor-frame-expand-button");
    expand_button.set_tooltip_text(Some(&t("More frame sizes")));
    expand_button.set_hexpand(false);
    expand_button.set_halign(Align::Center);
    expand_button.set_valign(Align::Center);
    dim_row.append(&w_pill);
    dim_row.append(&h_pill);
    dim_row.append(&expand_button);
    let frame_revealer = Revealer::new();
    frame_revealer.set_reveal_child(false);
    frame_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    frame_revealer.set_transition_duration(180);
    frame_revealer.set_hexpand(false);
    frame_revealer.set_halign(Align::Fill);
    let frame_expanded = GtkBox::new(Orientation::Vertical, 10);
    frame_expanded.add_css_class("editor-frame-expanded");
    frame_expanded.set_hexpand(false);
    frame_expanded.set_halign(Align::Fill);
    frame_revealer.set_child(Some(&frame_expanded));
    {
        let frame_revealer = frame_revealer.clone();
        let notify = notify_interact.clone();
        expand_button.clone().connect_clicked(move |button| {
            notify();
            let revealed = frame_revealer.reveals_child();
            frame_revealer.set_reveal_child(!revealed);
            button.set_label(if revealed { "\u{2304}" } else { "\u{2303}" });
        });
    }
    // Local tile builder: shape plus one or two centered labels, fixed to
    // the sidebar width via small max widths and ellipsis. Selection lives
    // on the diagram draw, not the button chrome.
    let build_ratio_tile = {
        let frame_tiles = frame_tiles.clone();
        let apply_preset = apply_preset.clone();
        move |preset: MotionFramePreset,
              aspect: f64,
              line1: String,
              line2: Option<String>,
              tooltip: String|
              -> Button {
            let button = Button::new();
            button.set_has_frame(false);
            button.add_css_class("editor-frame-tile");
            button.set_tooltip_text(Some(&tooltip));
            button.set_hexpand(false);
            button.set_halign(Align::Center);
            let cell = GtkBox::new(Orientation::Vertical, 4);
            cell.set_hexpand(false);
            cell.set_halign(Align::Center);
            cell.add_css_class("editor-frame-tile-cell");
            let selected: Rc<Cell<bool>> = Rc::new(Cell::new(false));
            let shape = frame_ratio_shape(aspect, selected.clone());
            cell.append(&shape);
            let top = Label::new(Some(&line1));
            top.add_css_class("editor-frame-tile-label");
            top.set_xalign(0.5);
            top.set_halign(Align::Center);
            top.set_hexpand(false);
            top.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            top.set_max_width_chars(8);
            cell.append(&top);
            if let Some(second) = line2 {
                let bottom = Label::new(Some(&second));
                bottom.add_css_class("editor-frame-tile-sublabel");
                bottom.set_xalign(0.5);
                bottom.set_halign(Align::Center);
                bottom.set_hexpand(false);
                bottom.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                bottom.set_max_width_chars(8);
                cell.append(&bottom);
            }
            button.set_child(Some(&cell));
            {
                let apply_preset = apply_preset.clone();
                button.connect_clicked(move |_| apply_preset(preset));
            }
            frame_tiles
                .borrow_mut()
                .push((preset, aspect, selected, shape, button.clone()));
            button
        }
    };
    let generic_grid = Grid::new();
    generic_grid.add_css_class("editor-frame-grid");
    generic_grid.set_column_homogeneous(true);
    generic_grid.set_row_homogeneous(false);
    generic_grid.set_column_spacing(6);
    generic_grid.set_row_spacing(8);
    generic_grid.set_hexpand(false);
    generic_grid.set_halign(Align::Fill);
    let generic_ratios: [(MotionFramePreset, f64, &str, &str); 9] = [
        (
            MotionFramePreset::SixteenNine,
            16.0 / 9.0,
            "16:9",
            "Widescreen 16:9 output",
        ),
        (
            MotionFramePreset::ThreeTwo,
            3.0 / 2.0,
            "3:2",
            "Photo 3:2 output",
        ),
        (
            MotionFramePreset::FourThree,
            4.0 / 3.0,
            "4:3",
            "Fullscreen 4:3 output",
        ),
        (
            MotionFramePreset::FiveFour,
            5.0 / 4.0,
            "5:4",
            "Large format 5:4 output",
        ),
        (MotionFramePreset::OneOne, 1.0, "1:1", "Square 1:1 output"),
        (
            MotionFramePreset::FourFive,
            4.0 / 5.0,
            "4:5",
            "Portrait 4:5 output",
        ),
        (
            MotionFramePreset::ThreeFour,
            3.0 / 4.0,
            "3:4",
            "Portrait 3:4 output",
        ),
        (
            MotionFramePreset::TwoThree,
            2.0 / 3.0,
            "2:3",
            "Portrait 2:3 output",
        ),
        (
            MotionFramePreset::NineSixteen,
            9.0 / 16.0,
            "9:16",
            "Vertical 9:16 output",
        ),
    ];
    for (index, (preset, aspect, label, tip)) in generic_ratios.into_iter().enumerate() {
        let tile = build_ratio_tile(preset, aspect, t(label), None, t(tip));
        let col = (index % 3) as i32;
        let row = (index / 3) as i32;
        generic_grid.attach(&tile, col, row, 1, 1);
    }
    frame_expanded.append(&generic_grid);
    let sep_one = Separator::new(Orientation::Horizontal);
    sep_one.add_css_class("editor-frame-separator");
    sep_one.set_hexpand(false);
    sep_one.set_halign(Align::Fill);
    frame_expanded.append(&sep_one);
    let instagram_header = Label::new(Some(&t("Instagram")));
    instagram_header.add_css_class("editor-frame-subheader");
    instagram_header.set_xalign(0.0);
    instagram_header.set_halign(Align::Fill);
    instagram_header.set_hexpand(false);
    instagram_header.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    frame_expanded.append(&instagram_header);
    let instagram_grid = Grid::new();
    instagram_grid.add_css_class("editor-frame-grid");
    instagram_grid.set_column_homogeneous(true);
    instagram_grid.set_row_homogeneous(false);
    instagram_grid.set_column_spacing(6);
    instagram_grid.set_row_spacing(8);
    instagram_grid.set_hexpand(false);
    instagram_grid.set_halign(Align::Fill);
    let instagram_tiles: [(MotionFramePreset, f64, &str, &str, &str); 3] = [
        (
            MotionFramePreset::InstagramPost,
            1.0,
            "Post",
            "1:1",
            "Instagram Post 1080x1080",
        ),
        (
            MotionFramePreset::InstagramPortrait,
            4.0 / 5.0,
            "Portrait",
            "4:5",
            "Instagram Portrait 1080x1350",
        ),
        (
            MotionFramePreset::InstagramStory,
            9.0 / 16.0,
            "Story",
            "9:16",
            "Instagram Story 1080x1920",
        ),
    ];
    for (index, (preset, aspect, name, ratio, tip)) in instagram_tiles.into_iter().enumerate() {
        let tile = build_ratio_tile(preset, aspect, t(name), Some(t(ratio)), t(tip));
        instagram_grid.attach(&tile, index as i32, 0, 1, 1);
    }
    frame_expanded.append(&instagram_grid);
    let sep_two = Separator::new(Orientation::Horizontal);
    sep_two.add_css_class("editor-frame-separator");
    sep_two.set_hexpand(false);
    sep_two.set_halign(Align::Fill);
    frame_expanded.append(&sep_two);
    let twitter_header = Label::new(Some(&t("Twitter")));
    twitter_header.add_css_class("editor-frame-subheader");
    twitter_header.set_xalign(0.0);
    twitter_header.set_halign(Align::Fill);
    twitter_header.set_hexpand(false);
    twitter_header.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    frame_expanded.append(&twitter_header);
    let twitter_grid = Grid::new();
    twitter_grid.add_css_class("editor-frame-grid");
    twitter_grid.set_column_homogeneous(true);
    twitter_grid.set_row_homogeneous(false);
    twitter_grid.set_column_spacing(6);
    twitter_grid.set_row_spacing(8);
    twitter_grid.set_hexpand(false);
    twitter_grid.set_halign(Align::Fill);
    let twitter_tiles: [(MotionFramePreset, f64, &str, &str, &str); 2] = [
        (
            MotionFramePreset::TwitterTweet,
            16.0 / 9.0,
            "Tweet",
            "16:9",
            "Twitter Tweet 1200x675",
        ),
        (
            MotionFramePreset::TwitterCover,
            3.0,
            "Cover",
            "3:1",
            "Twitter Cover 1500x500",
        ),
    ];
    for (index, (preset, aspect, name, ratio, tip)) in twitter_tiles.into_iter().enumerate() {
        let tile = build_ratio_tile(preset, aspect, t(name), Some(t(ratio)), t(tip));
        twitter_grid.attach(&tile, index as i32, 0, 1, 1);
    }
    frame_expanded.append(&twitter_grid);
    let sep_three = Separator::new(Orientation::Horizontal);
    sep_three.add_css_class("editor-frame-separator");
    sep_three.set_hexpand(false);
    sep_three.set_halign(Align::Fill);
    frame_expanded.append(&sep_three);
    let youtube_header = Label::new(Some(&t("YouTube")));
    youtube_header.add_css_class("editor-frame-subheader");
    youtube_header.set_xalign(0.0);
    youtube_header.set_halign(Align::Fill);
    youtube_header.set_hexpand(false);
    youtube_header.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    frame_expanded.append(&youtube_header);
    let youtube_grid = Grid::new();
    youtube_grid.add_css_class("editor-frame-grid");
    youtube_grid.set_column_homogeneous(true);
    youtube_grid.set_row_homogeneous(false);
    youtube_grid.set_column_spacing(6);
    youtube_grid.set_row_spacing(8);
    youtube_grid.set_hexpand(false);
    youtube_grid.set_halign(Align::Fill);
    let youtube_tiles: [(MotionFramePreset, f64, &str, &str, &str); 3] = [
        (
            MotionFramePreset::YouTubeBanner,
            16.0 / 9.0,
            "Banner",
            "16:9",
            "YouTube Banner 2560x1440",
        ),
        (
            MotionFramePreset::YouTubeThumbnail,
            16.0 / 9.0,
            "Thumbnail",
            "16:9",
            "YouTube Thumbnail 1280x720",
        ),
        (
            MotionFramePreset::YouTubeVideo,
            16.0 / 9.0,
            "Video",
            "16:9",
            "YouTube Video 1920x1080",
        ),
    ];
    for (index, (preset, aspect, name, ratio, tip)) in youtube_tiles.into_iter().enumerate() {
        let tile = build_ratio_tile(preset, aspect, t(name), Some(t(ratio)), t(tip));
        youtube_grid.attach(&tile, index as i32, 0, 1, 1);
    }
    frame_expanded.append(&youtube_grid);
    let sep_four = Separator::new(Orientation::Horizontal);
    sep_four.add_css_class("editor-frame-separator");
    sep_four.set_hexpand(false);
    sep_four.set_halign(Align::Fill);
    frame_expanded.append(&sep_four);
    let pinterest_header = Label::new(Some(&t("Pinterest")));
    pinterest_header.add_css_class("editor-frame-subheader");
    pinterest_header.set_xalign(0.0);
    pinterest_header.set_halign(Align::Fill);
    pinterest_header.set_hexpand(false);
    pinterest_header.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    frame_expanded.append(&pinterest_header);
    let pinterest_grid = Grid::new();
    pinterest_grid.add_css_class("editor-frame-grid");
    pinterest_grid.set_column_homogeneous(true);
    pinterest_grid.set_row_homogeneous(false);
    pinterest_grid.set_column_spacing(6);
    pinterest_grid.set_row_spacing(8);
    pinterest_grid.set_hexpand(false);
    pinterest_grid.set_halign(Align::Fill);
    let pinterest_tiles: [(MotionFramePreset, f64, &str, &str, &str); 3] = [
        (
            MotionFramePreset::PinterestLong,
            10.0 / 21.0,
            "Long",
            "10:21",
            "Pinterest Long 1000x2100",
        ),
        (
            MotionFramePreset::PinterestOptimal,
            2.0 / 3.0,
            "Optimal",
            "2:3",
            "Pinterest Optimal 1000x1500",
        ),
        (
            MotionFramePreset::PinterestSquare,
            1.0,
            "Square",
            "1:1",
            "Pinterest Square 1000x1000",
        ),
    ];
    for (index, (preset, aspect, name, ratio, tip)) in pinterest_tiles.into_iter().enumerate() {
        let tile = build_ratio_tile(preset, aspect, t(name), Some(t(ratio)), t(tip));
        pinterest_grid.attach(&tile, index as i32, 0, 1, 1);
    }
    frame_expanded.append(&pinterest_grid);
    sync_frame_selection(&initial_frame);
    frame_section.append(&dim_row);
    frame_section.append(&frame_revealer);
    root.append(&frame_section);

    // Scene Shadows: an independent overlay layer with its own
    // preset id, opacity, and above/below-card placement — deliberately not
    // the card's Border/Shadow drop shadow. The presets are procedural
    // shading.
    let scene_shadow_section = motion_appearance_section("Scene Shadows");
    // Exports render an unset background fill as a solid black scene, so a
    // dark shadow over it cannot be seen. Say so instead of leaving users to
    // discover a missing effect in their MP4.
    let shadow_hint = Label::new(Some(&t("Needs a background fill to appear in exports")));
    shadow_hint.add_css_class("editor-select-inspector-hint");
    shadow_hint.set_xalign(0.0);
    shadow_hint.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    scene_shadow_section.append(&shadow_hint);
    let initial_scene_shadow = {
        let runtime = session.runtime.borrow();
        runtime.motion.scene_shadow.clone()
    };
    let shadow_preset_buttons: Rc<RefCell<Vec<(MotionSceneShadowPreset, ToggleButton)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let shadow_preset_rows = GtkBox::new(Orientation::Vertical, 6);
    let mut shadow_row: Option<GtkBox> = None;
    for (index, preset) in MotionSceneShadowPreset::ALL.iter().enumerate() {
        if index % 2 == 0 {
            let row = GtkBox::new(Orientation::Horizontal, 6);
            row.set_homogeneous(true);
            shadow_preset_rows.append(&row);
            shadow_row = Some(row);
        }
        let button = ToggleButton::with_label(&t(preset.label()));
        button.set_has_frame(false);
        button.set_hexpand(true);
        button.add_css_class("recording-editor-zoom-easing-btn");
        button.set_active(initial_scene_shadow.preset == *preset);
        button.connect_toggled({
            let runtime = session.runtime.clone();
            let preview = preview.clone();
            let notify = notify_interact.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                notify();
                {
                    let mut runtime = runtime.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.scene_shadow.preset = *preset;
                    preview.queue_draw();
                }
            }
        });
        if let Some((_, first)) = shadow_preset_buttons.borrow().first() {
            button.set_group(Some(first));
        }
        shadow_preset_buttons
            .borrow_mut()
            .push((*preset, button.clone()));
        if let Some(row) = shadow_row.as_ref() {
            row.append(&button);
        }
    }
    scene_shadow_section.append(&shadow_preset_rows);

    let shadow_opacity = motion_appearance_slider(
        "Shadow opacity",
        0.0,
        1.0,
        initial_scene_shadow.opacity,
        "%",
    );
    shadow_opacity.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let notify = notify_interact.clone();
        move |slider| {
            notify();
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.scene_shadow.opacity = slider.value();
            preview.queue_draw();
        }
    });
    scene_shadow_section.append(&shadow_opacity.widget());

    let shadow_placement_buttons: Rc<RefCell<Vec<(MotionSceneShadowPlacement, ToggleButton)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let placement_row = GtkBox::new(Orientation::Horizontal, 6);
    placement_row.set_homogeneous(true);
    for (placement, label, tooltip) in [
        (
            MotionSceneShadowPlacement::Underlay,
            t("Underlay"),
            t("Shade beneath the card"),
        ),
        (
            MotionSceneShadowPlacement::Overlay,
            t("Overlay"),
            t("Shade above the card"),
        ),
    ] {
        let button = ToggleButton::with_label(&label);
        button.set_has_frame(false);
        button.set_hexpand(true);
        button.add_css_class("recording-editor-zoom-easing-btn");
        button.set_tooltip_text(Some(&tooltip));
        button.set_active(initial_scene_shadow.placement == placement);
        button.connect_toggled({
            let runtime = session.runtime.clone();
            let preview = preview.clone();
            let notify = notify_interact.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                notify();
                {
                    let mut runtime = runtime.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.scene_shadow.placement = placement;
                    preview.queue_draw();
                }
            }
        });
        if let Some((_, first)) = shadow_placement_buttons.borrow().first() {
            button.set_group(Some(first));
        }
        shadow_placement_buttons
            .borrow_mut()
            .push((placement, button.clone()));
        placement_row.append(&button);
    }
    scene_shadow_section.append(&placement_row);
    root.append(&scene_shadow_section);
    root
}

fn motion_appearance_section(title: &str) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-motion-settings-section");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("editor-background-section-title");
    heading.set_xalign(0.0);
    section.append(&heading);
    section
}

// Decoded wallpaper preview surfaces, shared by both Appearance panels
// (motion + static-shared) and every grid row. Without this the shared
// Background tool decodes each thumb twice on startup, blocking open.
// Main-thread only (GTK), so thread-local: cairo surfaces are !Sync.
// ponytail: one cache, not per-panel decode.
thread_local! {
    static WALLPAPER_PREVIEW_CACHE: RefCell<HashMap<String, gtk4::cairo::ImageSurface>> =
        RefCell::new(HashMap::new());
}

/// Bundled wallpaper thumbs are 256px wide; previews in the strip are 56px.
const WALLPAPER_THUMB_MAX_EDGE: u32 = 256;

fn cached_wallpaper_preview_surface(path: &str) -> Option<gtk4::cairo::ImageSurface> {
    if let Some(hit) = WALLPAPER_PREVIEW_CACHE.with(|cache| cache.borrow().get(path).cloned()) {
        return Some(hit);
    }
    let surface = crate::capture::editor::window::background_panel::load_background_preview_image(
        std::path::Path::new(path),
        WALLPAPER_THUMB_MAX_EDGE,
    )
    .and_then(|image| crate::capture::editor::render::rgba_image_to_surface(&image))?;
    WALLPAPER_PREVIEW_CACHE.with(|cache| {
        cache.borrow_mut().insert(path.to_owned(), surface.clone());
    });
    Some(surface)
}

/// Motion uses the app's bundled background catalog rather than requiring a
/// file chooser for the common Wallpaper path.  The picker intentionally
/// progresses from a short strip → the full grid, keeping the inspector
/// compact until someone asks to browse the catalog.
fn motion_wallpaper_catalog_section(
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    on_interact: &Rc<dyn Fn()>,
) -> (GtkBox, Rc<dyn Fn()>) {
    let catalog = GtkBox::new(Orientation::Vertical, 6);
    catalog.add_css_class("editor-motion-wallpaper-catalog");
    let stack = Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    catalog.append(&stack);

    let compact = GtkBox::new(Orientation::Vertical, 6);
    let compact_title = Label::new(Some(&t("Wallpapers")));
    compact_title.add_css_class("editor-background-section-title");
    compact_title.set_xalign(0.0);
    compact.append(&compact_title);
    let compact_row = GtkBox::new(Orientation::Horizontal, 8);
    compact_row.add_css_class("editor-motion-wallpaper-row");
    compact.append(&compact_row);
    stack.add_named(&compact, Some("compact"));

    let all = GtkBox::new(Orientation::Vertical, 6);
    let all_header = GtkBox::new(Orientation::Horizontal, 6);
    let all_title = Label::new(Some(&t("Wallpapers")));
    all_title.add_css_class("editor-background-section-title");
    all_title.set_xalign(0.0);
    all_title.set_hexpand(true);
    let show_less = Button::with_label(&t("Show less"));
    show_less.set_has_frame(false);
    show_less.add_css_class("editor-background-section-action-button");
    all_header.append(&all_title);
    all_header.append(&show_less);
    all.append(&all_header);
    let all_grid = GtkBox::new(Orientation::Vertical, 6);
    all_grid.add_css_class("editor-motion-wallpaper-grid");
    all.append(&all_grid);
    stack.add_named(&all, Some("all"));

    let paths: Vec<(PathBuf, PathBuf)> =
        crate::capture::editor::window::background_panel::MOTION_WALLPAPER_FILES
            .iter()
            .filter_map(|file_name| {
                let path = crate::capture::editor::window::background_panel::background_gradient_asset_path(file_name);
                path.is_file().then(|| {
                    let preview_path = crate::capture::editor::window::background_panel::motion_wallpaper_preview_asset_path(file_name);
                    (path, preview_path)
                })
            })
            .collect();
    let selection_buttons = Rc::new(RefCell::new(Vec::<(PathBuf, Button)>::new()));
    for (path, preview_path) in paths.iter().take(3) {
        compact_row.append(&motion_wallpaper_thumbnail(
            path,
            preview_path,
            session,
            preview,
            none_button,
            selection_buttons.clone(),
            on_interact,
        ));
    }
    // Keep the compact strip eager, then populate one small row per frame when
    // expanded. This keeps the Show all click responsive even with a large
    // bundled catalog.
    let all_populated = Rc::new(Cell::new(false));
    let populate_all: Rc<dyn Fn()> = Rc::new({
        let all_grid = all_grid.clone();
        let paths = paths.clone();
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        let all_populated = all_populated.clone();
        let on_interact = on_interact.clone();
        move || {
            if all_populated.replace(true) {
                return;
            }
            let next_row = Rc::new(Cell::new(0usize));
            glib::timeout_add_local(Duration::from_millis(12), {
                let all_grid = all_grid.clone();
                let paths = paths.clone();
                let session = session.clone();
                let preview = preview.clone();
                let none_button = none_button.clone();
                let selection_buttons = selection_buttons.clone();
                let next_row = next_row.clone();
                let on_interact = on_interact.clone();
                move || {
                    let start = next_row.get();
                    if start >= paths.len() {
                        return glib::ControlFlow::Break;
                    }
                    let row_paths = &paths[start..(start + 4).min(paths.len())];
                    next_row.set(start + row_paths.len());
                    let row = GtkBox::new(Orientation::Horizontal, 6);
                    row.add_css_class("editor-motion-wallpaper-row");
                    for (path, preview_path) in row_paths {
                        row.append(&motion_wallpaper_thumbnail(
                            path,
                            preview_path,
                            &session,
                            &preview,
                            &none_button,
                            selection_buttons.clone(),
                            &on_interact,
                        ));
                    }
                    all_grid.append(&row);
                    glib::ControlFlow::Continue
                }
            });
        }
    });
    if let Some((_, preview_path)) = paths.get(3) {
        compact_row.append(&motion_wallpaper_stack_thumbnail(
            preview_path,
            &stack,
            populate_all,
            on_interact,
        ));
    }

    show_less.connect_clicked({
        let stack = stack.clone();
        let notify = on_interact.clone();
        move |_| {
            notify();
            stack.set_visible_child_name("compact")
        }
    });
    stack.set_visible_child_name("compact");
    let activate_catalog: Rc<dyn Fn()> = Rc::new({
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        let default_entry = paths.first().cloned();
        let notify = on_interact.clone();
        move || {
            notify();
            let Some((default_path, default_preview)) = default_entry.clone() else {
                return;
            };
            let needs_switch = !matches!(
                session
                    .runtime
                    .borrow()
                    .motion
                    .appearance
                    .background_fill_type,
                MotionBackgroundFillType::Wallpaper
            );
            if needs_switch {
                {
                    let mut runtime = session.runtime.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.appearance.wallpaper_image_name =
                        Some(default_path.to_string_lossy().into_owned());
                    runtime.motion.appearance.background_fill_type =
                        MotionBackgroundFillType::Wallpaper;
                    runtime.motion.appearance.custom_background_image = None;
                    // Cached thumb now (no decode jank), full image off-thread.
                    // ponytail: never sync-decode full wallpaper on click.
                    runtime.set_background_surface(
                        Some(default_path.to_string_lossy().into_owned()),
                        cached_wallpaper_preview_surface(&default_preview.to_string_lossy()),
                        true,
                    );
                    none_button.remove_css_class("active-background-option");
                    preview.queue_draw();
                }
                load_motion_wallpaper_asynchronously(
                    default_path.clone(),
                    session.clone(),
                    preview.clone(),
                );
            }
            let selected_path = session
                .runtime
                .borrow()
                .motion
                .appearance
                .wallpaper_image_name
                .clone();
            for (path, button) in selection_buttons.borrow().iter() {
                if Some(path.to_string_lossy().as_ref()) == selected_path.as_deref() {
                    button.add_css_class("active-background-option");
                } else {
                    button.remove_css_class("active-background-option");
                }
            }
        }
    });
    (catalog, activate_catalog)
}

fn motion_wallpaper_thumbnail(
    path: &std::path::Path,
    preview_path: &std::path::Path,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    selection_buttons: Rc<RefCell<Vec<(PathBuf, Button)>>>,
    on_interact: &Rc<dyn Fn()>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-wallpaper-thumbnail");
    button.set_tooltip_text(path.file_stem().and_then(|name| name.to_str()));
    let path = path.to_path_buf();
    let preview_path = preview_path.to_path_buf();
    // Build empty so the panel opens instantly; fill thumb off the critical
    // path via cache (second panel + revisits are free). ponytail: async
    // thumbs, not sync decode on open.
    let thumbnail = motion_wallpaper_thumbnail_area(None);
    button.set_child(Some(&thumbnail));
    {
        let thumbnail = thumbnail.clone();
        let key = preview_path.to_string_lossy().into_owned();
        glib::idle_add_local_once(move || {
            if let Some(surface) = cached_wallpaper_preview_surface(&key) {
                thumbnail.set_draw_func(move |_, context, width, height| {
                    paint_wallpaper_thumb(context, &surface, width, height);
                });
                thumbnail.queue_draw();
            }
        });
    }

    if session
        .runtime
        .borrow()
        .motion
        .appearance
        .wallpaper_image_name
        .as_deref()
        == Some(path.to_string_lossy().as_ref())
    {
        button.add_css_class("active-background-option");
    }
    selection_buttons
        .borrow_mut()
        .push((path.clone(), button.clone()));
    button.connect_clicked({
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        let preview_path = preview_path.clone();
        let notify = on_interact.clone();
        move |_| {
            notify();
            // Re-clicking the active wallpaper must not re-decode + recomposite.
            // ponytail: early return, not another async load.
            let already_selected = {
                let runtime = session.runtime.borrow();
                runtime.motion.appearance.background_fill_type
                    == MotionBackgroundFillType::Wallpaper
                    && runtime.motion.appearance.wallpaper_image_name.as_deref()
                        == Some(path.to_string_lossy().as_ref())
            };
            if already_selected {
                return;
            }
            let mut runtime = session.runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.appearance.wallpaper_image_name =
                Some(path.to_string_lossy().into_owned());
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Wallpaper;
            // Replace, never stack: the other image slot must not survive or a
            // later round-trip can resurrect it behind the new fill.
            runtime.motion.appearance.custom_background_image = None;
            // The thumbnail is cached. Show it now, then replace it with the
            // full wallpaper from a worker thread. ponytail: cache hit, no decode.
            runtime.set_background_surface(
                Some(path.to_string_lossy().into_owned()),
                cached_wallpaper_preview_surface(&preview_path.to_string_lossy()),
                true,
            );
            for (candidate_path, candidate) in selection_buttons.borrow().iter() {
                if candidate_path == &path {
                    candidate.add_css_class("active-background-option");
                } else {
                    candidate.remove_css_class("active-background-option");
                }
            }
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
            load_motion_wallpaper_asynchronously(path.clone(), session.clone(), preview.clone());
        }
    });
    button
}

/// The fourth compact preview is the catalog's visual stack affordance.  It
/// remains a wallpaper thumbnail, with a small overlay indicating that it
/// opens the complete collection rather than selecting that particular image.
fn motion_wallpaper_stack_thumbnail(
    preview_path: &std::path::Path,
    stack: &Stack,
    populate_all: Rc<dyn Fn()>,
    on_interact: &Rc<dyn Fn()>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-wallpaper-thumbnail");
    button.add_css_class("editor-motion-wallpaper-stack-thumbnail");
    button.set_tooltip_text(Some(&t("Show all wallpapers")));
    let thumbnail = motion_wallpaper_thumbnail_area(None);
    {
        let thumbnail = thumbnail.clone();
        let key = preview_path.to_string_lossy().into_owned();
        glib::idle_add_local_once(move || {
            if let Some(surface) = cached_wallpaper_preview_surface(&key) {
                thumbnail.set_draw_func(move |_, context, width, height| {
                    paint_wallpaper_thumb(context, &surface, width, height);
                });
                thumbnail.queue_draw();
            }
        });
    }
    let overlay = Overlay::new();
    overlay.set_child(Some(&thumbnail));
    let expand = Label::new(Some("⌄"));
    expand.set_halign(Align::Center);
    expand.set_valign(Align::Center);
    expand.set_can_target(false);
    expand.add_css_class("editor-motion-wallpaper-stack-glyph");
    overlay.add_overlay(&expand);
    button.set_child(Some(&overlay));
    button.connect_clicked({
        let stack = stack.clone();
        let notify = on_interact.clone();
        move |_| {
            notify();
            populate_all();
            stack.set_visible_child_name("all");
        }
    });
    button
}

fn paint_wallpaper_thumb(
    context: &Context,
    surface: &gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
) {
    let source_w = f64::from(surface.width().max(1));
    let source_h = f64::from(surface.height().max(1));
    let scale = (f64::from(width) / source_w).max(f64::from(height) / source_h);
    let _ = context.save();
    motion_thumbnail_rounded_rectangle(
        context,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        11.0,
    );
    context.clip();
    context.translate(
        (f64::from(width) - source_w * scale) * 0.5,
        (f64::from(height) - source_h * scale) * 0.5,
    );
    context.scale(scale, scale);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.paint().ok();
    context.restore().ok();
}

fn motion_wallpaper_thumbnail_area(surface: Option<gtk4::cairo::ImageSurface>) -> DrawingArea {
    let thumbnail = DrawingArea::new();
    thumbnail.set_content_width(56);
    thumbnail.set_content_height(56);
    thumbnail.set_draw_func(move |_, context, width, height| {
        let Some(surface) = surface.as_ref() else {
            return;
        };
        paint_wallpaper_thumb(context, surface, width, height);
    });
    thumbnail
}

/// Decode a selected full-resolution wallpaper off the UI thread. The tile's
/// small preview is already assigned as a temporary background, so the
/// inspector remains responsive while the full image is prepared.
/// The decoded image is downscaled to a preview-bounded edge before crossing
/// to the UI thread: upload + cover-fit composite then stay cheap no matter
/// how large the source file is. Export reloads full-res from disk itself.
/// ponytail: bound preview pixels, not a loader pool; pool when still slow.
const PREVIEW_WALLPAPER_MAX_EDGE: u32 = 1600;

fn load_motion_wallpaper_asynchronously(
    path: PathBuf,
    session: MotionSession,
    preview: DrawingArea,
) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn({
        let path = path.clone();
        move || {
            // Bounded decode: DCT-scaled for JPEG, so the 8000x6000 catalog
            // entries cost a fraction of a full decode instead of seconds.
            let image =
                crate::capture::editor::window::background_panel::load_background_preview_image(
                    &path,
                    PREVIEW_WALLPAPER_MAX_EDGE,
                );
            let _ = sender.send((path, image));
        }
    });
    glib::timeout_add_local(Duration::from_millis(16), move || {
        match receiver.try_recv() {
            Ok((loaded_path, Some(image))) => {
                let mut runtime = session.runtime.borrow_mut();
                let still_selected = runtime.motion.appearance.background_fill_type
                    == MotionBackgroundFillType::Wallpaper
                    && runtime.motion.appearance.wallpaper_image_name.as_deref()
                        == Some(loaded_path.to_string_lossy().as_ref());
                if still_selected {
                    runtime.set_background_surface(
                        Some(loaded_path.to_string_lossy().into_owned()),
                        crate::capture::editor::render::rgba_image_to_surface(&image),
                        false,
                    );
                    preview.queue_draw();
                }
                glib::ControlFlow::Break
            }
            Ok((_, None)) | Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        }
    });
}

fn motion_thumbnail_rounded_rectangle(
    context: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    // Thumbnails preview the same smooth card outline as the canvas.
    crate::capture::editor::render::rounded_rect_path(context, x, y, width, height, radius);
}

fn motion_image_section(
    title: &str,
    dialog_title: &str,
    kind: MotionBackgroundFillType,
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    on_interact: &Rc<dyn Fn()>,
) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 6);
    let label = Label::new(Some(&t(title)));
    label.add_css_class("editor-background-section-title");
    label.set_xalign(0.0);
    let choose = Button::with_label(&t("Choose…"));
    choose.set_has_frame(false);
    choose.add_css_class("editor-sidebar-action-button");
    choose.connect_clicked({
        let window = window.downgrade();
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let dialog_title = dialog_title.to_string();
        let notify = on_interact.clone();
        move |_| {
            notify();
            let chooser = FileChooserNative::new(
                Some(&dialog_title),
                window.upgrade().as_ref(),
                FileChooserAction::Open,
                Some(&t("Choose")),
                Some(&t("Cancel")),
            );
            let filter = FileFilter::new();
            filter.set_name(Some(&t("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let session = session.clone();
            let preview = preview.clone();
            let kind = kind.clone();
            let none_button = none_button.clone();
            let notify_response = notify.clone();
            chooser.connect_response(move |dialog, response| {
                notify_response();
                if response != ResponseType::Accept {
                    return;
                }
                let Some(path) = dialog.file().and_then(|file| file.path()) else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                let mut runtime = session.runtime.borrow_mut();
                runtime.begin_motion_edit();
                match kind {
                    MotionBackgroundFillType::Wallpaper => {
                        runtime.motion.appearance.wallpaper_image_name = Some(path);
                        runtime.motion.appearance.custom_background_image = None;
                    }
                    MotionBackgroundFillType::Image => {
                        runtime.motion.appearance.custom_background_image = Some(path);
                        runtime.motion.appearance.wallpaper_image_name = None;
                    }
                    _ => return,
                }
                runtime.motion.appearance.background_fill_type = kind.clone();
                let active_path = match kind {
                    MotionBackgroundFillType::Wallpaper => runtime
                        .motion
                        .appearance
                        .wallpaper_image_name
                        .as_deref()
                        .map(str::to_owned),
                    MotionBackgroundFillType::Image => runtime
                        .motion
                        .appearance
                        .custom_background_image
                        .as_deref()
                        .map(str::to_owned),
                    _ => None,
                };
                let surface = active_path.as_deref().and_then(|path| {
                    super::super::motion_render::load_motion_background_preview_surface(
                        path,
                        PREVIEW_WALLPAPER_MAX_EDGE,
                    )
                });
                runtime.set_background_surface(active_path, surface, false);
                none_button.remove_css_class("active-background-option");
                preview.queue_draw();
            });
            chooser.show();
        }
    });
    section.append(&label);
    section.append(&choose);
    section
}

#[cfg(test)]
mod tests {
    #[test]
    fn frame_picker_collapses_to_manual_dims_with_expandable_shape_grid() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-frame-dim-row")
                && production_source.contains("editor-frame-dim-pill")
                && production_source.contains("w_entry")
                && production_source.contains("h_entry")
                && production_source.contains("frame_revealer")
                && production_source.contains("reveals_child")
                && production_source.contains("editor-frame-grid")
                && production_source.contains("frame_ratio_shape")
                && production_source.contains("\"Instagram\"")
                && production_source.contains("\"Twitter\"")
                && production_source.contains("\"YouTube\"")
                && production_source.contains("\"Pinterest\""),
            "Frame should collapse to W/H inputs with an arrow revealing shape tiles plus social groups",
        );
        assert!(
            !production_source.contains("frame_rows"),
            "old two-chip Frame rows should be replaced by the expandable grid",
        );
        assert!(
            !production_source.contains("t(\"Original canvas\")")
                && !production_source.contains("editor-frame-standard-button"),
            "Standard is the bare W/H state (original canvas dims); no separate Original button",
        );
    }

    #[test]
    fn frame_shape_tiles_fit_the_fixed_sidebar_width() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("set_column_homogeneous(true)")
                && production_source.contains("set_column_spacing(6)")
                && production_source.contains("index % 3")
                && production_source.contains("MAX_W")
                && production_source.contains("MAX_H")
                && production_source.contains("set_max_width_chars(8)")
                && production_source.contains("EllipsizeMode::End")
                && production_source.contains("set_hexpand(false)"),
            "ratio tiles must stay in a 3-column homogeneous grid with small capped shapes and ellipsized labels so expand never widens the panel",
        );
    }

    #[test]
    fn frame_selection_is_single_exact_with_fixed_social_sizes() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("TwitterTweet")
                && production_source.contains("TwitterCover")
                && production_source.contains("YouTubeBanner")
                && production_source.contains("YouTubeThumbnail")
                && production_source.contains("YouTubeVideo")
                && production_source.contains("InstagramPost")
                && production_source.contains("InstagramPortrait")
                && production_source.contains("InstagramStory")
                && production_source.contains("PinterestLong")
                && production_source.contains("PinterestOptimal")
                && production_source.contains("PinterestSquare"),
            "social tiles must own distinct fixed-size presets so W/H shows the picked size",
        );
        assert!(
            production_source.contains("*preset == frame.preset")
                && production_source.contains("runtime.motion.frame.preset == preset"),
            "selection and toggle must be exact-preset single-select, not aspect-wide multi-highlight",
        );
    }

    #[test]
    fn frame_selection_paints_on_diagrams_without_button_borders() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("frame_ratio_shape(aspect, selected")
                && production_source.contains("selected: Rc<Cell<bool>>")
                && production_source.contains("set_line_width(2.0)")
                && production_source.contains("should_reset"),
            "active ratio must stroke the diagram draw with toggle-back to Standard, not a button border",
        );
        let css = include_str!("../../css/12-background-choices.css");
        assert!(
            css.contains(".editor-frame-dim-pill:focus-within")
                && css.contains("outline: none")
                && css.contains("button.editor-frame-tile:hover"),
            "frame inputs/tiles must be borderless filled controls with inset focus, matching hex-entry/slider style",
        );
        assert!(
            !css.contains("button.editor-frame-tile.active-background-option {")
                || !css
                    .split("button.editor-frame-tile.active-background-option {")
                    .nth(1)
                    .unwrap_or("")
                    .split('}')
                    .next()
                    .unwrap_or("")
                    .contains("box-shadow: inset"),
            "tile buttons must not paint their own selection ring; the diagram stroke carries it",
        );
    }

    #[test]
    fn wallpaper_thumbs_share_cache_and_load_off_critical_path() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("WALLPAPER_PREVIEW_CACHE")
                && production_source.contains("cached_wallpaper_preview_surface")
                && production_source.contains("idle_add_local_once")
                && production_source.contains("load_motion_wallpaper_asynchronously(")
                && production_source.contains("PREVIEW_WALLPAPER_MAX_EDGE")
                && production_source.contains("already_selected"),
            "wallpaper thumbs must share one cache and never sync-decode full images on open/click",
        );
    }
}

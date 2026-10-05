use gtk4::cairo::{Context, Format, ImageSurface};
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, Entry,
    FileChooserAction, FileChooserNative, FileFilter, GestureClick, Grid, Image, Label,
    Orientation, PolicyType, ResponseType, Revealer, RevealerTransitionType, ScrolledWindow,
    Separator, Stack, ToggleButton,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::capture::editor::types::FrameStyle;
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::recording::editor::model::{
    GradientStop, MotionBackgroundFillType, MotionFrame, MotionFramePreset,
    MotionSceneShadowPlacement, MotionSceneShadowPreset, VideoGradient,
};
use crate::recording::editor::window::custom_wallpaper_popover::{
    build_custom_fill_popover, FillOps,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::widgets::{
    motion_appearance_slider, motion_reference_percent_slider, motion_rgb888, motion_rgba,
};
use super::MotionSession;

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
    // Each swatch runs a whole frame preview (backings, liquid gradient, border
    // strokes). The Style disclosure slides open by re-snapshotting its tiles
    // every frame, so render a swatch once per size and blit it after that
    // instead of re-running that cairo mid-animation, which is what made the
    // reveal stutter.
    let cache: Rc<RefCell<Option<(i32, i32, ImageSurface)>>> = Rc::new(RefCell::new(None));
    area.set_draw_func(move |_, cr, width, height| {
        let surface = {
            let mut cache = cache.borrow_mut();
            let stale = !matches!(cache.as_ref(), Some((w, h, _)) if *w == width && *h == height);
            if stale {
                *cache = render_frame_style_swatch(style, width, height)
                    .map(|surface| (width, height, surface));
            }
            cache.as_ref().map(|(_, _, surface)| surface.clone())
        };
        if let Some(surface) = surface {
            let _ = cr.set_source_surface(&surface, 0.0, 0.0);
            let _ = cr.paint();
        }
    });
    area
}

/// Render one frame-style swatch at `width`x`height` into a fresh surface, so
/// the draw func only ever blits it.
fn render_frame_style_swatch(style: FrameStyle, width: i32, height: i32) -> Option<ImageSurface> {
    let surface = ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)).ok()?;
    let cr = Context::new(&surface).ok()?;
    paint_frame_style_swatch(&cr, style, f64::from(width), f64::from(height));
    surface.flush();
    Some(surface)
}

/// Swatch body: a gray card with the preset's backings, liquid rim and
/// outside/inset border strokes, scaled to the tile.
fn paint_frame_style_swatch(cr: &Context, style: FrameStyle, w: f64, h: f64) {
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
    // Any pointer interaction in Appearance deactivates the Static tool.
    // Mutating controls also notify explicitly for keyboard/popover edits.
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

    // Fill-source tabs, matching the video editor's Background sidebar: one
    // pill tray that swaps the whole fill UI instead of stacking every source
    // into a single long scroll. Wired once the source stack exists below.
    let source_tabs = GtkBox::new(Orientation::Horizontal, 0);
    source_tabs.add_css_class("recording-editor-bg-tabs");
    source_tabs.set_hexpand(true);
    source_tabs.set_homogeneous(true);
    let wallpapers_tab = motion_source_tab("Wallpapers");
    let custom_tab = motion_source_tab("Custom");
    let image_tab = motion_source_tab("Image");
    custom_tab.set_group(Some(&wallpapers_tab));
    image_tab.set_group(Some(&wallpapers_tab));
    wallpapers_tab.set_active(true);
    source_tabs.append(&wallpapers_tab);
    source_tabs.append(&custom_tab);
    source_tabs.append(&image_tab);

    // The Custom tab's summary: a swatch of the current fill plus its kind, the
    // same "Color — Edit" row the video editor shows. The editors themselves
    // live in the video editor's Custom Wallpaper popover, opened by the Edit
    // pill, so this row only ever summarizes.
    let summary_label = Label::new(Some(&t(
        if initial_fill_type == MotionBackgroundFillType::Gradient {
            "Gradient"
        } else {
            "Color"
        },
    )));
    let summary_swatch = DrawingArea::new();
    summary_swatch.add_css_class("recording-editor-bg-custom-swatch");
    summary_swatch.set_content_width(28);
    summary_swatch.set_content_height(28);
    summary_swatch.set_valign(Align::Center);
    summary_swatch.set_can_target(false);
    // Redraw the summary chip from the stored fill: a ramp for a gradient, the
    // flat color otherwise, plus the row's label. The popover calls this on
    // every change, so the row stays current while the card is open.
    let redraw_summary: Rc<dyn Fn()> = {
        let swatch = summary_swatch.clone();
        let label = summary_label.clone();
        let runtime = session.runtime.clone();
        Rc::new(move || {
            let (is_gradient, solid, gradient) = {
                let runtime = runtime.borrow();
                let appearance = &runtime.motion.appearance;
                (
                    appearance.background_fill_type == MotionBackgroundFillType::Gradient,
                    appearance.background_color,
                    appearance.gradient.clone(),
                )
            };
            label.set_text(&t(if is_gradient { "Gradient" } else { "Color" }));
            swatch.set_draw_func(move |_, cr, width, height| {
                let (w, h) = (f64::from(width), f64::from(height));
                motion_thumbnail_rounded_rectangle(cr, 0.5, 0.5, w - 1.0, h - 1.0, 7.0);
                if is_gradient {
                    let stops = gradient.draw_stops();
                    let ramp = gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
                    if let (Some(first), Some(last)) = (stops.first(), stops.last()) {
                        let rgba = |stop: &GradientStop| {
                            (
                                f64::from(stop.r) / 255.0,
                                f64::from(stop.g) / 255.0,
                                f64::from(stop.b) / 255.0,
                                f64::from(stop.a) / 255.0,
                            )
                        };
                        let (r, g, b, a) = rgba(first);
                        ramp.add_color_stop_rgba(0.0, r, g, b, a);
                        let (r, g, b, a) = rgba(last);
                        ramp.add_color_stop_rgba(1.0, r, g, b, a);
                    }
                    cr.set_source(&ramp).ok();
                } else {
                    cr.set_source_rgba(solid[0], solid[1], solid[2], solid[3]);
                }
                cr.fill_preserve().ok();
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.28);
                cr.set_line_width(1.0);
                cr.stroke().ok();
            });
            swatch.queue_draw();
        })
    };

    // Every wallpaper tile built for the Wallpapers page, kept so one place can
    // map the current fill onto the rings. The selected tile used to be
    // highlighted on click and never un-highlighted, so a tile stayed ringed
    // after the fill had moved to None, a custom fill, or an image. The video
    // editor's panel re-syncs the same way on every refresh.
    let wallpaper_tiles: Rc<RefCell<Vec<(PathBuf, Button)>>> = Rc::new(RefCell::new(Vec::new()));
    let sync_wallpaper_tiles: Rc<dyn Fn()> = {
        let tiles = wallpaper_tiles.clone();
        let runtime = session.runtime.clone();
        Rc::new(move || {
            let selected = {
                let runtime = runtime.borrow();
                let appearance = &runtime.motion.appearance;
                if appearance.background_fill_type == MotionBackgroundFillType::Wallpaper {
                    appearance.wallpaper_image_name.clone()
                } else {
                    None
                }
            };
            for (path, tile) in tiles.borrow().iter() {
                let on = selected.as_deref() == Some(path.to_string_lossy().as_ref());
                if on {
                    tile.add_css_class("active-background-option");
                } else {
                    tile.remove_css_class("active-background-option");
                }
            }
        })
    };

    let background_section = motion_appearance_section("Background");
    let none_button = Button::with_label(&t("None"));
    none_button.set_has_frame(false);
    none_button.set_hexpand(true);
    none_button.set_halign(Align::Fill);
    // The video editor's None row, class and all. Both Background panels clear
    // the fill from the same chrome, so the two editors stay in step and the
    // rule only has to be kept in one place.
    none_button.add_css_class("recording-editor-bg-none-row");
    if initial_fill_type == MotionBackgroundFillType::None {
        none_button.add_css_class("active-background-option");
    }
    none_button.connect_clicked({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let notify = notify_interact.clone();
        let sync = sync_wallpaper_tiles.clone();
        move |_| {
            notify();
            {
                let mut runtime = runtime.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::None;
                runtime.backdrop_cache = None;
                runtime.preview_frame = None;
            }
            none_button.add_css_class("active-background-option");
            // Clearing the fill takes the ring off whichever tile held it.
            sync();
            preview.queue_draw();
        }
    });
    // The Custom fill's Color and Gradient pages are the video editor's Custom
    // Wallpaper popover, backed by Motion's own appearance fields through these
    // four closures.
    let fill_ops = FillOps {
        get_color: {
            let runtime = session.runtime.clone();
            Rc::new(move || {
                motion_rgb888(motion_rgba(
                    runtime.borrow().motion.appearance.background_color,
                ))
            })
        },
        set_color: {
            let runtime = session.runtime.clone();
            Rc::new(move |color: (u8, u8, u8)| {
                let mut runtime = runtime.borrow_mut();
                runtime.begin_motion_edit();
                let alpha = runtime.motion.appearance.background_color[3];
                runtime.motion.appearance.background_color = [
                    f64::from(color.0) / 255.0,
                    f64::from(color.1) / 255.0,
                    f64::from(color.2) / 255.0,
                    alpha,
                ];
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
                runtime.motion.appearance.wallpaper_image_name = None;
                runtime.motion.appearance.custom_background_image = None;
                runtime.backdrop_cache = None;
                runtime.preview_frame = None;
            })
        },
        get_gradient: {
            let runtime = session.runtime.clone();
            Rc::new(move || runtime.borrow().motion.appearance.gradient.clone())
        },
        set_gradient: {
            let runtime = session.runtime.clone();
            Rc::new(move |gradient: VideoGradient| {
                let mut runtime = runtime.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.appearance.gradient = gradient.normalized();
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
                runtime.motion.appearance.wallpaper_image_name = None;
                runtime.motion.appearance.custom_background_image = None;
                runtime.backdrop_cache = None;
                runtime.preview_frame = None;
            })
        },
    };
    // Every edit repaints the preview and refreshes the summary row.
    let fill_changed: Rc<dyn Fn()> = {
        let notify = notify_interact.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let redraw_summary = redraw_summary.clone();
        let sync = sync_wallpaper_tiles.clone();
        Rc::new(move || {
            notify();
            none_button.remove_css_class("active-background-option");
            // A hand-drawn fill is not a wallpaper, so no tile may stay ringed.
            sync();
            preview.queue_draw();
            redraw_summary();
        })
    };

    let wallpaper_catalog = motion_wallpaper_catalog_section(
        session,
        preview,
        &none_button,
        wallpaper_tiles.clone(),
        &sync_wallpaper_tiles,
        &notify_interact,
    );
    let image_section = motion_image_section(
        window,
        session,
        preview,
        &none_button,
        &sync_wallpaper_tiles,
        &notify_interact,
    );

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
    // The wallpaper catalog is its own page so 70 thumbnails never push the
    // sliders (or the other tools below) off the bottom of the sidebar. The
    // frame keeps a fixed height: the tiles are attached a few frames apart, so
    // a viewport that sized itself to its (initially empty) content would open
    // one row tall and stay there.
    let wallpaper_page = GtkBox::new(Orientation::Vertical, 0);
    wallpaper_page.set_hexpand(true);
    wallpaper_page.append(
        &ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .propagate_natural_width(false)
            .propagate_natural_height(false)
            .height_request(WALLPAPER_PAGE_HEIGHT)
            .child(&wallpaper_catalog)
            .build(),
    );

    // One source at a time, chosen by the tab tray. This mirrors the video
    // editor's Background panel: tabs pick the fill source, and Padding/Radius
    // plus the tool groups below stay shared on every tab.
    let source_stack = Stack::new();
    source_stack.set_hhomogeneous(false);
    source_stack.set_vhomogeneous(false);

    // The Custom tab is a summary row, not the editors: Color/Gradient move
    // into a popover behind the Edit pill, so this panel stays as short as the
    // video editor's.
    let custom_page = GtkBox::new(Orientation::Vertical, 0);
    custom_page.set_hexpand(true);
    let custom_row = GtkBox::new(Orientation::Horizontal, 10);
    custom_row.add_css_class("recording-editor-bg-custom-row");
    custom_row.set_hexpand(true);
    summary_label.set_hexpand(true);
    summary_label.set_xalign(0.0);
    summary_label.set_valign(Align::Center);
    let custom_edit = Button::with_label(&t("Edit"));
    custom_edit.add_css_class("recording-editor-bg-custom-edit");
    custom_edit.set_has_frame(false);
    custom_edit.set_valign(Align::Center);
    custom_edit.set_tooltip_text(Some(&t("Edit custom fill")));
    custom_row.append(&summary_swatch);
    custom_row.append(&summary_label);
    custom_row.append(&custom_edit);
    custom_page.append(&custom_row);

    // The Edit pill opens the video editor's Custom Wallpaper popover, so the
    // two editors share one card, one Color page, and one Gradient editor.
    // Motion's fields sit behind the shared fill closures.
    build_custom_fill_popover(
        &custom_page,
        &custom_edit,
        &t("Custom"),
        fill_ops,
        fill_changed,
    );

    let image_page = GtkBox::new(Orientation::Vertical, 6);
    image_page.append(&image_section);
    source_stack.add_named(&wallpaper_page, Some("wallpapers"));
    source_stack.add_named(&custom_page, Some("custom"));
    source_stack.add_named(&image_page, Some("image"));

    let show_source: Rc<dyn Fn(&str)> = {
        let source_stack = source_stack.clone();
        Rc::new(move |name| source_stack.set_visible_child_name(name))
    };
    for (tab, page) in [
        (&wallpapers_tab, "wallpapers"),
        (&custom_tab, "custom"),
        (&image_tab, "image"),
    ] {
        let show = show_source.clone();
        tab.connect_toggled(move |button| {
            if button.is_active() {
                show(page);
            }
        });
    }
    background_section.append(&source_tabs);
    background_section.append(&none_button);
    background_section.append(&source_stack);
    background_section.append(&padding.widget());
    background_section.append(&blur.widget());
    // The radius rounds the captured image card itself; the background scene
    // stays a full rectangle. It applies on top of the Style preset.
    // Late-bound handle so picking Liquid can also lift a sharp-corner card
    // onto a radius the glass highlights can play on (the Style tiles below
    // read it back on click).
    let radius_slider_slot: Rc<RefCell<Option<FillSlider>>> = Rc::new(RefCell::new(None));
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
    background_section.append(&radius.widget());
    background_section.append(&noise.widget());
    // Paint the Custom summary chip once so it is correct before any edit.
    redraw_summary();
    root.append(&background_section);

    let shadow_section = motion_appearance_body();
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
    root.append(&motion_disclosure_row("Shadow", &shadow_section));

    let border_section = motion_appearance_body();
    let style_grid = Grid::new();
    style_grid.set_column_homogeneous(true);
    style_grid.set_column_spacing(6);
    style_grid.set_row_spacing(8);
    style_grid.set_hexpand(false);
    style_grid.set_halign(Align::Fill);
    let style_buttons: Rc<RefCell<Vec<(FrameStyle, Button)>>> = Rc::new(RefCell::new(Vec::new()));
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
    root.append(&motion_disclosure_row("Style", &border_section));

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
    let frame_section = motion_appearance_body();
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
    frame_revealer.set_transition_duration(120);
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
    root.append(&motion_disclosure_row("Frame", &frame_section));

    // Scene Shadows: an independent overlay layer with its own
    // preset id, opacity, and above/below-card placement — deliberately not
    // the card's Border/Shadow drop shadow. The presets are procedural
    // shading.
    let scene_shadow_section = motion_appearance_body();
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
    root.append(&motion_disclosure_row(
        "Scene Shadows",
        &scene_shadow_section,
    ));
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

/// One pill in the fill-source tray. Reuses the video editor's Background
/// tab classes so the two sidebars read identically.
fn motion_source_tab(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(&t(label));
    button.add_css_class("recording-editor-bg-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

/// Body of a disclosure row. The row draws the section title, so the body is
/// bare spacing only — no second heading, no nested card surface.
fn motion_appearance_body() -> GtkBox {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.set_hexpand(false);
    body.set_halign(Align::Fill);
    body
}

/// A tool group below the fill tabs: a clickable header row (title + chevron)
/// over a revealer, collapsed by default. The fill tabs own the top of the
/// panel; everything else opens on demand so the sidebar never becomes one
/// long scroll. The heading stays in the row the revealer wraps.
fn motion_disclosure_row(title: &str, body: &GtkBox) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 8);
    row.add_css_class("editor-motion-settings-section");
    let header = Button::new();
    header.set_has_frame(false);
    header.add_css_class("editor-disclosure-header");
    header.set_hexpand(true);
    let header_inner = GtkBox::new(Orientation::Horizontal, 8);
    header_inner.set_hexpand(true);
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("editor-background-section-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    let chevron = Label::new(Some("\u{203A}"));
    chevron.add_css_class("editor-disclosure-chevron");
    header_inner.append(&heading);
    header_inner.append(&chevron);
    header.set_child(Some(&header_inner));
    row.append(&header);

    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    // Short enough that a dropped frame is barely perceptible and that fewer
    // frames redraw the body while it slides.
    revealer.set_transition_duration(120);
    revealer.set_hexpand(false);
    revealer.set_halign(Align::Fill);
    revealer.set_child(Some(body));
    row.append(&revealer);
    header.connect_clicked(move |_| {
        let revealed = revealer.reveals_child();
        revealer.set_reveal_child(!revealed);
        chevron.set_label(if revealed { "\u{203A}" } else { "\u{2304}" });
    });
    row
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

/// Fixed height for the bounded wallpaper grid. Tall enough for a few rows of
/// 56px thumbs (plus spacing), short enough that the tools below stay reachable
/// without scrolling the whole sidebar.
const WALLPAPER_PAGE_HEIGHT: i32 = 260;

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

/// Bundled wallpaper catalog for the Wallpapers tab. Tiles are attached a few
/// frames apart so opening the tab never blocks on 70 thumbnail decodes; the
/// page scrolls inside its own frame (see the Background section) so the grid
/// length never pushes Padding/Radius or the tools below off-screen.
///
/// The grid is the video editor's, cell for cell: four homogeneous columns with
/// the same gaps, so the two panels' wallpapers read and wrap identically.
fn motion_wallpaper_catalog_section(
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    tiles: Rc<RefCell<Vec<(PathBuf, Button)>>>,
    sync_tiles: &Rc<dyn Fn()>,
    on_interact: &Rc<dyn Fn()>,
) -> Grid {
    let grid = Grid::new();
    grid.add_css_class("editor-motion-wallpaper-grid");
    grid.add_css_class("recording-editor-bg-wallpaper-grid");
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(false);
    grid.set_halign(Align::Fill);

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
    let next_row = Rc::new(Cell::new(0usize));
    glib::timeout_add_local(Duration::from_millis(12), {
        let grid = grid.clone();
        let paths = paths.clone();
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let tiles = tiles.clone();
        let sync_tiles = sync_tiles.clone();
        let next_row = next_row.clone();
        let on_interact = on_interact.clone();
        move || {
            let start = next_row.get();
            if start >= paths.len() {
                return glib::ControlFlow::Break;
            }
            let end = (start + 4).min(paths.len());
            for (offset, (path, preview_path)) in paths[start..end].iter().enumerate() {
                let index = start + offset;
                let tile = motion_wallpaper_thumbnail(
                    path,
                    preview_path,
                    &session,
                    &preview,
                    &none_button,
                    tiles.clone(),
                    &sync_tiles,
                    &on_interact,
                );
                grid.attach(&tile, (index % 4) as i32, (index / 4) as i32, 1, 1);
            }
            next_row.set(end);
            // A tile that arrives after the fill was set still has to show it.
            sync_tiles();
            glib::ControlFlow::Continue
        }
    });
    grid
}

fn motion_wallpaper_thumbnail(
    path: &std::path::Path,
    preview_path: &std::path::Path,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    tiles: Rc<RefCell<Vec<(PathBuf, Button)>>>,
    sync_tiles: &Rc<dyn Fn()>,
    on_interact: &Rc<dyn Fn()>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-wallpaper-thumbnail");
    // Pinned to 56px and centred in its grid cell, exactly as the video
    // editor's tiles are: without this a homogeneous column stretches the tile
    // to fill the sidebar and the thumbnails stop matching.
    button.set_hexpand(false);
    button.set_halign(Align::Center);
    button.set_valign(Align::Start);
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

    // The ring is not set here: the caller syncs every tile against the fill
    // once the row is attached, so a tile built late cannot claim a selection
    // the model no longer holds.
    tiles.borrow_mut().push((path.clone(), button.clone()));
    button.connect_clicked({
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let sync_tiles = sync_tiles.clone();
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
            {
                let mut runtime = session.runtime.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.appearance.wallpaper_image_name =
                    Some(path.to_string_lossy().into_owned());
                runtime.motion.appearance.background_fill_type =
                    MotionBackgroundFillType::Wallpaper;
                // Replace, never stack: the other image slot must not survive or
                // a later round-trip can resurrect it behind the new fill.
                runtime.motion.appearance.custom_background_image = None;
                // The thumbnail is cached. Show it now, then replace it with the
                // full wallpaper from a worker thread. ponytail: cache hit, no decode.
                runtime.set_background_surface(
                    Some(path.to_string_lossy().into_owned()),
                    cached_wallpaper_preview_surface(&preview_path.to_string_lossy()),
                    true,
                );
            }
            // One source of truth for the rings: this tile on, every other off.
            sync_tiles();
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
            load_motion_wallpaper_asynchronously(path.clone(), session.clone(), preview.clone());
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

/// The Image tab: one full-width row that opens the image chooser and shows
/// the picked file as its leading chip, matching the video editor's Image
/// page. The section title and "Choose…" button the page used to carry are
/// gone — the tab's own label already names the page, and the row reads as
/// the same single control the video editor shows.
fn motion_image_section(
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    sync_tiles: &Rc<dyn Fn()>,
    on_interact: &Rc<dyn Fn()>,
) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);

    let row = Button::new();
    row.add_css_class("recording-editor-bg-image-row");
    row.set_has_frame(false);
    row.set_hexpand(true);
    let inner = GtkBox::new(Orientation::Horizontal, 10);
    let thumb = DrawingArea::new();
    thumb.add_css_class("recording-editor-bg-custom-swatch");
    thumb.set_content_width(28);
    thumb.set_content_height(28);
    thumb.set_valign(Align::Center);
    thumb.set_can_target(false);
    // The chip reads the runtime at draw time instead of capturing a surface:
    // coming back to this tab after switching fills repaints it from whatever
    // is current, rather than leaving the last pick frozen in the row.
    thumb.set_draw_func({
        let runtime = session.runtime.clone();
        move |_, cr, width, height| {
            let surface = {
                let runtime = runtime.borrow();
                if runtime.motion.appearance.background_fill_type == MotionBackgroundFillType::Image
                {
                    runtime.background_surface.clone()
                } else {
                    None
                }
            };
            if let Some(surface) = surface.as_ref() {
                paint_image_row_thumb(cr, surface, width, height);
            }
        }
    });
    let label = Label::new(Some(&t("Select image...")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    icon.set_pixel_size(13);
    icon.set_valign(Align::Center);
    inner.append(&thumb);
    inner.append(&label);
    inner.append(&icon);
    row.set_child(Some(&inner));
    page.append(&row);

    // The picker writes the image fill and decodes its preview exactly as the
    // old Choose… button did; only the row's chrome changed.
    row.connect_clicked({
        let window = window.downgrade();
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let thumb = thumb.clone();
        let notify = on_interact.clone();
        let sync_tiles = sync_tiles.clone();
        move |_| {
            notify();
            let chooser = FileChooserNative::new(
                Some(&t("Choose Background Image")),
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
            let none_button = none_button.clone();
            let thumb = thumb.clone();
            let notify_response = notify.clone();
            let sync_tiles = sync_tiles.clone();
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
                runtime.motion.appearance.custom_background_image = Some(path.clone());
                runtime.motion.appearance.wallpaper_image_name = None;
                runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Image;
                // Decode at the preview edge and hand the pixels to the shared
                // runtime, so the canvas, this chip, and Motion all paint the
                // same file.
                let surface = super::super::motion_render::load_motion_background_preview_surface(
                    &path,
                    PREVIEW_WALLPAPER_MAX_EDGE,
                );
                runtime.set_background_surface(Some(path), surface, false);
                drop(runtime);
                none_button.remove_css_class("active-background-option");
                // An image is not a wallpaper, so no tile may stay ringed.
                sync_tiles();
                thumb.queue_draw();
                preview.queue_draw();
            });
            chooser.show();
        }
    });
    page
}

/// Paint the picked file into the Image row's chip: cover-cropped like the
/// wallpaper cards, so a portrait shot still fills the square.
fn paint_image_row_thumb(
    cr: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
) {
    let source_w = surface.width().max(1) as f64;
    let source_h = surface.height().max(1) as f64;
    let scale = (f64::from(width) / source_w).max(f64::from(height) / source_h);
    let _ = cr.save();
    motion_thumbnail_rounded_rectangle(cr, 0.0, 0.0, f64::from(width), f64::from(height), 7.0);
    cr.clip();
    cr.translate(
        (f64::from(width) - source_w * scale) * 0.5,
        (f64::from(height) - source_h * scale) * 0.5,
    );
    cr.scale(scale, scale);
    let _ = cr.set_source_surface(surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

#[cfg(test)]
mod tests {
    /// The Image tab is the video editor's Image page: one full-width row
    /// carrying a leading chip, the label, and the folder glyph. The old
    /// section title over a separate Choose… button read as two controls for
    /// one action.
    #[test]
    fn image_tab_is_a_single_select_row() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("recording-editor-bg-image-row")
                && production_source.contains("t(\"Select image...\")")
                && production_source.contains("FOLDER_OPEN_REGULAR")
                && production_source.contains("paint_image_row_thumb(")
                && !production_source.contains("t(\"Choose…\")"),
            "the Image tab must be the video editor's single Select image... row",
        );
    }

    /// The None row is the video editor's, shared by class rather than copied:
    /// the recording stylesheet is installed in this window too, so both
    /// Background panels clear the fill from one rule and one look.
    #[test]
    fn none_row_reuses_the_video_editor_chrome() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source
                .contains("none_button.add_css_class(\"recording-editor-bg-none-row\")"),
            "the None row must reuse the video editor's row chrome",
        );
        assert!(
            !production_source
                .contains("none_button.add_css_class(\"editor-background-section-action-button\")"),
            "None must not fall back to the small uppercase section-action button",
        );
        assert!(
            production_source.contains("background_section.append(&none_button);"),
            "the None row must sit in the Background section, under the tabs",
        );
    }

    /// Every Background slider must be parented into the section that owns it.
    /// The noise control was built and wired but never appended, so it silently
    /// vanished from the Appearance panel while still mutating state.
    #[test]
    fn background_sliders_are_parented_into_the_background_section() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        for control in ["padding", "blur", "radius", "noise"] {
            let needle = format!("background_section.append(&{control}.widget());");
            assert!(
                production_source.contains(&needle),
                "Background {control} slider must be appended to the Background section",
            );
        }
    }

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

    /// Disclosure clicks also deactivate the current Static canvas tool.
    #[test]
    fn appearance_pointer_interaction_deactivates_static_tool() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("fn render_frame_style_swatch(")
                && production_source.contains("fn paint_frame_style_swatch(")
                && production_source.contains("set_source_surface(&surface, 0.0, 0.0)")
                && production_source.contains("Some((w, h, _)) if *w == width && *h == height"),
            "frame-style swatches must render once per size and blit during the reveal",
        );
        assert!(production_source.contains("capture.connect_pressed(move |_, _, _, _| notify());"));
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
                && production_source.contains("glib::timeout_add_local")
                && production_source.contains("load_motion_wallpaper_asynchronously(")
                && production_source.contains("PREVIEW_WALLPAPER_MAX_EDGE")
                && production_source.contains("already_selected"),
            "wallpaper thumbs must share one cache and never sync-decode full images on open/click",
        );
    }

    /// The wallpaper grid is its own bounded page: 70 thumbnails must scroll
    /// inside a fixed-height frame so Padding/Radius and the other tools stay
    /// reachable without scrolling the whole sidebar. The frame height is fixed
    /// rather than content-sized because the tiles arrive a few frames apart —
    /// a content-sized frame opens one row tall and stays there. The grid markup
    /// itself is the video editor's.
    #[test]
    fn wallpaper_grid_scrolls_inside_its_own_page() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(".height_request(WALLPAPER_PAGE_HEIGHT)")
                && production_source
                    .contains("source_stack.add_named(&wallpaper_page, Some(\"wallpapers\"))"),
            "the wallpaper catalog must live in a fixed-height page inside the source stack",
        );
        assert!(
            production_source.contains("grid.set_column_spacing(8)")
                && production_source.contains("grid.set_row_spacing(8)")
                && production_source.contains("grid.set_column_homogeneous(true)")
                && production_source
                    .contains("grid.add_css_class(\"recording-editor-bg-wallpaper-grid\")"),
            "the catalog must use the video editor's grid gaps and chrome",
        );
        assert!(
            production_source.contains("button.set_hexpand(false)")
                && production_source.contains("button.set_halign(Align::Center)"),
            "tiles must stay 56px and centred in their cell, like the video editor's",
        );
    }

    /// Selecting None, applying a custom fill, or picking an image has to drop
    /// the ring from the wallpaper tile, exactly as the video editor's refresh
    /// does. The ring used to be added on click and never taken off, so a tile
    /// stayed highlighted after the fill had moved on.
    #[test]
    fn the_wallpaper_ring_follows_the_fill() {
        let source = include_str!("appearance.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        let sync_start = production_source
            .find("let sync_wallpaper_tiles: Rc<dyn Fn()>")
            .expect("the panel needs one place that maps the fill onto the tile rings");
        let sync_body = &production_source[sync_start..];
        assert!(
            sync_body.contains("tile.add_css_class(\"active-background-option\")")
                && sync_body.contains("tile.remove_css_class(\"active-background-option\")"),
            "the sync must be able to both set and clear a ring",
        );
        // None and the custom fill route through the panel-level closure; the
        // catalog's arriving tiles and the image picker route through theirs.
        assert!(
            production_source.matches("sync();").count() >= 2,
            "None and the custom fill must resync the tile rings",
        );
        assert!(
            production_source.matches("sync_tiles();").count() >= 2,
            "the catalog and the image picker must resync the tile rings too",
        );
    }
}

//! The Motion mode page: the checkerboard preview, the timeline dock, and
//! the tool notch. Entering Motion swaps the workspace and top toolbar the
//! way the Linux editor swaps its stacks; the notch tabs switch the dock's
//! pages.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Image, Orientation, Overlay};

use super::canvas::{draw_canvas_checkerboard, Canvas};
use super::inspector::Inspector;
use super::timeline;
use super::ui_support::px;
use crate::capture::editor::types::ViewTransform;
use crate::capture::editor::window::icon_names::{custom, shipped};
use crate::i18n::t;

pub(super) struct MotionMode {
    /// The page the workspace stack shows while Motion is open.
    pub page: GtkBox,
    /// The top toolbar's Motion page: one labelled Static button.
    pub toolbar: GtkBox,
    pub static_button: Button,
    pub timeline: Rc<timeline::Timeline>,
    inspector: Rc<Inspector>,
    tabs: Rc<std::cell::RefCell<Vec<(Button, &'static str)>>>,
}

impl MotionMode {
    /// Select one notch tab: highlight it and show its dock page. Entry
    /// always lands on Composition, like the Linux editor applies its mode.
    pub(super) fn set_active_tab(&self, page: &'static str) {
        for (button, button_page) in self.tabs.borrow().iter() {
            if *button_page == page {
                button.add_css_class("active-tool");
            } else {
                button.remove_css_class("active-tool");
            }
        }
        self.inspector.show_motion_page(page);
    }
}

pub(super) fn build(
    scale: f64,
    dark: bool,
    inspector: Rc<Inspector>,
    canvas: &Rc<Canvas>,
) -> MotionMode {
    let timeline = Rc::new(timeline::build(scale, dark));
    let preview = DrawingArea::new();
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.set_can_target(false);
    preview.set_draw_func({
        let host = canvas.host.clone();
        let timeline = timeline.clone();
        move |_, context, width, height| {
            let (width, height) = (f64::from(width), f64::from(height));
            draw_canvas_checkerboard(context, width, height, scale, dark);
            let transform = host.read(|st| {
                let (image_width, image_height) = st.working_image.dimensions();
                let layout = st.background.layout(image_width, image_height);
                preview_transform(width, height, scale, image_width, image_height, layout)
            });
            if let Some(transform) = transform {
                let frames = timeline.preview_frames(
                    transform.canvas_width * transform.canvas_scale,
                    transform.canvas_height * transform.canvas_scale,
                );
                paint_frames(&host, context, transform, &frames);
            }
        }
    });
    timeline.set_preview_listener({
        let preview = preview.downgrade();
        Rc::new(move || {
            if let Some(preview) = preview.upgrade() {
                preview.queue_draw();
            }
        })
    });
    canvas.add_change_listener({
        let preview = preview.downgrade();
        let timeline = timeline.clone();
        let inspector = Rc::downgrade(&inspector);
        let generation = canvas.host.session_generation.clone();
        let last_generation = std::cell::Cell::new(generation.get());
        Rc::new(move || {
            if last_generation.replace(generation.get()) != generation.get() {
                timeline.reset_source();
            }
            if let Some(preview) = preview.upgrade() {
                preview.queue_draw();
            }
            if let Some(inspector) = inspector.upgrade() {
                inspector.refresh_motion_thumbnail();
            }
        })
    });
    let preview_shell = Overlay::new();
    preview_shell.set_hexpand(true);
    preview_shell.set_vexpand(true);
    preview_shell.set_child(Some(&preview));

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.set_focusable(true);
    page.append(&preview_shell);
    page.append(&timeline.dock);

    // The mode toolbar keeps the annotate toolbar's pill shape and top gap;
    // Static rides back to the editor the button was clicked from.
    let toolbar = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    toolbar.add_css_class("image-editor-toolbar");
    toolbar.set_halign(Align::Center);
    toolbar.set_valign(Align::Start);
    toolbar.set_margin_top(px(8.0, scale));
    let static_button = Button::with_label(&t("Static"));
    static_button.add_css_class("image-editor-tool-button");
    static_button.add_css_class("image-editor-mode-button");
    static_button.set_focusable(false);
    static_button.set_tooltip_text(Some(&t("Switch to Static")));
    toolbar.append(&static_button);

    // The tool notch hugs the preview's right edge, midway to the timeline,
    // exactly the Linux placement. One Motion tool per notch button.
    let notch = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    notch.add_css_class("editor-motion-tool-notch");
    notch.set_halign(Align::End);
    notch.set_valign(Align::Center);
    let tabs: Rc<std::cell::RefCell<Vec<(Button, &'static str)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let tab_specs: [(&'static str, &str, &str); 4] = [
        ("motion", custom::SPLINE_SYMBOLIC, "Motion"),
        ("motion-text", custom::TYPE_SYMBOLIC, "Text"),
        (
            "motion-appearance",
            custom::IMAGE_ALT_SYMBOLIC,
            "Appearance",
        ),
        ("motion-watermark", shipped::IMAGE_REGULAR, "Watermark"),
    ];
    for (name, icon_name, tooltip) in tab_specs {
        let button = Button::new();
        button.set_has_frame(false);
        button.set_tooltip_text(Some(&t(tooltip)));
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(px(20.0, scale));
        button.set_child(Some(&icon));
        button.add_css_class("image-editor-notch-button");
        button.set_focusable(false);
        notch.append(&button);
        tabs.borrow_mut().push((button.clone(), name));
        let inspector_for_tab = inspector.clone();
        let tabs_for_click = tabs.clone();
        let page_name = name;
        button.connect_clicked(move |_| {
            inspector_for_tab.show_motion_page(page_name);
            for (tab_button, tab_page) in tabs_for_click.borrow().iter() {
                if *tab_page == page_name {
                    tab_button.add_css_class("active-tool");
                } else {
                    tab_button.remove_css_class("active-tool");
                }
            }
        });
    }
    preview_shell.add_overlay(&notch);

    inspector.bind_motion(&timeline);
    timeline.set_state_listener({
        let inspector = Rc::downgrade(&inspector);
        let timeline = Rc::downgrade(&timeline);
        let tabs = Rc::downgrade(&tabs);
        let previous = std::cell::Cell::new((false, false));
        Rc::new(move || {
            let (Some(inspector), Some(timeline), Some(tabs)) =
                (inspector.upgrade(), timeline.upgrade(), tabs.upgrade())
            else {
                return;
            };
            let state = timeline.motion_state();
            inspector.sync_motion(&state);
            let selection = (state.selected.is_some(), state.selected_text.is_some());
            let old = previous.replace(selection);
            let page = if selection.1 && !old.1 {
                Some("motion-text")
            } else if selection.0 && !old.0 {
                Some("motion")
            } else {
                None
            };
            if let Some(page) = page {
                inspector.show_motion_page(page);
                for (button, name) in tabs.borrow().iter() {
                    if *name == page {
                        button.add_css_class("active-tool");
                    } else {
                        button.remove_css_class("active-tool");
                    }
                }
            }
        })
    });

    MotionMode {
        page,
        toolbar,
        static_button,
        timeline,
        inspector,
        tabs,
    }
}

fn paint_frames(
    host: &super::annotate::EditorHost,
    context: &gtk4::cairo::Context,
    transform: ViewTransform,
    frames: &[(crate::recording::editor::model::MotionTransform, (f64, f64))],
) {
    if frames.len() == 1 {
        let (pose, anchor) = frames[0];
        host.paint_motion_preview(context, transform, pose, anchor);
    } else if !frames.is_empty() {
        host.paint_motion_background(context, transform);
        let _ = context.save();
        context.rectangle(
            transform.canvas_offset_x,
            transform.canvas_offset_y,
            transform.canvas_width * transform.canvas_scale,
            transform.canvas_height * transform.canvas_scale,
        );
        context.clip();
        let reference = projected_points(host, transform, frames[0]);
        // Rasterize the expensive card, annotations and shadow once. The
        // short exposure's other poses use an affine approximation of its
        // projected corners instead of rebuilding the entire scene.
        context.push_group();
        host.paint_motion_foreground(context, transform, frames[0].0, frames[0].1);
        let foreground = context.pop_group();
        let Ok(foreground) = foreground else {
            let _ = context.restore();
            return;
        };
        context.push_group();
        for frame in frames {
            let _ = context.save();
            if let (Some(reference), Some(points)) =
                (reference, projected_points(host, transform, *frame))
            {
                if let Some(matrix) =
                    crate::capture::editor::transform_render::triangle_transform(reference, points)
                {
                    context.transform(matrix);
                }
            }
            context.set_operator(gtk4::cairo::Operator::Add);
            context.set_source(&foreground).ok();
            context.paint_with_alpha(1.0 / frames.len() as f64).ok();
            let _ = context.restore();
        }
        if let Ok(composite) = context.pop_group() {
            context.set_operator(gtk4::cairo::Operator::Over);
            context.set_source(&composite).ok();
            context.paint().ok();
        }
        let _ = context.restore();
    }
}

fn projected_points(
    host: &super::annotate::EditorHost,
    transform: ViewTransform,
    frame: (crate::recording::editor::model::MotionTransform, (f64, f64)),
) -> Option<[(f64, f64); 3]> {
    host.read(|state| {
        let (width, height) = state.working_image.dimensions();
        let layout = state.background.layout(width, height);
        let (width, height) = (f64::from(width), f64::from(height));
        let (motion, anchor) = frame;
        let pose = crate::capture::editor::image_transform::StaticImageTransform {
            yaw: motion.rotation_y,
            pitch: motion.rotation_x,
            roll: motion.rotation_z,
            perspective: motion.perspective,
            pos_x: motion.pos_x / motion.scale.max(0.01),
            pos_y: motion.pos_y / motion.scale.max(0.01),
        };
        let anchor_pose = crate::capture::editor::image_transform::StaticImageTransform {
            pos_x: 0.0,
            pos_y: 0.0,
            ..pose
        };
        let anchor_project =
            crate::capture::editor::transform_render::projector(layout, width, height, anchor_pose);
        let (anchor_x, anchor_y) =
            anchor_project((anchor.0 - 0.5) * width, (anchor.1 - 0.5) * height);
        let project =
            crate::capture::editor::transform_render::projector(layout, width, height, pose);
        [
            (-width / 2.0, -height / 2.0),
            (width / 2.0, -height / 2.0),
            (-width / 2.0, height / 2.0),
        ]
        .map(|(x, y)| {
            let (x, y) = project(x, y);
            (
                transform.canvas_offset_x
                    + (anchor_x + (x - anchor_x) * motion.scale) * transform.canvas_scale,
                transform.canvas_offset_y
                    + (anchor_y + (y - anchor_y) * motion.scale) * transform.canvas_scale,
            )
        })
    })
}

fn preview_transform(
    width: f64,
    height: f64,
    ui_scale: f64,
    image_width: u32,
    image_height: u32,
    layout: crate::capture::editor::state::background::Layout,
) -> ViewTransform {
    let inset = (24.0 * ui_scale).min(width.min(height).max(0.0) * 0.25);
    let available_width = (width - inset * 2.0).max(1.0);
    let available_height = (height - inset * 2.0).max(1.0);
    let zoom = (available_width / layout.width.max(1.0))
        .min(available_height / layout.height.max(1.0))
        .min(1.0);
    let x = (width - layout.width * zoom) / 2.0;
    let y = (height - layout.height * zoom) / 2.0;
    ViewTransform {
        scale: zoom,
        offset_x: x + layout.image_x * zoom,
        offset_y: y + layout.image_y * zoom,
        canvas_offset_x: x,
        canvas_offset_y: y,
        canvas_scale: zoom,
        canvas_width: layout.width,
        canvas_height: layout.height,
        image_rect_x: layout.image_x,
        image_rect_y: layout.image_y,
        ..ViewTransform::for_image(f64::from(image_width), f64::from(image_height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::background::{Appearance, Fill};
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    #[test]
    fn preview_fits_and_centers_the_full_appearance_canvas() {
        let mut appearance = Appearance::default();
        appearance.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([0; 4]))));
        appearance.padding = 40.0;
        let layout = appearance.layout(400, 200);
        let fit = preview_transform(300.0, 200.0, 1.0, 400, 200, layout);
        assert!((fit.canvas_width * fit.canvas_scale - 252.0).abs() < 0.001);
        assert!((fit.canvas_offset_x - 24.0).abs() < 0.001);
        assert!((fit.offset_x - fit.canvas_offset_x - layout.image_x * fit.scale).abs() < 0.001);
        let native = preview_transform(1200.0, 800.0, 1.0, 400, 200, layout);
        assert_eq!(native.scale, 1.0);
        assert_eq!(native.canvas_offset_x, (1200.0 - layout.width) / 2.0);
        assert_eq!(native.canvas_offset_y, (800.0 - layout.height) / 2.0);
    }

    #[test]
    fn blur_composites_motion_frames_without_moving_the_background() {
        use crate::capture::editor::state::EditorState;
        use crate::recording::editor::model::MotionTransform;
        let host = super::super::annotate::EditorHost::new(
            Rc::new(|| {}),
            Rc::new(|_| {}),
            Rc::new(|| (0.0, 0.0)),
        );
        let mut state = EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([240, 0, 0, 255])));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 0, 255, 255]),
        )));
        state.background.padding = 40.0;
        let layout = state.background.layout(100, 80);
        *host.editor.borrow_mut() = Some(state);
        let transform = ViewTransform {
            offset_x: layout.image_x,
            offset_y: layout.image_y,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(100.0, 80.0)
        };
        let (width, height) = (layout.width.ceil() as i32, layout.height.ceil() as i32);
        let mut surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, width, height).unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            let pose = MotionTransform::default();
            paint_frames(
                &host,
                &context,
                transform,
                &[
                    (pose, (0.5, 0.5)),
                    (MotionTransform { pos_x: 0.5, ..pose }, (0.5, 0.5)),
                ],
            );
        }
        assert_eq!(host.document_paints.get(), 2);
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let image = crate::capture::editor::render::cairo_argb_to_rgba_image(
            width as u32,
            height as u32,
            stride,
            data.as_ref(),
        );
        let blended = image.get_pixel(
            (layout.image_x + 85.0) as u32,
            (layout.image_y + 40.0) as u32,
        );
        assert!(blended[0] > 100 && blended[0] < 140);
        assert!(blended[2] > 110 && blended[2] < 145);
        assert_eq!(
            *image.get_pixel(width as u32 - 1, 0),
            Rgba([0, 0, 255, 255])
        );
    }

    #[test]
    fn blur_reuses_one_foreground_for_all_samples() {
        use crate::capture::editor::state::EditorState;
        use crate::recording::editor::model::MotionTransform;

        let host = super::super::annotate::EditorHost::new(
            Rc::new(|| {}),
            Rc::new(|_| {}),
            Rc::new(|| (0.0, 0.0)),
        );
        *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::from_pixel(
            160,
            120,
            Rgba([255, 0, 0, 255]),
        )));
        let surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 160, 120).unwrap();
        let context = gtk4::cairo::Context::new(&surface).unwrap();
        let frames: Vec<_> = (0..5)
            .map(|i| {
                (
                    MotionTransform {
                        scale: 1.0 + f64::from(i) * 0.005,
                        rotation_y: 5.0 + f64::from(i) * 0.1,
                        pos_x: f64::from(i) * 0.005,
                        ..Default::default()
                    },
                    (0.35, 0.4),
                )
            })
            .collect();
        paint_frames(
            &host,
            &context,
            ViewTransform::for_image(160.0, 120.0),
            &frames,
        );
        assert_eq!(host.document_paints.get(), 2);
        let reference =
            projected_points(&host, ViewTransform::for_image(160.0, 120.0), frames[0]).unwrap();
        for frame in &frames {
            let target =
                projected_points(&host, ViewTransform::for_image(160.0, 120.0), *frame).unwrap();
            let matrix =
                crate::capture::editor::transform_render::triangle_transform(reference, target)
                    .unwrap();
            for (source, expected) in reference.into_iter().zip(target) {
                let actual = matrix.transform_point(source.0, source.1);
                assert!((actual.0 - expected.0).abs() < 1e-7);
                assert!((actual.1 - expected.1).abs() < 1e-7);
            }
        }
    }

    #[test]
    #[ignore = "manual performance comparison"]
    fn blur_preview_performance_comparison() {
        use crate::capture::editor::state::EditorState;
        use crate::recording::editor::model::{MotionSceneShadowPreset, MotionTransform};
        let host = super::super::annotate::EditorHost::new(
            Rc::new(|| {}),
            Rc::new(|_| {}),
            Rc::new(|| (0.0, 0.0)),
        );
        let mut state =
            EditorState::new(RgbaImage::from_pixel(1600, 900, Rgba([210, 80, 20, 255])));
        state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([20, 30, 70, 255]),
        )));
        state.background.padding = 40.0;
        state.scene_shadow.preset = MotionSceneShadowPreset::Adaptive;
        let layout = state.background.layout(1600, 900);
        *host.editor.borrow_mut() = Some(state);
        let transform = preview_transform(960.0, 600.0, 1.0, 1600, 900, layout);
        let surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 960, 600).unwrap();
        let context = gtk4::cairo::Context::new(&surface).unwrap();
        let frames: Vec<_> = (0..5)
            .map(|i| {
                (
                    MotionTransform {
                        scale: 1.1,
                        rotation_y: 15.0 + f64::from(i) * 0.1,
                        pos_x: f64::from(i) * 0.002,
                        ..Default::default()
                    },
                    (0.5, 0.5),
                )
            })
            .collect();
        host.paint_motion_preview(&context, transform, frames[0].0, frames[0].1);
        let start = std::time::Instant::now();
        for (pose, anchor) in &frames {
            context.push_group();
            host.paint_motion_preview(&context, transform, *pose, *anchor);
            let _ = context.pop_group();
        }
        let original = start.elapsed();
        let start = std::time::Instant::now();
        paint_frames(&host, &context, transform, &frames);
        let optimized = start.elapsed();
        eprintln!("five full scene renders: {original:?}; optimized blur: {optimized:?}");
    }
}

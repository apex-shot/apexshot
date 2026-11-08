//! The Motion mode page: the checkerboard preview, the timeline dock, and
//! the tool notch. Entering Motion swaps the workspace and top toolbar the
//! way the Linux editor swaps its stacks; the notch tabs switch the dock's
//! pages.

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gtk4::cairo::{Context as CairoContext, Format, ImageSurface, Operator};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, GestureDrag, Image, Orientation, Overlay};

use super::canvas::{draw_canvas_checkerboard, Canvas};
use super::inspector::Inspector;
use super::motion_text;
use super::timeline;
use super::ui_support::px;
use super::watermark;
use crate::capture::editor::state::background::Layout;
use crate::capture::editor::types::ViewTransform;
use crate::capture::editor::window::icon_names::{custom, shipped};
use crate::i18n::t;
use crate::recording::editor::model::{MotionState, MOTION_EXPORT_FPS};

type VideoDone = Rc<RefCell<Option<Box<dyn FnOnce(Result<PathBuf, String>)>>>>;
type VideoSetup = (
    Rc<RefCell<MotionVideoJob>>,
    Rc<RefCell<Option<ImageSurface>>>,
);

pub(super) struct MotionMode {
    /// The page the workspace stack shows while Motion is open.
    pub page: GtkBox,
    /// The top toolbar's Motion page: one labelled Static button.
    pub toolbar: GtkBox,
    pub static_button: Button,
    pub timeline: Rc<timeline::Timeline>,
    inspector: Rc<Inspector>,
    tabs: Rc<std::cell::RefCell<Vec<(Button, &'static str)>>>,
    host: Rc<super::annotate::EditorHost>,
}

impl MotionMode {
    /// Render the frame the preview is showing, at the composition's own
    /// resolution, for Copy, Upload and Export. The mark and titles are
    /// painted the same way the preview paints them, so the still matches
    /// what the user sees.
    pub(super) fn current_frame_image(&self) -> Option<image::RgbaImage> {
        if self.host.background_loading() {
            return None;
        }
        let watermark_surface = self.inspector.watermark.surface();
        frame_image(&self.host, &self.timeline, &watermark_surface)
    }

    /// Render the whole composition to an MP4 on the main loop, one frame per
    /// tick, so the window stays responsive. The still card, titles and mark
    /// are painted through the same preview path used on screen; the backdrop
    /// is painted once and copied under each pose.
    pub(super) fn export_video(
        &self,
        destination: PathBuf,
        on_done: impl FnOnce(Result<PathBuf, String>) + 'static,
    ) -> Result<(), String> {
        let (job, watermark_surface) = self.prepare_video_job(destination)?;
        let done: VideoDone = Rc::new(RefCell::new(Some(Box::new(on_done))));
        let host = self.host.clone();
        gtk4::glib::timeout_add_local(Duration::from_millis(1), move || {
            let mut job = job.borrow_mut();
            let result = if job.frame >= job.frame_count {
                job.finish()
            } else {
                match job.render_frame(&host, &watermark_surface) {
                    Ok(()) if job.frame >= job.frame_count => job.finish(),
                    Ok(()) => return gtk4::glib::ControlFlow::Continue,
                    Err(error) => job.abort(error),
                }
            };
            drop(job);
            if let Some(callback) = done.borrow_mut().take() {
                callback(result);
            }
            gtk4::glib::ControlFlow::Break
        });
        Ok(())
    }

    #[cfg(test)]
    fn export_video_blocking(&self, destination: PathBuf) -> Result<PathBuf, String> {
        let (job, watermark_surface) = self.prepare_video_job(destination)?;
        let mut job = job.borrow_mut();
        while job.frame < job.frame_count {
            if let Err(error) = job.render_frame(&self.host, &watermark_surface) {
                return job.abort(error);
            }
        }
        job.finish()
    }

    fn prepare_video_job(&self, destination: PathBuf) -> Result<VideoSetup, String> {
        crate::studio_entitlement::ensure_export_allowed(
            crate::studio_entitlement::capabilities_for_motion_export(),
        )
        .map_err(|error| error.to_string())?;
        crate::recording::editor::ffmpeg::ensure_tools_available()?;

        let (layout, image_width, image_height) = self
            .host
            .read(|st| {
                let (image_width, image_height) = st.working_image.dimensions();
                (
                    st.background.layout(image_width, image_height),
                    f64::from(image_width),
                    f64::from(image_height),
                )
            })
            .ok_or_else(|| t("Motion has no still to export."))?;
        let width = even_output_size(layout.width);
        let height = even_output_size(layout.height);
        let extra_x = (f64::from(width) - layout.width) / 2.0;
        let extra_y = (f64::from(height) - layout.height) / 2.0;
        let transform = ViewTransform {
            offset_x: extra_x + layout.image_x,
            offset_y: extra_y + layout.image_y,
            canvas_offset_x: extra_x,
            canvas_offset_y: extra_y,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(image_width, image_height)
        };

        let backdrop = ImageSurface::create(Format::ARgb32, width, height)
            .map_err(|error| error.to_string())?;
        {
            let context = CairoContext::new(&backdrop).map_err(|error| error.to_string())?;
            self.host
                .paint_motion_background(&context, transform, false);
        }
        let surface = ImageSurface::create(Format::ARgb32, width, height)
            .map_err(|error| error.to_string())?;

        let motion = self.timeline.motion_state();
        let frame_count = motion_frame_count(motion.duration);
        let size = format!("{width}x{height}");
        let fps = MOTION_EXPORT_FPS.to_string();
        let mut command = crate::recording::editor::ffmpeg::export_command();
        let mut child = command
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                size.as_str(),
                "-framerate",
                fps.as_str(),
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-crf",
                "18",
                "-movflags",
                "+faststart",
            ])
            .arg(&destination)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start ffmpeg: {error}"))?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(t("Could not open the Motion encoder input."));
        };

        let job = Rc::new(RefCell::new(MotionVideoJob {
            child,
            stdin: Some(stdin),
            surface,
            backdrop,
            transform,
            layout,
            image_width,
            image_height,
            width,
            height,
            motion,
            frame: 0,
            frame_count,
            destination,
        }));
        Ok((job, self.inspector.watermark.surface()))
    }

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
    let watermark_surface = inspector.watermark.surface();
    let preview = DrawingArea::new();
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.set_can_target(true);
    preview.set_draw_func({
        let host = canvas.host.clone();
        let timeline = timeline.clone();
        let watermark_surface = watermark_surface.clone();
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
                paint_frames(&host, context, transform, &frames, timeline.is_live());
                // Titles sit above the card and its annotations, in
                // composition coordinates so they follow the camera.
                if let Some((pose, anchor)) = frames.first() {
                    let state = timeline.motion_state();
                    let time = timeline.preview_time();
                    let Some((layout, image_width, image_height)) = host.read(|st| {
                        let (image_width, image_height) = st.working_image.dimensions();
                        (
                            st.background.layout(image_width, image_height),
                            f64::from(image_width),
                            f64::from(image_height),
                        )
                    }) else {
                        return;
                    };
                    let card =
                        motion_text::card_layout(layout, image_width, image_height, *pose, *anchor);
                    let _ = context.save();
                    context.translate(transform.canvas_offset_x, transform.canvas_offset_y);
                    context.scale(transform.canvas_scale, transform.canvas_scale);
                    context.rectangle(0.0, 0.0, layout.width, layout.height);
                    context.clip();
                    motion_text::paint_motion_text(context, card, &state, time);
                    // The mark sits above the card and its titles, in the
                    // same card space, so it tracks the camera too.
                    if let Some(surface) = watermark_surface.borrow().clone() {
                        watermark::paint(context, card, &state, &surface);
                    }
                    let _ = context.restore();
                }
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
    timeline.set_thumbnail_source({
        let host = canvas.host.clone();
        Rc::new(move || host.motion_thumbnail())
    });
    // A title can be dragged on the preview itself, the same gesture the
    // panel's pad offers; the renderer's own hit test decides what was grabbed.
    {
        let host = canvas.host.clone();
        let timeline = timeline.clone();
        let preview = preview.clone();
        let grab: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let host = host.clone();
            let timeline = timeline.clone();
            let preview = preview.clone();
            let grab = grab.clone();
            move |_, x, y| {
                grab.set(None);
                let Some(transform) = current_preview_transform(&host, &preview, scale) else {
                    return;
                };
                let Some(context) = TextDragContext::new(&host, &timeline, transform) else {
                    return;
                };
                let state = timeline.motion_state();
                let Some(segment) = state.selected_text_segment() else {
                    return;
                };
                let Some(placement) = context.placement(segment) else {
                    return;
                };
                let (stage_x, stage_y) = context.view_to_stage(x, y);
                if !placement.contains_stage_point(stage_x, stage_y, placement.local_slop(6.0)) {
                    return;
                }
                grab.set(Some(context.stage_to_view(placement.anchor_view_point())));
                timeline.begin_edit();
            }
        });
        drag.connect_drag_update({
            let host = host.clone();
            let timeline = timeline.clone();
            let preview = preview.clone();
            let grab = grab.clone();
            move |_, dx, dy| {
                let Some((anchor_x, anchor_y)) = grab.get() else {
                    return;
                };
                let Some(transform) = current_preview_transform(&host, &preview, scale) else {
                    return;
                };
                let Some(context) = TextDragContext::new(&host, &timeline, transform) else {
                    return;
                };
                let state = timeline.motion_state();
                let Some(segment) = state.selected_text_segment() else {
                    return;
                };
                let Some(placement) = context.placement(segment) else {
                    return;
                };
                let (stage_x, stage_y) = context.view_to_stage(anchor_x + dx, anchor_y + dy);
                let Some((pos_x, pos_y)) = placement.position_at(stage_x, stage_y) else {
                    return;
                };
                timeline.edit_motion(|state| state.set_selected_text_pos(pos_x, pos_y), false);
            }
        });
        drag.connect_drag_end({
            let timeline = timeline.clone();
            let grab = grab.clone();
            move |_, _, _| {
                if grab.take().is_some() {
                    timeline.finish_edit();
                }
            }
        });
        preview.add_controller(drag);
    }
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
        host: canvas.host.clone(),
    }
}

fn paint_frames(
    host: &super::annotate::EditorHost,
    context: &gtk4::cairo::Context,
    transform: ViewTransform,
    frames: &[(crate::recording::editor::model::MotionTransform, (f64, f64))],
    live: bool,
) {
    if frames.len() == 1 {
        let (pose, anchor) = frames[0];
        host.paint_motion_preview(context, transform, pose, anchor, live);
    } else if !frames.is_empty() {
        host.paint_motion_background(context, transform, live);
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
        host.paint_motion_foreground(context, transform, frames[0].0, frames[0].1, live);
        let foreground = context.pop_group();
        let Ok(foreground) = foreground else {
            let _ = context.restore();
            return;
        };
        // Every subframe resamples this group through its own affine matrix.
        // Good filtering convolves the whole surface per sample, which is
        // what makes live motion blur miss the frame budget; the parked frame
        // is composited again with the default filter.
        if live {
            foreground.set_filter(gtk4::cairo::Filter::Fast);
        }
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

/// The preview's current view transform, rebuilt from the allocated size and
/// the document so a gesture and the frame it is steering agree.
fn current_preview_transform(
    host: &Rc<super::annotate::EditorHost>,
    preview: &DrawingArea,
    scale: f64,
) -> Option<ViewTransform> {
    let width = f64::from(preview.width().max(1));
    let height = f64::from(preview.height().max(1));
    host.read(|st| {
        let (image_width, image_height) = st.working_image.dimensions();
        let layout = st.background.layout(image_width, image_height);
        preview_transform(width, height, scale, image_width, image_height, layout)
    })
}

/// What a preview title drag needs: the card projection and the view mapping.
struct TextDragContext {
    layout: crate::capture::editor::state::background::Layout,
    image_width: f64,
    image_height: f64,
    pose: crate::recording::editor::model::MotionTransform,
    anchor: (f64, f64),
    time: f64,
    transform: ViewTransform,
}

impl TextDragContext {
    fn new(
        host: &Rc<super::annotate::EditorHost>,
        timeline: &Rc<timeline::Timeline>,
        transform: ViewTransform,
    ) -> Option<Self> {
        let state = timeline.motion_state();
        let time = timeline.preview_time();
        let (layout, image_width, image_height) = host.read(|st| {
            let (image_width, image_height) = st.working_image.dimensions();
            (
                st.background.layout(image_width, image_height),
                f64::from(image_width),
                f64::from(image_height),
            )
        })?;
        Some(Self {
            layout,
            image_width,
            image_height,
            pose: state.sample(time),
            anchor: state.zoom_anchor_at(time),
            time,
            transform,
        })
    }

    fn placement(
        &self,
        segment: &crate::recording::editor::model::MotionTextSegment,
    ) -> Option<motion_text::MotionTextPlacement> {
        let surface = ImageSurface::create(Format::ARgb32, 1, 1).ok()?;
        let context = CairoContext::new(&surface).ok()?;
        let card = motion_text::card_layout(
            self.layout,
            self.image_width,
            self.image_height,
            self.pose,
            self.anchor,
        );
        motion_text::motion_text_placement(&context, card, segment, self.time)
    }

    fn view_to_stage(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.transform.canvas_offset_x) / self.transform.canvas_scale,
            (y - self.transform.canvas_offset_y) / self.transform.canvas_scale,
        )
    }

    fn stage_to_view(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.transform.canvas_offset_x + x * self.transform.canvas_scale,
            self.transform.canvas_offset_y + y * self.transform.canvas_scale,
        )
    }
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

/// Paint the preview's current pose into a composition-sized surface at 1:1:
/// no checkerboard, no inset, no view zoom. Copy, Upload and Export consume
/// this so all three act on what Motion is showing.
fn frame_image(
    host: &super::annotate::EditorHost,
    timeline: &timeline::Timeline,
    watermark_surface: &Rc<RefCell<Option<ImageSurface>>>,
) -> Option<image::RgbaImage> {
    let (layout, image_width, image_height) = host.read(|st| {
        let (image_width, image_height) = st.working_image.dimensions();
        (
            st.background.layout(image_width, image_height),
            f64::from(image_width),
            f64::from(image_height),
        )
    })?;
    let width = layout.width.round().max(1.0) as i32;
    let height = layout.height.round().max(1.0) as i32;
    let mut surface = ImageSurface::create(Format::ARgb32, width, height).ok()?;
    {
        let context = CairoContext::new(&surface).ok()?;
        let transform = ViewTransform {
            offset_x: layout.image_x,
            offset_y: layout.image_y,
            canvas_width: layout.width,
            canvas_height: layout.height,
            image_rect_x: layout.image_x,
            image_rect_y: layout.image_y,
            ..ViewTransform::for_image(image_width, image_height)
        };
        let frames = timeline.preview_frames(layout.width, layout.height);
        paint_frames(host, &context, transform, &frames, false);
        if let Some((pose, anchor)) = frames.first() {
            let state = timeline.motion_state();
            let time = timeline.preview_time();
            let card = motion_text::card_layout(layout, image_width, image_height, *pose, *anchor);
            let _ = context.save();
            context.rectangle(0.0, 0.0, layout.width, layout.height);
            context.clip();
            motion_text::paint_motion_text(&context, card, &state, time);
            if let Some(mark) = watermark_surface.borrow().clone() {
                watermark::paint(&context, card, &state, &mark);
            }
            let _ = context.restore();
        }
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?.to_vec();
    Some(crate::capture::editor::render::cairo_argb_to_rgba_image(
        width as u32,
        height as u32,
        stride,
        &data,
    ))
}

/// H.264 with yuv420p needs even dimensions. A composition can end on an odd
/// pixel after padding or a fractional card scale, so round up.
fn even_output_size(value: f64) -> i32 {
    let size = value.round().max(2.0) as i32;
    size + (size & 1)
}

fn motion_frame_count(duration: f64) -> usize {
    let fps = f64::from(MOTION_EXPORT_FPS);
    ((duration.max(1.0 / fps) * fps).round() as usize).max(1)
}

/// One MP4 render in progress. The loop owns the encoder, the reused frame
/// surface and the backdrop, and advances one frame per main-loop tick.
struct MotionVideoJob {
    child: Child,
    stdin: Option<ChildStdin>,
    surface: ImageSurface,
    backdrop: ImageSurface,
    transform: ViewTransform,
    layout: Layout,
    image_width: f64,
    image_height: f64,
    width: i32,
    height: i32,
    motion: MotionState,
    frame: usize,
    frame_count: usize,
    destination: PathBuf,
}

impl MotionVideoJob {
    fn render_frame(
        &mut self,
        host: &super::annotate::EditorHost,
        watermark_surface: &Rc<RefCell<Option<ImageSurface>>>,
    ) -> Result<(), String> {
        let time = self.frame as f64 / f64::from(MOTION_EXPORT_FPS);
        let pose = self.motion.sample(time);
        let anchor = self.motion.zoom_anchor_at(time);
        {
            let context = CairoContext::new(&self.surface).map_err(|error| error.to_string())?;
            context.set_operator(Operator::Source);
            context
                .set_source_surface(&self.backdrop, 0.0, 0.0)
                .map_err(|error| error.to_string())?;
            context.paint().map_err(|error| error.to_string())?;
            context.set_operator(Operator::Over);
            host.paint_motion_foreground(&context, self.transform, pose, anchor, false);
            let card = motion_text::card_layout(
                self.layout,
                self.image_width,
                self.image_height,
                pose,
                anchor,
            );
            let _ = context.save();
            context.rectangle(
                self.transform.canvas_offset_x,
                self.transform.canvas_offset_y,
                self.layout.width,
                self.layout.height,
            );
            context.clip();
            motion_text::paint_motion_text(&context, card, &self.motion, time);
            if let Some(mark) = watermark_surface.borrow().clone() {
                watermark::paint(&context, card, &self.motion, &mark);
            }
            let _ = context.restore();
        }
        self.surface.flush();
        let stride = self.surface.stride() as usize;
        let data = self.surface.data().map_err(|error| error.to_string())?;
        let rgba = crate::capture::editor::render::cairo_argb_to_rgba_image(
            self.width as u32,
            self.height as u32,
            stride,
            data.as_ref(),
        );
        drop(data);
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| t("The Motion encoder is not running."))?;
        stdin
            .write_all(rgba.as_raw())
            .map_err(|error| format!("Could not stream the Motion frame: {error}"))?;
        self.frame += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<PathBuf, String> {
        self.stdin.take();
        let status = self.child.wait().map_err(|error| error.to_string())?;
        if !status.success() {
            let _ = std::fs::remove_file(&self.destination);
            return Err(t("ffmpeg failed to encode the Motion video."));
        }
        Ok(self.destination.clone())
    }

    fn abort(&mut self, error: String) -> Result<PathBuf, String> {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.destination);
        Err(error)
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
                false,
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
            false,
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
        host.paint_motion_preview(&context, transform, frames[0].0, frames[0].1, false);
        let start = std::time::Instant::now();
        for (pose, anchor) in &frames {
            context.push_group();
            host.paint_motion_preview(&context, transform, *pose, *anchor, false);
            let _ = context.pop_group();
        }
        let original = start.elapsed();
        let start = std::time::Instant::now();
        paint_frames(&host, &context, transform, &frames, false);
        let optimized = start.elapsed();
        eprintln!("five full scene renders: {original:?}; optimized blur: {optimized:?}");
    }

    #[test]
    fn motion_frame_renders_the_composition_at_native_size() {
        use crate::capture::editor::state::EditorState;

        let Some(()) = crate::test_support::with_gtk(|| {
            let host = super::super::annotate::EditorHost::new(
                Rc::new(|| {}),
                Rc::new(|_| {}),
                Rc::new(|| (0.0, 0.0)),
            );
            let mut state =
                EditorState::new(RgbaImage::from_pixel(100, 80, Rgba([240, 0, 0, 255])));
            state.background.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(
                1,
                1,
                Rgba([0, 0, 255, 255]),
            )));
            state.background.padding = 40.0;
            let layout = state.background.layout(100, 80);
            *host.editor.borrow_mut() = Some(state);
            let timeline = timeline::build(1.0, true);
            let mark = Rc::new(RefCell::new(None));

            let frame = frame_image(&host, &timeline, &mark).expect("motion frame");

            assert_eq!(frame.width(), layout.width.ceil() as u32);
            assert_eq!(frame.height(), layout.height.ceil() as u32);
            assert_eq!(*frame.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
            let center = frame.get_pixel(
                (layout.image_x + 50.0) as u32,
                (layout.image_y + 40.0) as u32,
            );
            assert!(center[0] > 200 && center[1] < 40 && center[2] < 40);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn video_frames_use_even_pixels_and_cover_the_duration() {
        assert_eq!(even_output_size(100.0), 100);
        assert_eq!(even_output_size(101.0), 102);
        assert_eq!(even_output_size(0.0), 2);
        assert_eq!(motion_frame_count(1.0), 30);
        assert_eq!(motion_frame_count(0.01), 1);
    }

    #[test]
    fn motion_video_export_writes_an_mp4() {
        if crate::recording::editor::ffmpeg::ensure_tools_available().is_err() {
            eprintln!("skipping: ffmpeg unavailable");
            return;
        }
        let Some(()) = crate::test_support::with_gtk(|| {
            let _entitlement = crate::studio_entitlement::test_studio_entitlement();
            super::super::ui_support::install_css();
            let app = gtk4::Application::new(
                Some("org.apexshot.test.motion-export"),
                gtk4::gio::ApplicationFlags::NON_UNIQUE,
            );
            app.register(None::<&gtk4::gio::Cancellable>).unwrap();
            let window = gtk4::ApplicationWindow::new(&app);
            let canvas = super::super::canvas::build(1.0, true);
            *canvas.host.editor.borrow_mut() =
                Some(crate::capture::editor::state::EditorState::new(
                    RgbaImage::from_pixel(32, 24, Rgba([210, 40, 30, 255])),
                ));
            let inspector = Rc::new(super::super::inspector::build(1.0, &canvas.host, &window));
            let mode = build(1.0, true, inspector, &canvas);
            mode.timeline
                .edit_motion(|state| state.duration = 0.2, false);
            let path = std::env::temp_dir()
                .join(format!("apexshot-motion-export-{}.mp4", std::process::id()));
            let output = mode
                .export_video_blocking(path.clone())
                .expect("the Motion export succeeded");
            assert_eq!(output, path);
            let length = std::fs::metadata(&output).expect("exported file").len();
            assert!(length > 1000, "MP4 is only {length} bytes");
            let probe =
                std::process::Command::new(crate::recording::editor::ffmpeg::ffprobe_executable())
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=codec_name,width,height,nb_frames",
                        "-of",
                        "default=noprint_wrappers=1",
                    ])
                    .arg(&output)
                    .output()
                    .expect("run ffprobe");
            assert!(probe.status.success());
            let fields = String::from_utf8_lossy(&probe.stdout);
            let mut codec = "";
            let mut width = 0;
            let mut height = 0;
            let mut frames = 0;
            for line in fields.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "codec_name" => codec = value,
                        "width" => width = value.parse().unwrap_or(0),
                        "height" => height = value.parse().unwrap_or(0),
                        "nb_frames" => frames = value.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            assert_eq!(codec, "h264");
            assert!(width > 0 && height > 0 && width % 2 == 0 && height % 2 == 0);
            assert!(frames > 1, "MP4 has {frames} frames");
            let _ = std::fs::remove_file(&output);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}

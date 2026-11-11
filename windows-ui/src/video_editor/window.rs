use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::settings::ui_support::{
    caption_button, CaptionGlyph, CAPTION_BUTTON_WIDTH, TITLE_BAR_HEIGHT,
};
use crate::settings::windowing;
use gtk4::gdk;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, AspectFrame, Box as GtkBox, Button, CssProvider,
    DrawingArea, DropTarget, FileChooserAction, FileChooserNative, FileFilter, Image, Label,
    Orientation, Overlay, Picture, Popover, ResponseType, Separator,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Once;

type UpdateDocument = Rc<dyn Fn(Option<&Path>)>;
type CameraTransform = (f64, f64, f64, f64);
/// Where the layout's screen layer draws inside the video rect: the whole
/// source scaled uniformly to cover the pane, and the pane that clips it.
#[derive(Clone, Copy)]
struct ScreenView {
    draw: super::cursor::VideoRect,
    clip: super::cursor::VideoRect,
}

const IDENTITY_CAMERA_TRANSFORM: CameraTransform = (0.0, 0.0, 1.0, 1.0);
const IDENTITY_CAMERA_CSS: &str = ".recording-editor-video { transform: none; }";
const CAMERA_TRANSFORM_EPSILON: f64 = 1e-6;

pub(super) fn build_window(app: &Application, initial: Option<&Path>) {
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    install_css();

    let scale = crate::platform::ui_scale();
    let (default_width, default_height) = default_window_size(scale);
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Video Editor"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(default_width)
        .default_height(default_height)
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");
    window.add_css_class("video-editor-window");
    let minimum = scaled_pair(920.0, 620.0, scale);
    window.set_size_request(minimum.0, minimum.1);

    let prefers_dark = windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("recording-editor-root");
    root.add_css_class("video-editor-root");
    root.add_css_class(if prefers_dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if !prefers_dark {
        window.add_css_class("editor-theme-light");
    }
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let document_title = Rc::new(RefCell::new(document_title(initial)));
    let (title_bar, export_button) = build_title_bar(&window, document_title.clone(), scale);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let background = super::background::Background::default();
    let stage = build_stage(scale, background.clone(), export_button);
    workspace.append(&stage.root);

    let cursor_layer = stage.cursor_layer.clone();
    let cursor_repaint: Rc<dyn Fn()> = Rc::new(move || cursor_layer.queue_draw());

    // The timeline is built first because the Zoom panel reads and writes the
    // zoom track through it.
    let timeline = super::timeline::build(
        &window,
        &stage.canvas,
        scale,
        prefers_dark,
        stage.cursor.clone(),
        stage.webcam.clone(),
        stage.source_size.clone(),
    );

    let tools_column = super::side_panel::build_tools_column(
        scale,
        &window,
        background,
        stage.cursor.clone(),
        cursor_repaint,
        &timeline,
    );
    workspace.append(&tools_column);
    let editor_body = GtkBox::new(Orientation::Vertical, 0);
    editor_body.set_vexpand(true);
    editor_body.append(&workspace);
    editor_body.append(&timeline.root);
    timeline.set_frame_sink({
        let stage = stage.clone();
        Rc::new(move |frame| {
            stage.source_seconds.set(frame.source_seconds);
            stage.webcam.set_composition_time(frame.composition_time);
            stage.webcam.request(frame.source_seconds);
            stage.preview.show_time(frame.source_seconds, frame.playing);
            stage
                .estimate
                .set_composition_seconds(frame.composition_seconds);
            stage.set_placement(frame.placement);
            stage.set_add_zoom(frame.add_zoom);
            stage.set_camera(frame.camera);
            let trail = stage.source_size.get().and_then(|(width, height)| {
                let source = (f64::from(width), f64::from(height));
                let now = stage.camera_view_for(frame.camera)?;
                let before = stage.camera_view_for(frame.camera_before)?;
                super::motion_blur::ScreenTrail::new(
                    now,
                    before,
                    source,
                    (width, height),
                    frame.fps,
                    frame.motion_blur,
                )
                .map(|_| (now, before, frame.fps, frame.motion_blur))
            });
            // With the studied framing, the moving card's whole trail is drawn
            // over the stage so its edges smear across the background.
            let card = trail.and_then(|_| {
                let (now, _, stage_size) = stage.card_geometry(frame.camera)?;
                let (previous, _, _) = stage.card_geometry(frame.camera_before)?;
                Some(super::preview::CardLayer {
                    now,
                    previous,
                    stage: stage_size,
                    radius: stage.clip.radius_ratio(),
                    fps: frame.fps,
                    amount: frame.motion_blur,
                })
            });
            stage.preview.set_trail(trail, card, frame.playing);
            stage.apply_webcam_layout();
            let ripple = stage
                .cursor
                .ripple_at(frame.source_seconds, stage.source_size.get());
            stage.preview.set_ripple(ripple);
            stage.cursor_layer.queue_draw();
        })
    });
    stage.connect_placement({
        let timeline = timeline.clone();
        Rc::new(move |center| timeline.set_selected_zoom_center(center))
    });
    stage.connect_add_zoom(
        {
            let timeline = timeline.clone();
            Rc::new(move |point| timeline.place_zoom_at(point))
        },
        {
            let timeline = timeline.clone();
            Rc::new(move || timeline.add_automatic_zoom())
        },
    );

    let shell = build_editor_shell(&root, &title_bar, &editor_body);

    wire_export(&window, &stage, &timeline, &shell);
    wire_history_keys(&window, &timeline, &shell);

    window.set_child(Some(&root));

    let update_document: UpdateDocument = {
        let document_title = document_title.clone();
        let title_bar = title_bar.clone();
        let stage = stage.clone();
        let timeline = timeline.clone();
        Rc::new(move |path| {
            let title = document_title_for_path(path);
            *document_title.borrow_mut() = title.clone();
            set_title_bar_text(&title_bar, &title);
            // The timeline probes the recording, so the stage takes its
            // lengths from there for the poster and the export estimate.
            timeline.set_document(path);
            stage.show_document(
                path,
                timeline.source_duration(),
                timeline.composition_duration(),
            );
        })
    };
    update_document(initial);

    stage.connect_open({
        let window = window.clone();
        let update_document = update_document.clone();
        Rc::new(move || choose_recording(&window, update_document.clone()))
    });

    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop({
        let update_document = update_document.clone();
        move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            if !is_supported_video(&path) {
                return false;
            }
            update_document(Some(&path));
            true
        }
    });
    stage.root.add_controller(drop_target);

    // Closing the window is the last chance to keep the session: the autosave
    // tick only runs while the editor is up, so the edits are written here
    // rather than a tick later.
    window.connect_close_request({
        let timeline = timeline.clone();
        move |_| {
            timeline.flush_project();
            gtk4::glib::Propagation::Proceed
        }
    });

    window.present();
}

fn build_editor_shell(root: &GtkBox, title_bar: &GtkBox, editor_body: &GtkBox) -> Overlay {
    let editor = GtkBox::new(Orientation::Vertical, 0);
    editor.append(title_bar);
    editor.append(editor_body);

    // Export replaces the title bar as well as the workspace, inside the
    // existing window outline. Clip descendants to that outline's corners.
    root.set_overflow(gtk4::Overflow::Hidden);
    let shell = Overlay::new();
    shell.add_css_class("video-editor-shell");
    shell.set_hexpand(true);
    shell.set_vexpand(true);
    shell.set_overflow(gtk4::Overflow::Hidden);
    shell.set_child(Some(&editor));
    root.append(&shell);
    shell
}

fn build_title_bar(
    window: &ApplicationWindow,
    document_title: Rc<RefCell<String>>,
    scale: f64,
) -> (GtkBox, Button) {
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("settings-window-controls");
    bar.add_css_class("video-editor-titlebar");
    bar.set_size_request(-1, (TITLE_BAR_HEIGHT * scale).round() as i32);

    // Balance the bar like the source editor: reserve the caption controls on
    // the left and the tool column plus controls on the right, so the title is
    // centred over the stage rather than over the whole window. The title then
    // lines up with the preview hint and the stage tools row beneath it.
    let controls_width = ((CAPTION_BUTTON_WIDTH * scale).round() as i32) * 3;
    let tools_width = (super::side_panel::TOOL_COLUMN_WIDTH as f64 * scale).round() as i32;
    let left_spacer = GtkBox::new(Orientation::Horizontal, 0);
    left_spacer.set_halign(Align::Fill);
    left_spacer.set_size_request(controls_width, -1);
    bar.append(&left_spacer);

    let title = Label::new(Some(&document_title.borrow()));
    title.add_css_class("recording-editor-title");
    title.add_css_class("video-editor-document-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_valign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(72);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    title.set_can_target(false);
    bar.append(&title);

    let (minimize, _) = caption_button(
        Rc::new(Cell::new(CaptionGlyph::Minimize)),
        &t("Minimize"),
        scale,
    );
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());

    let maximize_state = Rc::new(Cell::new(CaptionGlyph::Maximize));
    let (maximize, maximize_glyph) = caption_button(maximize_state.clone(), &t("Maximize"), scale);
    let maximize_window = window.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
    });
    window.connect_maximized_notify({
        let state = maximize_state;
        move |window| {
            state.set(if window.is_maximized() {
                CaptionGlyph::Restore
            } else {
                CaptionGlyph::Maximize
            });
            maximize_glyph.queue_draw();
        }
    });

    let (close, _) = caption_button(Rc::new(Cell::new(CaptionGlyph::Close)), &t("Close"), scale);
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());

    let export_button = Button::new();
    export_button.set_has_frame(false);
    export_button.add_css_class("recording-editor-primary-button");
    // The shared primary action's 5px padding and the theme's 34px minimum
    // height do not fit the Windows caption bar, which is 32px at 100%: the
    // button would overhang the window edge. Cap the height for that bar only.
    let export_provider = CssProvider::new();
    export_provider.load_from_data(&format!(
        ".video-editor-title-export {{ min-height: 0; padding: {}px {}px; }}",
        (3.0 * scale).round() as i32,
        (12.0 * scale).round() as i32
    ));
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &export_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
        );
    }
    export_button.add_css_class("video-editor-title-export");
    export_button.set_valign(Align::Center);
    export_button.set_tooltip_text(Some(&t("Choose video export options")));
    let export_label = Label::new(Some(&t("Export")));
    export_label.set_halign(Align::Center);
    export_label.set_valign(Align::Center);
    export_button.set_child(Some(&export_label));
    export_button.set_sensitive(false);
    export_button.set_margin_end((8.0 * scale).round() as i32);

    let actions = GtkBox::new(Orientation::Horizontal, 0);
    actions.set_halign(Align::End);
    actions.set_valign(Align::Fill);
    actions.append(&export_button);
    actions.append(&minimize);
    actions.append(&maximize);
    actions.append(&close);

    // The tool column is as wide as the actions and the caption controls
    // beside it, so the title stays centred over the stage column. The actions
    // hug the window edge inside this block.
    let right_balance = GtkBox::new(Orientation::Horizontal, 0);
    right_balance.set_hexpand(false);
    right_balance.set_size_request(tools_width + controls_width, -1);
    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);
    right_spacer.set_halign(Align::Fill);
    right_balance.append(&right_spacer);
    right_balance.append(&actions);
    bar.append(&right_balance);

    windowing::install_window_drag(&left_spacer, window);
    windowing::install_window_drag(&right_spacer, window);
    (bar, export_button)
}

fn set_title_bar_text(bar: &GtkBox, title: &str) {
    let mut child = bar.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("video-editor-document-title") {
            if let Ok(label) = widget.downcast::<Label>() {
                label.set_text(title);
                label.set_tooltip_text(Some(title));
            }
            return;
        }
        child = widget.next_sibling();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameAspect {
    Original,
    Wide,
    Portrait,
}

impl FrameAspect {
    fn label(self) -> String {
        match self {
            Self::Original => t("Original"),
            Self::Wide => t("16:9"),
            Self::Portrait => t("9:16"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QualityTier {
    Balanced,
    High,
    Ultra,
}

impl Default for QualityTier {
    /// Sharp enough for a screen recording without inflating it, the same
    /// default the reference editor ships.
    fn default() -> Self {
        Self::High
    }
}

impl QualityTier {
    fn label(self) -> String {
        match self {
            Self::Balanced => t("Balanced"),
            Self::High => t("High"),
            Self::Ultra => t("Ultra"),
        }
    }

    /// Settings tier index shared with the recorder (`0 / 1 / 2`), so the
    /// export picker and the recording picker agree on each value.
    #[allow(dead_code)]
    fn tier(self) -> u8 {
        match self {
            Self::Balanced => 0,
            Self::High => 1,
            Self::Ultra => 2,
        }
    }

    /// x264 CRF the exporter encodes with for this tier, shared with the
    /// recorder's own quality scale.
    fn crf(self) -> u32 {
        crate::recording::crf_for_quality(self.tier())
    }

    /// How far an export of this tier sits above or below the source's own
    /// bitrate, used only for the size estimate.
    fn size_factor(self) -> f64 {
        match self {
            Self::Balanced => 1.0,
            Self::High => 1.18,
            Self::Ultra => 1.5,
        }
    }
}

fn frame_ratio(aspect: FrameAspect, source: Option<f32>) -> f32 {
    match aspect {
        FrameAspect::Wide => 16.0 / 9.0,
        FrameAspect::Portrait => 9.0 / 16.0,
        FrameAspect::Original => source.unwrap_or(16.0 / 9.0),
    }
}

/// The frame's base canvas: `Original` keeps the source size, while a fixed
/// frame works at 1080p in that orientation, exactly as the reference editor's
/// dimension presets do. Padding and the corner radius are both measured
/// against this canvas's long edge.
fn canvas_dimensions(aspect: FrameAspect, source: (u32, u32)) -> (u32, u32) {
    match aspect {
        FrameAspect::Original => (source.0.max(1), source.1.max(1)),
        FrameAspect::Wide => (1920, 1080),
        FrameAspect::Portrait => (1080, 1920),
    }
}

/// The stage the layouts are placed on: the source, widened to the studied
/// auto stage when the frame follows the recording and the default camera
/// layout is the split.
fn stage_source(aspect: FrameAspect, source: (u32, u32), widen: bool, gap: f64) -> (u32, u32) {
    if !widen || aspect != FrameAspect::Original || source.0 < source.1 || source.1 == 0 {
        return source;
    }
    let height = source.1;
    let screen = f64::from(source.0) / f64::from(height);
    // An unpadded stage is the studied resolver's zero-gap case.
    let ratio = if gap > 0.0 {
        super::webcam::auto_split_stage_with_gap(screen, gap)
    } else {
        super::webcam::auto_split_stage_aspect(screen, 0.0)
    };
    let width = even_dimension((f64::from(height) * ratio).round() as u32);
    (width.max(source.0), height)
}

/// The stage and the padded split's gap (in stage heights). The gap is the
/// Background padding, the studied way the padded split spaces its panes;
/// it widens an auto stage, which in turn sets the padding, so the pair is
/// settled in two passes.
fn split_stage(
    aspect: FrameAspect,
    screen: (u32, u32),
    widen: bool,
    padded: bool,
    padding: f64,
    has_fill: bool,
) -> ((u32, u32), f64) {
    let mut stage = stage_source(aspect, screen, widen, 0.0);
    let mut gap = 0.0;
    if padded {
        for _ in 0..2 {
            let canvas = canvas_dimensions(aspect, stage);
            let pad = padding_px(canvas, padding, has_fill);
            let video = video_rect_dimensions(aspect, stage, padding, has_fill);
            gap = pad / f64::from(video.1.max(1));
            stage = stage_source(aspect, screen, widen, gap);
        }
    }
    (stage, gap)
}

/// Even dimensions keep the encoder's yuv420p happy.
fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(2);
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        clamped - 1
    }
}

/// Background surround in canvas pixels. The panel's Padding is defined against
/// a 400px long edge, so it scales with the canvas; without a fill the video is
/// the canvas and nothing is inset.
fn padding_px(canvas: (u32, u32), padding: f64, has_fill: bool) -> f64 {
    if !has_fill {
        return 0.0;
    }
    let reference = f64::from(canvas.0.max(canvas.1)) / super::background::PADDING_REFERENCE;
    super::background::effective_padding(padding) * reference
}

/// Export size. A fixed frame is the output exactly (16:9 exports 1920x1080
/// whatever the recording is); `Original` has no ratio to hold, so a fill grows
/// the canvas around the source instead of insetting the video.
fn output_dimensions(
    aspect: FrameAspect,
    source: (u32, u32),
    padding: f64,
    has_fill: bool,
) -> (u32, u32) {
    let canvas = canvas_dimensions(aspect, source);
    if !has_fill || aspect != FrameAspect::Original {
        return canvas;
    }
    let pad = padding_px(canvas, padding, has_fill).round().max(0.0) as u32;
    (
        even_dimension(canvas.0 + pad * 2),
        even_dimension(canvas.1 + pad * 2),
    )
}

/// The video rect inside the output canvas: the source fitted inside the
/// padding, centered. The preview rounds this rect rather than the padded box,
/// so the corners land on the footage instead of on the letterbox the fill
/// makes visible around it.
fn video_rect_dimensions(
    aspect: FrameAspect,
    source: (u32, u32),
    padding: f64,
    has_fill: bool,
) -> (u32, u32) {
    let canvas = canvas_dimensions(aspect, source);
    let (out_w, out_h) = output_dimensions(aspect, source, padding, has_fill);
    let pad = padding_px(canvas, padding, has_fill).round().max(0.0) as u32;
    contain_fit(
        canvas.0,
        canvas.1,
        out_w.saturating_sub(pad * 2).max(2),
        out_h.saturating_sub(pad * 2).max(2),
    )
}

/// Fit `source` inside `box` with its aspect preserved, centered, scaling up
/// when the box is bigger: a fixed frame is a chosen output size, so a 720p
/// recording still fills a 1080p canvas.
fn contain_fit(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let src_w = f64::from(src_w.max(1));
    let src_h = f64::from(src_h.max(1));
    let box_w = f64::from(box_w.max(2));
    let box_h = f64::from(box_h.max(2));
    let scale = (box_w / src_w).min(box_h / src_h);
    let width = even_dimension((src_w * scale).round().max(2.0) as u32);
    let height = even_dimension((src_h * scale).round().max(2.0) as u32);
    // Matching aspects only miss the box by the even-dimension rounding; snap
    // so a recording that fills its box leaves no hairline letterbox.
    if box_w - f64::from(width) <= 2.0 && box_h - f64::from(height) <= 2.0 {
        return (box_w as u32, box_h as u32);
    }
    (width.max(2), height.max(2))
}

/// The video card's corner radius in canvas pixels together with the video
/// rect width it is measured against, so the clip can rescale it to its own
/// width. The radius uses the panel's 400px-long-edge units like padding, and
/// unlike padding it stays live with no fill picked.
fn card_radius(
    aspect: FrameAspect,
    source: (u32, u32),
    padding: f64,
    has_fill: bool,
    radius: f64,
) -> (f64, f64) {
    let canvas = canvas_dimensions(aspect, source);
    let (video_w, _) = video_rect_dimensions(aspect, source, padding, has_fill);
    let video_w = f64::from(video_w.max(1));
    if !radius.is_finite() || radius <= 0.0 {
        return (0.0, video_w);
    }
    let reference = f64::from(canvas.0.max(canvas.1)) / super::background::PADDING_REFERENCE;
    (radius * reference, video_w)
}

/// Estimated export size in bytes: the share of the source that survives the
/// trim, bracketed by the quality tier and the picked frame's pixel count.
fn estimated_size_bytes(
    source_bytes: u64,
    source_seconds: f64,
    composition_seconds: f64,
    source_pixels: (u32, u32),
    target_pixels: (u32, u32),
    quality: QualityTier,
) -> u64 {
    if source_seconds <= f64::EPSILON || source_bytes == 0 {
        return 0;
    }
    let kept = (composition_seconds / source_seconds).max(0.0);
    let base = source_bytes as f64 * kept;
    let source_area = (source_pixels.0 as f64 * source_pixels.1 as f64).max(1.0);
    let target_area = if source_pixels.0 == 0 || source_pixels.1 == 0 {
        source_area
    } else {
        target_pixels.0 as f64 * target_pixels.1 as f64
    };
    let dimension_factor = (target_area / source_area).max(0.0);
    (base * quality.size_factor() * dimension_factor)
        .round()
        .max(0.0) as u64
}

/// The muted readout next to the quality chip: "~12.4 MB", never a bare 0.0
/// while a recording is loaded.
fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{mb:.0} MB")
    }
}

/// Live export estimate for the stage. It follows the document, the quality
/// tier, the frame pick and the timeline's current length, so the number
/// beside the chip always describes what an export would write now.
#[derive(Clone)]
struct Estimate {
    label: Label,
    source_bytes: Rc<Cell<u64>>,
    source_seconds: Rc<Cell<f64>>,
    source_pixels: Rc<Cell<(u32, u32)>>,
    composition_seconds: Rc<Cell<f64>>,
    aspect: Rc<Cell<FrameAspect>>,
    quality: Rc<Cell<QualityTier>>,
}

impl Estimate {
    fn new(label: Label, aspect: Rc<Cell<FrameAspect>>, quality: Rc<Cell<QualityTier>>) -> Self {
        let estimate = Self {
            label,
            source_bytes: Rc::new(Cell::new(0)),
            source_seconds: Rc::new(Cell::new(0.0)),
            source_pixels: Rc::new(Cell::new((0, 0))),
            composition_seconds: Rc::new(Cell::new(0.0)),
            aspect,
            quality,
        };
        estimate.refresh();
        estimate
    }

    fn set_source(&self, path: Option<&Path>, seconds: f64, composition: f64) {
        let bytes = path
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        self.source_bytes.set(bytes);
        self.source_seconds.set(seconds.max(0.0));
        self.composition_seconds.set(composition.max(0.0));
        self.refresh();
    }

    fn set_pixels(&self, width: u32, height: u32) {
        self.source_pixels.set((width, height));
        self.refresh();
    }

    fn set_composition_seconds(&self, seconds: f64) {
        if (self.composition_seconds.get() - seconds).abs() < 1e-6 {
            return;
        }
        self.composition_seconds.set(seconds.max(0.0));
        self.refresh();
    }

    fn refresh(&self) {
        let bytes = estimated_size_bytes(
            self.source_bytes.get(),
            self.source_seconds.get(),
            self.composition_seconds.get(),
            self.source_pixels.get(),
            canvas_dimensions(self.aspect.get(), self.source_pixels.get()),
            self.quality.get(),
        );
        self.label.set_text(&format!("~{}", format_size(bytes)));
    }
}

type Placement = Rc<Cell<Option<(f64, (f64, f64))>>>;

/// The preview stage, mirrored from the recording editor: a padded 16:9 play
/// surface with the centred stage tools below it. The transport lives in the
/// timeline, exactly as it does in the Linux editor.
#[derive(Clone)]
struct Stage {
    root: GtkBox,
    canvas: AspectFrame,
    overlay: Overlay,
    picture: Picture,
    hint: Label,
    aspect: Rc<Cell<FrameAspect>>,
    preview: Rc<super::preview::Preview>,
    estimate: Estimate,
    quality: Rc<Cell<QualityTier>>,
    export_button: Button,
    background: super::background::BackgroundState,
    background_layer: DrawingArea,
    cursor_layer: DrawingArea,
    cursor: super::cursor::CursorModel,
    webcam: super::webcam::WebcamModel,
    source_seconds: Rc<Cell<f64>>,
    background_pixbuf: Rc<RefCell<Option<Pixbuf>>>,
    background_fill: Rc<RefCell<Option<super::background::FillPainter>>>,
    background_selection: Rc<RefCell<Option<String>>>,
    source_size: Rc<Cell<Option<(u32, u32)>>>,
    widened: Cell<bool>,
    padded: Cell<bool>,
    clip: super::rounded_clip::RoundedClip,
    clip_margins: Rc<RefCell<(i32, i32, i32, i32)>>,
    camera_view: Rc<Cell<Option<super::zoom::ViewRect>>>,
    /// The stage inside the padding, as fractions of the stage: the area a
    /// zoomed card must keep covering.
    zoom_area: Rc<Cell<Option<super::zoom::StageRect>>>,
    /// The background's zoom and the stage point it zooms about, as fractions.
    background_zoom: Rc<Cell<(f64, (f64, f64))>>,
    /// The Manual zoom outline the preview shows, as `(scale, focus point)`.
    placement: Placement,
    /// The armed Add zoom tool, while the transport is stopped.
    add_zoom: Rc<Cell<Option<super::timeline::AddZoomTool>>>,
    /// Where the pointer is over the video while the tool is armed, in source
    /// pixels: the centre of the outline the tool draws.
    add_zoom_hover: Rc<Cell<Option<(f64, f64)>>>,
    add_zoom_bar: GtkBox,
    add_zoom_hint: Label,
    add_zoom_auto: Button,
    zoom_provider: CssProvider,
    last_zoom_transform: Rc<Cell<CameraTransform>>,
    last_zoom_css: Rc<RefCell<String>>,
}

impl Stage {
    /// Lay the stage out for the current frame, fill, padding, and radius.
    ///
    /// The stage carries the output canvas's aspect and the video rect is
    /// centred inside it: `Original` grows the canvas around the source when a
    /// fill is picked, while a fixed frame keeps its ratio and insets the video.
    /// The rounded clip is the video rect itself, so its corners always land on
    /// the footage rather than on the letterbox the fill reveals around it.
    fn apply_geometry(&self) {
        let Some(source) = self.source_size.get() else {
            self.strip_margins();
            self.set_stage_ratio(frame_ratio(self.aspect.get(), None));
            self.clip.set_radius(0.0, 1.0);
            self.zoom_area.set(None);
            return;
        };
        let widen = self.webcam.widens_stage();
        self.widened.set(widen);
        let padded = self.webcam.layouts().split_padded;
        self.padded.set(padded);
        let screen = source;
        let (source, gap) = split_stage(
            self.aspect.get(),
            screen,
            widen,
            padded,
            self.background.padding(),
            self.background.has_fill(),
        );
        self.webcam.set_stage_aspect(if source == screen {
            0.0
        } else {
            f64::from(source.0) / f64::from(source.1.max(1))
        });
        self.clip
            .set_screen_aspect(f64::from(screen.0) / f64::from(screen.1.max(1)));
        let aspect = self.aspect.get();
        let padding = self.background.padding();
        let has_fill = self.background.has_fill();
        let (out_w, out_h) = output_dimensions(aspect, source, padding, has_fill);
        let (video_w, video_h) = video_rect_dimensions(aspect, source, padding, has_fill);
        self.set_stage_ratio(out_w as f32 / out_h.max(1) as f32);
        self.set_video_margins((video_w, video_h), (out_w, out_h));
        let canvas = canvas_dimensions(aspect, source);
        let pad = padding_px(canvas, padding, has_fill).round().max(0.0);
        let (out_w, out_h) = (f64::from(out_w.max(1)), f64::from(out_h.max(1)));
        self.zoom_area.set(Some((
            pad / out_w,
            pad / out_h,
            ((out_w - pad * 2.0) / out_w).max(0.0),
            ((out_h - pad * 2.0) / out_h).max(0.0),
        )));
        let (radius, measured_width) =
            card_radius(aspect, source, padding, has_fill, self.background.radius());
        let pane_radius = if padded {
            radius / f64::from(video_h.max(1))
        } else {
            0.0
        };
        self.webcam.set_split_padding(gap, pane_radius);
        self.clip.set_radius(radius, measured_width);
    }

    /// Put the preview's video card where the layout says the screen goes.
    /// The split clips it to its share and cover-crops the source; every other
    /// layout fills the video rect, so the card keeps its old geometry.
    fn apply_webcam_layout(&self) {
        if self.source_size.get().is_some()
            && (self.webcam.widens_stage() != self.widened.get()
                || self.webcam.layouts().split_padded != self.padded.get())
        {
            self.apply_geometry();
        }
        let screen = self
            .webcam
            .layout_frame(self.source_size.get())
            .map(|frame| frame.screen)
            .filter(|screen| !screen.is_full());
        self.clip.set_layout_rect(
            screen.map(|screen| (screen.x, screen.y, screen.width, screen.height)),
        );
    }

    /// Set the stage aspect only when it really moves: `AspectFrame` re-runs
    /// layout on a ratio change, and the resize that follows calls back here.
    fn set_stage_ratio(&self, ratio: f32) {
        if (self.canvas.ratio() - ratio).abs() > 0.001 {
            self.canvas.set_ratio(ratio);
        }
    }

    /// Centre the video rect inside the stage with per-axis margins, the way the
    /// reference editor's preview does: an inset on both axes would change the
    /// clip's aspect and letterbox the footage inside its own rounded corners.
    fn set_video_margins(&self, video: (u32, u32), out: (u32, u32)) {
        let width = f64::from(self.overlay.allocated_width().max(0));
        let height = f64::from(self.overlay.allocated_height().max(0));
        if width < 2.0 || height < 2.0 {
            // Allocation not ready yet; the overlay's resize brings us back.
            self.strip_margins();
            return;
        }
        let margin_x = ((1.0 - f64::from(video.0) / f64::from(out.0.max(1))) / 2.0 * width)
            .round()
            .max(0.0) as i32;
        let margin_y = ((1.0 - f64::from(video.1) / f64::from(out.1.max(1))) / 2.0 * height)
            .round()
            .max(0.0) as i32;
        let margins = (margin_x, margin_x, margin_y, margin_y);
        if *self.clip_margins.borrow() == margins {
            return;
        }
        *self.clip_margins.borrow_mut() = margins;
        self.clip.set_margin_start(margin_x);
        self.clip.set_margin_end(margin_x);
        self.clip.set_margin_top(margin_y);
        self.clip.set_margin_bottom(margin_y);
    }

    fn strip_margins(&self) {
        let margins = (0, 0, 0, 0);
        if *self.clip_margins.borrow() == margins {
            return;
        }
        *self.clip_margins.borrow_mut() = margins;
        self.clip.set_margin_start(0);
        self.clip.set_margin_end(0);
        self.clip.set_margin_top(0);
        self.clip.set_margin_bottom(0);
    }

    /// The card rect, rest rect, and stage size in stage pixels for `camera`,
    /// or `None` when the studied framing does not apply: no recording, no
    /// layout yet, or a split layout, which keeps cropping inside its pane.
    fn card_geometry(
        &self,
        camera: super::zoom::Camera,
    ) -> Option<(super::zoom::StageRect, super::zoom::StageRect, (f64, f64))> {
        let (source_width, source_height) = self.source_size.get()?;
        let frame = self
            .webcam
            .layout_frame(Some((source_width, source_height)));
        if frame.is_some_and(|frame| !frame.screen.is_full()) {
            return None;
        }
        let stage_width = f64::from(self.overlay.allocated_width());
        let stage_height = f64::from(self.overlay.allocated_height());
        if stage_width < 2.0 || stage_height < 2.0 {
            return None;
        }
        let video = clip_video_rect(
            &self.clip,
            self.overlay.allocated_width(),
            self.overlay.allocated_height(),
            (source_width, source_height),
        )?;
        let rest = (video.x, video.y, video.width, video.height);
        let area = self
            .zoom_area
            .get()
            .map(|(x, y, w, h)| {
                (
                    x * stage_width,
                    y * stage_height,
                    w * stage_width,
                    h * stage_height,
                )
            })
            .unwrap_or(rest);
        // The camera overlay moves the aim into the room it leaves free. The
        // aim blends in with the overlay's opacity, so a layout change fades
        // the card across instead of snapping it.
        let centre = (area.0 + area.2 / 2.0, area.1 + area.3 / 2.0);
        let focal = match frame.filter(|frame| frame.camera_alpha > 0.0) {
            Some(frame) => {
                let camera = (
                    video.x + frame.camera.x * video.width,
                    video.y + frame.camera.y * video.height,
                    frame.camera.width * video.width,
                    frame.camera.height * video.height,
                );
                let aim = super::zoom::focal_point(area, Some(camera));
                let alpha = frame.camera_alpha.clamp(0.0, 1.0);
                (
                    centre.0 + (aim.0 - centre.0) * alpha,
                    centre.1 + (aim.1 - centre.1) * alpha,
                )
            }
            None => centre,
        };
        let target01 = (
            camera.1 .0 / f64::from(source_width.max(1)),
            camera.1 .1 / f64::from(source_height.max(1)),
        );
        let card = super::zoom::card_rect(rest, area, focal, camera.0, target01);
        Some((card, rest, (stage_width, stage_height)))
    }

    /// The source view that expresses `camera` on the rest rect: the studied
    /// card when it applies, the plain crop otherwise.
    fn camera_view_for(&self, camera: super::zoom::Camera) -> Option<super::zoom::ViewRect> {
        let (width, height) = self.source_size.get()?;
        let source = (f64::from(width), f64::from(height));
        Some(match self.card_geometry(camera) {
            Some((card, rest, _)) => super::zoom::card_view(rest, card, source),
            None => super::zoom::view_rect(camera.0, camera.1, source),
        })
    }

    fn set_camera(&self, camera: super::zoom::Camera) {
        let source_size = self.source_size.get();
        let geometry = self.card_geometry(camera);
        let view = self.camera_view_for(camera);
        self.camera_view.set(view);
        self.apply_card(geometry, camera);

        let transform = match (source_size, view) {
            (Some((width, height)), Some(view)) => {
                let clip_width = f64::from(self.clip.allocated_width().max(0));
                let clip_height = f64::from(self.clip.allocated_height().max(0));
                if clip_width < 2.0 || clip_height < 2.0 {
                    return;
                }
                super::zoom::camera_transform(
                    view,
                    f64::from(width),
                    f64::from(height),
                    clip_width,
                    clip_height,
                )
            }
            _ => IDENTITY_CAMERA_TRANSFORM,
        };
        self.update_camera_transform(transform);
    }

    /// Draw the card where the zoom put it and drift the background with it.
    fn apply_card(
        &self,
        geometry: Option<(super::zoom::StageRect, super::zoom::StageRect, (f64, f64))>,
        camera: super::zoom::Camera,
    ) {
        let card = geometry.and_then(|(card, rest, stage)| {
            let moved = (card.0 - rest.0).abs() > 0.01
                || (card.1 - rest.1).abs() > 0.01
                || (card.2 - rest.2).abs() > 0.01
                || (card.3 - rest.3).abs() > 0.01;
            moved.then_some((
                (card.0 - rest.0, card.1 - rest.1, card.2, card.3),
                (-rest.0, -rest.1, stage.0, stage.1),
            ))
        });
        self.clip.set_card(card);

        let background = match (geometry, self.source_size.get()) {
            (Some(_), Some((width, height))) => (
                super::zoom::background_zoom(camera.0),
                (
                    (camera.1 .0 / f64::from(width.max(1))).clamp(0.0, 1.0),
                    (camera.1 .1 / f64::from(height.max(1))).clamp(0.0, 1.0),
                ),
            ),
            _ => (1.0, (0.5, 0.5)),
        };
        let (old_zoom, old_pivot) = self.background_zoom.get();
        if (old_zoom - background.0).abs() > 1e-5
            || (old_pivot.0 - background.1 .0).abs() > 1e-5
            || (old_pivot.1 - background.1 .1).abs() > 1e-5
        {
            self.background_zoom.set(background);
            self.background_layer.queue_draw();
        }
    }

    fn reset_camera(&self) {
        self.camera_view.set(None);
        self.clip.set_card(None);
        self.background_zoom.set((1.0, (0.5, 0.5)));
        self.background_layer.queue_draw();
        self.update_camera_transform(IDENTITY_CAMERA_TRANSFORM);
    }

    fn update_camera_transform(&self, transform: CameraTransform) {
        let transform = if is_identity_camera_transform(transform) {
            IDENTITY_CAMERA_TRANSFORM
        } else {
            transform
        };
        let previous = self.last_zoom_transform.get();
        if camera_transforms_equal(previous, transform) {
            return;
        }

        let css = if transform == IDENTITY_CAMERA_TRANSFORM {
            IDENTITY_CAMERA_CSS.to_string()
        } else {
            let (tx, ty, sx, sy) = transform;
            format!(
                ".recording-editor-video {{ transform-origin: 0px 0px; transform: translate({tx:.6}px, {ty:.6}px) scale({sx:.6}, {sy:.6}); }}"
            )
        };
        if *self.last_zoom_css.borrow() != css {
            self.zoom_provider.load_from_data(&css);
            self.last_zoom_css.replace(css);
        }
        self.last_zoom_transform.set(transform);
    }

    /// Paint the picked wallpaper behind the video. The video keeps its own
    /// aspect and sits inset by the panel's Padding, so the wallpaper shows
    /// around it exactly as it does in the export.
    fn apply_background(&self) {
        let selection = self.background.selection();
        if *self.background_selection.borrow() != selection {
            let pixbuf = selection
                .as_deref()
                .and_then(super::background::load_pixbuf);
            *self.background_pixbuf.borrow_mut() = pixbuf;
            *self.background_selection.borrow_mut() = selection;
        }
        // Copied every time rather than diffed: the panel rebuilds its painter
        // on each edit, so a new handle is the signal, and a drag repaints the
        // stage through it.
        *self.background_fill.borrow_mut() = self.background.custom_fill();
        self.background_layer.queue_draw();
        self.apply_geometry();
    }

    /// Show or clear the Manual zoom outline. While it is up the preview is
    /// unzoomed, so the outline says where the zoom will look before you see it.
    fn set_placement(&self, placement: Option<(f64, (f64, f64))>) {
        if self.placement.get() == placement {
            return;
        }
        self.placement.set(placement);
        self.canvas.set_cursor_from_name(
            (placement.is_some() || self.add_zoom.get().is_some()).then_some("crosshair"),
        );
        self.cursor_layer.queue_draw();
    }

    /// Show or put away the armed Add zoom tool: its prompt, the Automatic
    /// zoom button when the recording can follow the pointer, and the outline
    /// that follows the pointer.
    fn set_add_zoom(&self, tool: Option<super::timeline::AddZoomTool>) {
        if self.add_zoom.get() == tool {
            return;
        }
        self.add_zoom.set(tool);
        if tool.is_none() {
            self.add_zoom_hover.set(None);
        }
        let automatic = tool.is_some_and(|tool| tool.automatic);
        self.add_zoom_hint.set_text(&if automatic {
            t("Click where to zoom in, or use Automatic zoom")
        } else {
            t("Click where to zoom in")
        });
        self.add_zoom_auto.set_visible(automatic);
        self.add_zoom_bar.set_visible(tool.is_some());
        self.canvas.set_cursor_from_name(
            (tool.is_some() || self.placement.get().is_some()).then_some("crosshair"),
        );
        self.cursor_layer.queue_draw();
    }

    /// The source pixel under a point in the preview overlay, or `None` off the
    /// video.
    fn source_point_at(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (source_width, source_height) = self.source_size.get()?;
        let video = clip_video_rect(
            &self.clip,
            self.overlay.allocated_width(),
            self.overlay.allocated_height(),
            (source_width, source_height),
        )?;
        let video = match self
            .webcam
            .layout_frame(Some((source_width, source_height)))
            .map(|frame| frame.screen)
            .filter(|screen| !screen.is_full())
        {
            Some(screen) => {
                let draw = screen_transform(
                    video,
                    screen,
                    f64::from(source_width) / f64::from(source_height.max(1)),
                )
                .draw;
                super::cursor::VideoRect {
                    x: draw.x,
                    y: draw.y,
                    width: draw.width,
                    height: draw.height,
                }
            }
            None => video,
        };
        if x < video.x || y < video.y || x > video.x + video.width || y > video.y + video.height {
            return None;
        }
        let view = self.camera_view.get().unwrap_or((
            0.0,
            0.0,
            f64::from(source_width),
            f64::from(source_height),
        ));
        Some(super::zoom::stage_point_to_source(
            (x, y),
            view,
            [video.x, video.y, video.width, video.height],
        ))
    }

    /// With the Add zoom tool armed, the outline follows the pointer and a
    /// click on the video places a zoom there; the prompt's button adds an
    /// automatic one instead.
    fn connect_add_zoom(&self, place: Rc<dyn Fn((f64, f64))>, automatic: Rc<dyn Fn()>) {
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion({
            let stage = self.clone();
            move |_, x, y| {
                if stage.add_zoom.get().is_none() {
                    return;
                }
                stage.add_zoom_hover.set(stage.source_point_at(x, y));
                stage.cursor_layer.queue_draw();
            }
        });
        motion.connect_leave({
            let stage = self.clone();
            move |_| {
                if stage.add_zoom_hover.take().is_some() {
                    stage.cursor_layer.queue_draw();
                }
            }
        });
        self.overlay.add_controller(motion);

        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_released({
            let stage = self.clone();
            move |_, _, x, y| {
                if stage.add_zoom.get().is_none() {
                    return;
                }
                // A press on the prompt's button is that button's, not a placement.
                if let Some(picked) = stage.overlay.pick(x, y, gtk4::PickFlags::DEFAULT) {
                    let button = stage.add_zoom_auto.upcast_ref::<gtk4::Widget>();
                    if &picked == button || picked.is_ancestor(button) {
                        return;
                    }
                }
                if let Some(point) = stage.source_point_at(x, y) {
                    place(point);
                }
            }
        });
        self.overlay.add_controller(click);

        self.add_zoom_auto.connect_clicked(move |_| automatic());
    }

    /// Dragging the preview places the selected Manual zoom's focus point.
    fn connect_placement(&self, place: Rc<dyn Fn((f64, f64))>) {
        let canvas = self.canvas.clone();
        let source_size = self.source_size.clone();
        let camera_view = self.camera_view.clone();
        let placement = self.placement.clone();
        let place_point: Rc<dyn Fn(f64, f64)> = Rc::new(move |x: f64, y: f64| {
            // The outline is only up while a Manual zoom is selected and the
            // transport is stopped; a drag at any other time is not a placement.
            if placement.get().is_none() {
                return;
            }
            let Some((source_width, source_height)) = source_size.get() else {
                return;
            };
            let (width, height) = (
                f64::from(canvas.allocated_width().max(1)),
                f64::from(canvas.allocated_height().max(1)),
            );
            let (source_width, source_height) = (f64::from(source_width), f64::from(source_height));
            let Some(video) =
                super::cursor::video_rect_for(width, height, source_width, source_height)
            else {
                return;
            };
            let view = camera_view
                .get()
                .unwrap_or((0.0, 0.0, source_width, source_height));
            place(super::zoom::stage_point_to_source(
                (x, y),
                view,
                [video.x, video.y, video.width, video.height],
            ));
        });

        let drag = gtk4::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let place = place_point.clone();
            move |_, x, y| place(x, y)
        });
        drag.connect_drag_update({
            let place = place_point.clone();
            move |gesture, offset_x, offset_y| {
                if let Some((start_x, start_y)) = gesture.start_point() {
                    place(start_x + offset_x, start_y + offset_y);
                }
            }
        });
        self.canvas.add_controller(drag);
    }

    fn connect_open(&self, open: Rc<dyn Fn()>) {
        let picture = self.picture.clone();
        let click = gtk4::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            if !picture.is_visible() {
                open();
            }
        });
        self.canvas.add_controller(click);
    }

    fn show_document(&self, path: Option<&Path>, source_seconds: f64, composition_seconds: f64) {
        self.reset_camera();
        self.cursor.load(path);
        self.source_seconds.set(0.0);
        self.cursor_layer.queue_draw();
        let clean_source = path.and_then(|video| self.cursor.sidecar()?.editable_video_path(video));
        let preview_source = clean_source.as_deref().or(path);
        let Some((texture, width, height)) = preview_source.and_then(poster_texture) else {
            self.export_button.set_sensitive(false);
            self.picture.set_visible(false);
            self.hint.set_visible(true);
            self.source_size.set(None);
            self.preview.set_document(None, 0, 0);
            self.apply_geometry();
            self.apply_webcam_layout();
            self.estimate.set_source(None, 0.0, 0.0);
            return;
        };
        self.picture.set_paintable(Some(&texture));
        self.picture.set_visible(true);
        self.hint.set_visible(false);
        self.source_size.set(Some((width, height)));
        self.export_button.set_sensitive(true);
        self.apply_geometry();
        self.apply_webcam_layout();
        self.set_camera((1.0, (f64::from(width) / 2.0, f64::from(height) / 2.0)));
        self.cursor_layer.queue_draw();
        if let Some(path) = path {
            self.preview.set_document(preview_source, width, height);
            self.estimate.set_pixels(width, height);
            self.estimate
                .set_source(Some(path), source_seconds, composition_seconds);
        }
    }
}

/// The video card's rect inside the stage: where the picture, its zoom, the
/// cursor and every layout share their origin. The clip carries the margins
/// the stage gives the footage, so reading its allocation keeps all of them on
/// the same rectangle even when a background fill insets the video.
fn clip_video_rect(
    clip: &super::rounded_clip::RoundedClip,
    allocation_width: i32,
    allocation_height: i32,
    source_size: (u32, u32),
) -> Option<super::cursor::VideoRect> {
    let width = clip.allocated_width();
    let height = clip.allocated_height();
    if width > 1 && height > 1 {
        return Some(super::cursor::VideoRect {
            x: f64::from(clip.margin_start()),
            y: f64::from(clip.margin_top()),
            width: f64::from(width),
            height: f64::from(height),
        });
    }
    super::cursor::video_rect_for(
        f64::from(allocation_width),
        f64::from(allocation_height),
        f64::from(source_size.0),
        f64::from(source_size.1),
    )
}

/// The screen's share of the video rect, in stage pixels: the cover rect the
/// whole source draws into and the pane it is clipped to.
fn screen_transform(
    video: super::cursor::VideoRect,
    screen: super::webcam::Rect,
    screen_aspect: f64,
) -> ScreenView {
    let placement = super::rounded_clip::pane_placement(
        video.width,
        video.height,
        (screen.x, screen.y, screen.width, screen.height),
        screen_aspect,
    );
    let place = |(x, y, width, height): (f64, f64, f64, f64)| super::cursor::VideoRect {
        x: video.x + x,
        y: video.y + y,
        width,
        height,
    };
    ScreenView {
        draw: place(placement.source),
        clip: place(placement.clip),
    }
}

/// Draw `draw` in the video rect, or over the layout's screen pane with the
/// cover rect's allocation and the pane as its clip.
fn draw_in_video(
    context: &gtk4::cairo::Context,
    video: Option<super::cursor::VideoRect>,
    screen: Option<ScreenView>,
    allocation: (i32, i32),
    draw: impl FnOnce(&gtk4::cairo::Context, i32, i32),
) {
    let Some(video) = video else {
        draw(context, allocation.0, allocation.1);
        return;
    };
    let _ = context.save();
    match screen {
        Some(view) => {
            context.rectangle(view.clip.x, view.clip.y, view.clip.width, view.clip.height);
            context.clip();
            context.translate(view.draw.x, view.draw.y);
            draw(
                context,
                view.draw.width.round().max(1.0) as i32,
                view.draw.height.round().max(1.0) as i32,
            );
        }
        None => {
            context.translate(video.x, video.y);
            draw(context, video.width as i32, video.height as i32);
        }
    }
    let _ = context.restore();
}

/// The orange outline a Manual zoom shows over an unzoomed preview: the region
/// its focus point and scale will frame.
fn draw_placement_outline(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    source_size: Option<(u32, u32)>,
    view: Option<super::zoom::ViewRect>,
    placement: Option<(f64, (f64, f64))>,
    scale: f64,
) {
    let (Some((source_width, source_height)), Some(view), Some((zoom_scale, center))) =
        (source_size, view, placement)
    else {
        return;
    };
    let (source_width, source_height) = (f64::from(source_width), f64::from(source_height));
    let Some(video) = super::cursor::video_rect_for(
        f64::from(width),
        f64::from(height),
        source_width,
        source_height,
    ) else {
        return;
    };
    let [x, y, rect_width, rect_height] = super::zoom::placement_rect(
        zoom_scale,
        center,
        view,
        [video.x, video.y, video.width, video.height],
    );
    context.set_source_rgba(1.0, 0.48, 0.12, 0.22);
    context.rectangle(x, y, rect_width, rect_height);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 0.62, 0.20, 0.95);
    context.set_line_width(2.0 * scale);
    let _ = context.stroke();
}

fn is_identity_camera_transform(transform: CameraTransform) -> bool {
    transform.0.abs() < 0.5
        && transform.1.abs() < 0.5
        && (transform.2 - 1.0).abs() < 0.002
        && (transform.3 - 1.0).abs() < 0.002
}

fn camera_transforms_equal(left: CameraTransform, right: CameraTransform) -> bool {
    (left.0 - right.0).abs() <= CAMERA_TRANSFORM_EPSILON
        && (left.1 - right.1).abs() <= CAMERA_TRANSFORM_EPSILON
        && (left.2 - right.2).abs() <= CAMERA_TRANSFORM_EPSILON
        && (left.3 - right.3).abs() <= CAMERA_TRANSFORM_EPSILON
}

fn build_stage(
    scale: f64,
    background: super::background::Background,
    export_button: Button,
) -> Stage {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-stage");
    root.set_hexpand(true);
    root.set_vexpand(true);

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("recording-editor-preview-frame");
    frame.set_hexpand(true);
    frame.set_vexpand(true);

    let workspace = GtkBox::new(Orientation::Vertical, 0);
    workspace.add_css_class("recording-editor-preview-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let canvas = AspectFrame::new(0.5, 0.5, frame_ratio(FrameAspect::Original, None), false);
    canvas.add_css_class("recording-editor-preview-stage");
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    canvas.set_halign(Align::Fill);
    canvas.set_valign(Align::Fill);

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-preview-canvas");
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_overflow(gtk4::Overflow::Hidden);

    let background_layer = DrawingArea::new();
    background_layer.add_css_class("recording-editor-preview-background");
    background_layer.set_hexpand(true);
    background_layer.set_vexpand(true);
    background_layer.set_can_target(false);
    // The layer stays visible with nothing to paint so its allocation always
    // tracks the stage; the draw func no-ops while no fill is picked.
    let background_pixbuf = Rc::new(RefCell::new(None::<Pixbuf>));
    let background_fill: Rc<RefCell<Option<super::background::FillPainter>>> =
        Rc::new(RefCell::new(None));
    let background_zoom = Rc::new(Cell::new((1.0_f64, (0.5_f64, 0.5_f64))));
    background_layer.set_draw_func({
        let background_pixbuf = background_pixbuf.clone();
        let background_fill = background_fill.clone();
        let background_zoom = background_zoom.clone();
        move |_, context, width, height| {
            // The background drifts in with the screen zoom, about the target.
            let (zoom, (pivot_x, pivot_y)) = background_zoom.get();
            if zoom > 1.0 + 1e-6 {
                let x = pivot_x * f64::from(width);
                let y = pivot_y * f64::from(height);
                context.translate(x, y);
                context.scale(zoom, zoom);
                context.translate(-x, -y);
            }
            if let Some(pixbuf) = background_pixbuf.borrow().as_ref() {
                super::background::paint_cover(context, width, height, pixbuf);
                return;
            }
            if let Some(paint) = background_fill.borrow().as_ref() {
                paint(context, width, height);
            }
        }
    });
    overlay.set_child(Some(&background_layer));

    let source_size = Rc::new(Cell::new(None::<(u32, u32)>));
    let source_seconds = Rc::new(Cell::new(0.0));
    let cursor = super::cursor::CursorModel::new();
    let zoom_provider = CssProvider::new();
    zoom_provider.load_from_data(IDENTITY_CAMERA_CSS);
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &zoom_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
        );
    }
    let last_zoom_transform = Rc::new(Cell::new(IDENTITY_CAMERA_TRANSFORM));
    let last_zoom_css = Rc::new(RefCell::new(IDENTITY_CAMERA_CSS.to_string()));
    let camera_view = Rc::new(Cell::new(None::<super::zoom::ViewRect>));
    let zoom_area = Rc::new(Cell::new(None::<super::zoom::StageRect>));

    let picture = Picture::new();
    picture.add_css_class("recording-editor-video");
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_can_target(false);
    picture.set_visible(false);
    let clip = super::rounded_clip::RoundedClip::new(&picture);
    clip.add_css_class("recording-editor-preview-clip");
    clip.set_hexpand(true);
    clip.set_vexpand(true);
    overlay.add_overlay(&clip);

    // While the zoomed card moves, its blur is drawn whole over the stage here,
    // edges and all, in place of the sharp card.
    let card_layer = Picture::new();
    card_layer.set_can_target(false);
    card_layer.set_can_shrink(true);
    card_layer.set_content_fit(gtk4::ContentFit::Fill);
    card_layer.set_hexpand(true);
    card_layer.set_vexpand(true);
    card_layer.set_visible(false);
    overlay.add_overlay(&card_layer);

    let cursor_layer = DrawingArea::new();
    cursor_layer.set_hexpand(true);
    cursor_layer.set_vexpand(true);
    cursor_layer.set_can_target(false);
    // A still playhead gets one repaint when its camera frame lands; playback
    // repaints on every preview frame anyway.
    let webcam = super::webcam::WebcamModel::new(Rc::new({
        let layer = cursor_layer.clone();
        move || layer.queue_draw()
    }));
    let placement: Placement = Rc::new(Cell::new(None));
    let add_zoom: Rc<Cell<Option<super::timeline::AddZoomTool>>> = Rc::new(Cell::new(None));
    let add_zoom_hover: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
    cursor_layer.set_draw_func({
        let cursor = cursor.clone();
        let webcam = webcam.clone();
        let clip = clip.clone();
        let source_size = source_size.clone();
        let source_seconds = source_seconds.clone();
        let camera_view = camera_view.clone();
        let placement = placement.clone();
        let add_zoom = add_zoom.clone();
        let add_zoom_hover = add_zoom_hover.clone();
        move |_, context, width, height| {
            let source = source_size.get();
            let video = source.and_then(|source| clip_video_rect(&clip, width, height, source));
            let screen = webcam
                .layout_frame(source)
                .map(|frame| frame.screen)
                .filter(|screen| !screen.is_full());
            let source_aspect = source
                .map(|(w, h)| f64::from(w) / f64::from(h.max(1)))
                .unwrap_or(0.0);
            let view = match (video, screen) {
                (Some(video), Some(screen)) => Some(screen_transform(video, screen, source_aspect)),
                _ => None,
            };
            draw_in_video(
                context,
                video,
                view,
                (width, height),
                |context, width, height| {
                    cursor.draw(
                        context,
                        width,
                        height,
                        source_seconds.get(),
                        source,
                        camera_view.get(),
                    );
                },
            );
            webcam.draw(
                context,
                width,
                height,
                source,
                video.map(|video| super::webcam::Rect {
                    x: video.x,
                    y: video.y,
                    width: video.width,
                    height: video.height,
                }),
            );
            draw_in_video(
                context,
                video,
                view,
                (width, height),
                |context, width, height| {
                    draw_placement_outline(
                        context,
                        width,
                        height,
                        source,
                        camera_view.get(),
                        placement.get().or_else(|| {
                            add_zoom
                                .get()
                                .zip(add_zoom_hover.get())
                                .map(|(tool, point)| (tool.scale, point))
                        }),
                        scale,
                    );
                },
            );
        }
    });
    overlay.add_overlay(&cursor_layer);

    let hint = Label::new(Some(&t("Click to open a video, or drop one here")));
    hint.add_css_class("recording-editor-empty-preview-hint");
    hint.set_halign(Align::Center);
    hint.set_valign(Align::Center);
    hint.set_wrap(true);
    hint.set_can_target(false);
    overlay.add_overlay(&hint);

    // The armed Add zoom tool's prompt, along the bottom of the preview.
    let add_zoom_bar = GtkBox::new(Orientation::Horizontal, (8.0 * scale).round() as i32);
    add_zoom_bar.add_css_class("video-editor-add-zoom-bar");
    add_zoom_bar.set_halign(Align::Center);
    add_zoom_bar.set_valign(Align::End);
    add_zoom_bar.set_margin_bottom((16.0 * scale).round() as i32);
    add_zoom_bar.set_visible(false);
    let add_zoom_hint = Label::new(Some(&t("Click where to zoom in")));
    add_zoom_hint.add_css_class("video-editor-add-zoom-hint");
    add_zoom_hint.set_can_target(false);
    let add_zoom_auto = Button::with_label(&t("Automatic zoom"));
    add_zoom_auto.add_css_class("video-editor-add-zoom-auto");
    add_zoom_auto.set_tooltip_text(Some(&t(
        "Adds a zoom that automatically follows the mouse activity",
    )));
    add_zoom_bar.append(&add_zoom_hint);
    add_zoom_bar.append(&add_zoom_auto);
    overlay.add_overlay(&add_zoom_bar);

    canvas.set_child(Some(&overlay));
    workspace.append(&canvas);
    frame.append(&workspace);

    let aspect = Rc::new(Cell::new(FrameAspect::Original));
    let preview = super::preview::Preview::new(picture.clone());
    preview.set_card_layer(card_layer.clone(), clip.clone().upcast());
    let stage_aspect_label = Label::new(Some(&FrameAspect::Original.label()));
    stage_aspect_label.add_css_class("recording-editor-stage-chip-label");

    let quality = Rc::new(Cell::new(QualityTier::default()));
    let estimate_label = Label::new(None);
    estimate_label.add_css_class("recording-editor-estimate");
    estimate_label.set_valign(Align::Center);
    estimate_label.set_tooltip_text(Some(&t("Estimated export size")));
    let estimate = Estimate::new(estimate_label.clone(), aspect.clone(), quality.clone());

    root.append(&frame);

    let tools = GtkBox::new(Orientation::Horizontal, 10);
    tools.add_css_class("recording-editor-stage-tools");
    tools.set_halign(Align::Center);
    tools.set_valign(Align::End);
    tools.set_hexpand(true);

    let aspect_chip = Button::new();
    aspect_chip.set_has_frame(false);
    aspect_chip.add_css_class("recording-editor-stage-chip");
    aspect_chip.set_tooltip_text(Some(&t("Change aspect ratio")));
    aspect_chip.set_child(Some(&stage_aspect_label));

    let crop = Button::new();
    crop.set_has_frame(false);
    crop.add_css_class("recording-editor-stage-chip");
    crop.set_tooltip_text(Some(&t("Crop video")));
    let crop_row = GtkBox::new(Orientation::Horizontal, 6);
    let crop_icon = Image::from_icon_name(icon_names::custom::CROP_SYMBOLIC);
    crop_icon.set_pixel_size((13.0 * scale).round() as i32);
    let crop_label = Label::new(Some(&t("Crop video")));
    crop_label.add_css_class("recording-editor-stage-chip-label");
    crop_row.append(&crop_icon);
    crop_row.append(&crop_label);
    crop.set_child(Some(&crop_row));

    let quality_label = Label::new(Some(&quality.get().label()));
    quality_label.add_css_class("recording-editor-stage-chip-label");
    let quality_button = Button::new();
    quality_button.set_has_frame(false);
    quality_button.add_css_class("recording-editor-stage-chip");
    quality_button.set_tooltip_text(Some(&t("Export quality")));
    quality_button.set_child(Some(&quality_label));
    wire_quality_menu(&quality_button, quality.clone(), {
        let quality_label = quality_label.clone();
        let estimate = estimate.clone();
        Rc::new(move |tier| {
            quality_label.set_text(&tier.label());
            // The tier brackets the exported size, so the readout moves too.
            estimate.refresh();
        })
    });

    tools.append(&aspect_chip);
    tools.append(&stage_rule());
    tools.append(&crop);
    tools.append(&stage_rule());
    tools.append(&quality_button);
    tools.append(&stage_rule());
    tools.append(&estimate_label);
    root.append(&tools);

    let stage = Stage {
        root,
        canvas,
        overlay: overlay.clone(),
        picture: picture.clone(),
        hint,
        aspect: aspect.clone(),
        preview,
        estimate: estimate.clone(),
        quality: quality.clone(),
        export_button: export_button.clone(),
        background: background.state(),
        background_layer: background_layer.clone(),
        cursor_layer,
        cursor,
        webcam,
        source_seconds,
        background_pixbuf,
        background_fill,
        background_selection: Rc::new(RefCell::new(None)),
        source_size,
        widened: Cell::new(false),
        padded: Cell::new(false),
        clip,
        clip_margins: Rc::new(RefCell::new((i32::MIN, 0, 0, 0))),
        camera_view,
        zoom_area,
        background_zoom,
        placement,
        add_zoom,
        add_zoom_hover,
        add_zoom_bar,
        add_zoom_hint,
        add_zoom_auto,
        zoom_provider,
        last_zoom_transform,
        last_zoom_css,
    };

    let apply_aspect: Rc<dyn Fn(FrameAspect)> = {
        let stage = stage.clone();
        let stage_label = stage_aspect_label.clone();
        Rc::new(move |choice| {
            stage.aspect.set(choice);
            stage_label.set_text(&choice.label());
            // A fixed frame exports at its own pixel count, so the estimate
            // moves with the pick, and the corner radius is measured against
            // that canvas.
            stage.estimate.refresh();
            stage.apply_geometry();
        })
    };
    wire_aspect_menu(&aspect_chip, aspect.get(), apply_aspect);

    background_layer.connect_resize({
        let stage = stage.clone();
        move |_, _, _| stage.apply_geometry()
    });
    background.connect({
        let stage = stage.clone();
        Rc::new(move || stage.apply_background())
    });
    stage
}

fn wire_aspect_menu(button: &Button, active: FrameAspect, choose: Rc<dyn Fn(FrameAspect)>) {
    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Top);
    popover.add_css_class("recording-editor-stage-aspect");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("recording-editor-stage-aspect-list");
    popover.set_child(Some(&list));
    popover.set_parent(button);

    for choice in [
        FrameAspect::Original,
        FrameAspect::Wide,
        FrameAspect::Portrait,
    ] {
        let item = Button::new();
        item.set_has_frame(false);
        item.add_css_class("recording-editor-stage-aspect-item");
        if choice == active {
            item.add_css_class("recording-editor-stage-aspect-item-active");
        }
        item.set_hexpand(true);
        item.set_child(Some(&Label::new(Some(&choice.label()))));
        let choose = choose.clone();
        let popover = popover.clone();
        item.connect_clicked(move |_| {
            choose(choice);
            popover.popdown();
        });
        list.append(&item);
    }

    button.connect_clicked(move |_| popover.popup());
}

fn wire_quality_menu(
    button: &Button,
    quality: Rc<Cell<QualityTier>>,
    on_change: Rc<dyn Fn(QualityTier)>,
) {
    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Top);
    popover.add_css_class("recording-editor-stage-aspect");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("recording-editor-stage-aspect-list");
    popover.set_child(Some(&list));
    popover.set_parent(button);

    for tier in [QualityTier::Balanced, QualityTier::High, QualityTier::Ultra] {
        let item = Button::new();
        item.set_has_frame(false);
        item.add_css_class("recording-editor-stage-aspect-item");
        if tier == quality.get() {
            item.add_css_class("recording-editor-stage-aspect-item-active");
        }
        item.set_hexpand(true);
        item.set_child(Some(&Label::new(Some(&tier.label()))));
        let quality = quality.clone();
        let on_change = on_change.clone();
        let popover = popover.clone();
        item.connect_clicked(move |_| {
            quality.set(tier);
            on_change(tier);
            popover.popdown();
        });
        list.append(&item);
    }

    button.connect_clicked(move |_| popover.popup());
}

fn stage_rule() -> Separator {
    let rule = Separator::new(Orientation::Vertical);
    rule.add_css_class("recording-editor-stage-rule");
    rule.set_valign(Align::Center);
    rule
}

/// The recording's first frame, extracted once by ffmpeg and cached beside the
/// other thumbnails. The clip rounds its corners, so the poster is a plain
/// texture.
fn poster_texture(path: &Path) -> Option<(gdk::Texture, u32, u32)> {
    let directory = crate::recording::editor::ffmpeg::thumbnail_cache_dir(path);
    let output = directory.join("poster.png");
    if !output.is_file() {
        crate::recording::editor::ffmpeg::extract_poster_frame(path, &output, 0.0).ok()?;
    }
    let texture = gdk::Texture::from_filename(&output).ok()?;
    let (width, height) = image::image_dimensions(&output).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((texture, width, height))
}
fn choose_recording(window: &ApplicationWindow, update_document: UpdateDocument) {
    let chooser = FileChooserNative::new(
        Some(&t("Open recording")),
        Some(window),
        FileChooserAction::Open,
        Some(&t("Open")),
        Some(&t("Cancel")),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&t("MP4 recordings")));
    filter.add_pattern("*.mp4");
    filter.add_pattern("*.MP4");
    chooser.set_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response == ResponseType::Accept {
            if let Some(path) = chooser.file().and_then(|file| file.path()) {
                if is_supported_video(&path) {
                    update_document(Some(&path));
                }
            }
        }
        chooser.hide();
    });
    chooser.show();
}

/// The canvas the stage shows around the video, rendered for the export: the
/// fill at the canvas size, the centred video rect, and the card's corner
/// radius. None when the video is the canvas and nothing is rounded.
fn export_canvas(stage: &Stage, source_size: (u32, u32)) -> Option<super::export::CanvasLayout> {
    let source_size = split_stage(
        stage.aspect.get(),
        source_size,
        stage.webcam.widens_stage(),
        stage.webcam.layouts().split_padded,
        stage.background.padding(),
        stage.background.has_fill(),
    )
    .0;
    let aspect = stage.aspect.get();
    let padding = stage.background.padding();
    let has_fill = stage.background.has_fill();
    let (radius, _) = card_radius(
        aspect,
        source_size,
        padding,
        has_fill,
        stage.background.radius(),
    );
    if !has_fill && aspect == FrameAspect::Original && radius <= 0.0 {
        return None;
    }
    let size = output_dimensions(aspect, source_size, padding, has_fill);
    let video = video_rect_dimensions(aspect, source_size, padding, has_fill);
    let mut surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        size.0 as i32,
        size.1 as i32,
    )
    .ok()?;
    {
        let context = gtk4::cairo::Context::new(&surface).ok()?;
        context.set_source_rgb(0.0, 0.0, 0.0);
        let _ = context.paint();
        // The same fill the stage's background layer paints, in the same order.
        if let Some(pixbuf) = stage.background_pixbuf.borrow().as_ref() {
            super::background::paint_cover(&context, size.0 as i32, size.1 as i32, pixbuf);
        } else if let Some(paint) = stage.background_fill.borrow().as_ref() {
            paint(&context, size.0 as i32, size.1 as i32);
        }
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let width = size.0 as usize;
    let mut background = vec![0u8; width * size.1 as usize * 4];
    for (row, line) in background.chunks_exact_mut(width * 4).enumerate() {
        let source = &data[row * stride..row * stride + width * 4];
        for (target, bgra) in line
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<4>().0)
        {
            target.copy_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
        }
    }
    Some(super::export::CanvasLayout {
        size,
        video,
        radius,
        background: std::sync::Arc::new(background),
    })
}

/// The composition an export should write, read fresh when the user commits a
/// destination so a last-second trim or zoom is included.
fn export_model(
    stage: &Stage,
    timeline: &super::timeline::Timeline,
) -> Option<super::export::ExportModel> {
    let footage = timeline.export_footage()?;
    let source_size = stage.source_size.get()?;
    let sidecar = stage.cursor.sidecar();
    let source = sidecar
        .as_ref()
        .and_then(|sidecar| sidecar.editable_video_path(&footage.source))
        .unwrap_or(footage.source);
    Some(super::export::ExportModel {
        source,
        source_size,
        trim_start: footage.trim_start,
        trim_end: footage.trim_end,
        cuts: footage.cuts,
        zooms: footage.zooms,
        zoom_camera: footage.zoom_camera,
        cursor: stage.cursor.settings(),
        sidecar,
        webcam: stage.webcam.export(),
        stage: {
            let widened = split_stage(
                stage.aspect.get(),
                source_size,
                stage.webcam.widens_stage(),
                stage.webcam.layouts().split_padded,
                stage.background.padding(),
                stage.background.has_fill(),
            )
            .0;
            (widened != source_size).then_some(widened)
        },
        canvas: export_canvas(stage, source_size),
        crf: stage.quality.get().crf(),
    })
}

/// What a Ctrl+key combination asks the document's history to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HistoryKey {
    Undo,
    Redo,
}

/// Map a key press to a history action.
///
/// The key is read through its Unicode value rather than the `gdk::Key`
/// variant, so Ctrl+Shift+Z works whether the backend reports it as `z` or
/// `Z`, and Ctrl+Y is accepted alongside it.
fn history_key(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<HistoryKey> {
    if !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
        return None;
    }
    let redo = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
    match key.to_unicode().map(|c| c.to_ascii_lowercase()) {
        Some('z') if redo => Some(HistoryKey::Redo),
        Some('z') => Some(HistoryKey::Undo),
        Some('y') => Some(HistoryKey::Redo),
        _ => None,
    }
}

/// Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y for the document's undo history.
///
/// The controller sits on the window and runs in the bubble phase: a focused
/// text entry keeps its own Ctrl+Z, because GTK offers the key to the focused
/// widget's controllers first.
fn wire_history_keys(
    window: &ApplicationWindow,
    timeline: &super::timeline::Timeline,
    shell: &Overlay,
) {
    let keys = gtk4::EventControllerKey::new();
    let timeline = timeline.clone();
    let shell = shell.clone();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if shell.child().is_some_and(|editor| !editor.is_sensitive()) {
            return gtk4::glib::Propagation::Proceed;
        }
        if key == gdk::Key::Escape && timeline.disarm_add_zoom_tool() {
            return gtk4::glib::Propagation::Stop;
        }
        let Some(action) = history_key(key, modifiers) else {
            return gtk4::glib::Propagation::Proceed;
        };
        let changed = match action {
            HistoryKey::Undo => timeline.undo(),
            HistoryKey::Redo => timeline.redo(),
        };
        if changed {
            gtk4::glib::Propagation::Stop
        } else {
            // Nothing left to take back: let the key travel on rather than
            // swallowing it for a handler that might have work to do.
            gtk4::glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}

fn wire_export(
    window: &ApplicationWindow,
    stage: &Stage,
    timeline: &super::timeline::Timeline,
    shell: &Overlay,
) {
    let window = window.clone();
    let stage_handle = stage.clone();
    let timeline = timeline.clone();
    let shell = shell.clone();
    if let Some(editor) = shell.child() {
        let button = stage.export_button.downgrade();
        editor.connect_sensitive_notify(move |editor| {
            if let Some(button) = button.upgrade() {
                button.set_sensitive(editor.is_sensitive());
            }
        });
    }
    stage.export_button.connect_clicked(move |_| {
        if shell.child().is_some_and(|editor| !editor.is_sensitive()) {
            return;
        }
        timeline.pause();
        let Some(model) = export_model(&stage_handle, &timeline) else {
            return;
        };
        let retry = stage_handle.export_button.downgrade();
        if !crate::studio_upsell::gate(&window, super::studio::capabilities(&model), move || {
            if let Some(button) = retry.upgrade() {
                button.emit_clicked();
            }
        }) {
            return;
        }
        let source = model.source.clone();
        let stage = stage_handle.clone();
        let timeline = timeline.clone();
        let shell_for_options = shell.clone();
        let window = window.clone();
        super::export_options::show(
            &shell,
            &model,
            Rc::new(move |settings, crf, clipboard| {
                let stage = stage.clone();
                let timeline = timeline.clone();
                let shell = shell_for_options.clone();
                let export_window = window.clone();
                let begin = Rc::new(move |output| {
                    start_gated_video_export(
                        &export_window,
                        &stage,
                        &timeline,
                        &shell,
                        VideoExportRequest {
                            output,
                            settings,
                            crf,
                            clipboard,
                        },
                    );
                });
                if clipboard {
                    let folder = dirs::cache_dir()
                        .unwrap_or_else(std::env::temp_dir)
                        .join("apexshot")
                        .join("clipboard-exports");
                    if let Err(error) = std::fs::create_dir_all(&folder) {
                        eprintln!(
                            "[video editor] Could not create clipboard export folder: {error}"
                        );
                        // A normal save dialog still lets the user complete the export.
                        choose_export_path(&window, &source, begin);
                        return;
                    }
                    let id = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos();
                    begin(folder.join(format!("recording-{id}.mp4")));
                } else {
                    choose_export_path(&window, &source, begin);
                }
            }),
        );
    });
}

#[derive(Clone)]
struct VideoExportRequest {
    output: PathBuf,
    settings: super::export::ExportSettings,
    crf: u32,
    clipboard: bool,
}

fn start_gated_video_export(
    window: &ApplicationWindow,
    stage: &Stage,
    timeline: &super::timeline::Timeline,
    shell: &Overlay,
    request: VideoExportRequest,
) {
    let Some(mut model) = export_model(stage, timeline) else {
        return;
    };
    model.crf = request.crf;
    let retry_window = window.downgrade();
    let retry_stage = stage.clone();
    let retry_timeline = timeline.clone();
    let retry_shell = shell.downgrade();
    let retry_request = request.clone();
    if crate::studio_upsell::gate(window, super::studio::capabilities(&model), move || {
        if let (Some(window), Some(shell)) = (retry_window.upgrade(), retry_shell.upgrade()) {
            start_gated_video_export(
                &window,
                &retry_stage,
                &retry_timeline,
                &shell,
                retry_request.clone(),
            );
        }
    }) {
        super::export_card::show(
            shell,
            model,
            request.output,
            request.settings,
            request.clipboard,
        );
    }
}

/// Ask where the export goes, using the remembered folder when enabled.
fn choose_export_path(window: &ApplicationWindow, source: &Path, then: Rc<dyn Fn(PathBuf)>) {
    let chooser = FileChooserNative::new(
        Some(&t("Export video")),
        Some(window),
        FileChooserAction::Save,
        Some(&t("Export")),
        Some(&t("Cancel")),
    );
    let fallback = source.parent().unwrap_or_else(|| Path::new("."));
    let folder = crate::config::load_config().video_editor_export_dir(fallback);
    let _ = chooser.set_current_folder(Some(&gio::File::for_path(folder)));
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("recording");
    chooser.set_current_name(&format!("{stem}-edited.mp4"));
    let filter = FileFilter::new();
    filter.set_name(Some(&t("MP4 video")));
    filter.add_pattern("*.mp4");
    filter.add_pattern("*.MP4");
    chooser.set_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response == ResponseType::Accept {
            if let Some(mut path) = chooser.file().and_then(|file| file.path()) {
                let has_extension = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("mp4"));
                if !has_extension {
                    path.set_extension("mp4");
                }
                let mut config = crate::config::load_config();
                if config.remember_video_export_dir(&path) {
                    if let Err(error) = crate::config::save_config(&config) {
                        eprintln!("[video editor] Could not remember export folder: {error:#}");
                    }
                }
                then(path);
            }
        }
        chooser.hide();
    });
    chooser.show();
}

fn document_title(path: Option<&Path>) -> String {
    path.map(|path| document_title_for_path(Some(path)))
        .unwrap_or_else(|| t("Untitled recording"))
}

fn document_title_for_path(path: Option<&Path>) -> String {
    path.and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| t("Untitled recording"))
}

fn is_supported_video(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
}

fn default_window_size(scale: f64) -> (i32, i32) {
    let requested = scaled_pair(1280.0, 860.0, scale);
    let Some(display) = gdk::Display::default() else {
        return requested;
    };
    let Some(monitor) = display.monitors().item(0).and_downcast::<gdk::Monitor>() else {
        return requested;
    };
    let geometry = monitor.geometry();
    let max_width = (f64::from(geometry.width()) * 0.92).round() as i32;
    let max_height = (f64::from(geometry.height()) * 0.90).round() as i32;
    (
        requested.0.min(max_width).max(1),
        requested.1.min(max_height).max(1),
    )
}

fn scaled_pair(width: f64, height: f64, scale: f64) -> (i32, i32) {
    (
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    )
}

fn install_css() {
    crate::settings::install_window_css();
    super::ui_support::install_recording_editor_css();
    super::side_panel::install_css();
    super::export_card::install_css();
    // The clip menu is the shared select menu, so it needs the Settings sheet
    // that draws its surface and rows.
    crate::settings::shell::install_select_menu_css();
    super::type_scale::install();

    let Some(display) = gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round() as i32;
    let css = format!(
        r#"
        window.video-editor-window {{
            background: transparent;
        }}
        .video-editor-root {{
            border: 1px solid rgba(255,255,255,0.12);
            border-radius: 12px;
        }}
        .video-editor-root.editor-theme-light {{
            border: 1px solid rgba(17,24,39,0.12);
        }}
        .video-editor-shell {{
            border-radius: 11px;
        }}
        .video-editor-titlebar {{
            min-height: {titlebar}px;
            padding: 0;
            border-radius: 12px 12px 0 0;
        }}
        .video-editor-document-title {{
            font-size: {title}px;
            font-weight: 500;
        }}
        .recording-editor-tools-column {{
            min-width: {tools_w}px;
        }}
        .video-editor-tool-panel {{
            padding: {panel_pad}px {panel_side}px;
        }}
        .video-editor-panel-head {{
            padding: {panel_head_y}px 0 {panel_head_bottom}px 0;
        }}
        .video-editor-panel-title {{
            color: alpha(white,0.92);
            font-size: {panel_title}px;
            font-weight: 600;
        }}
        .editor-theme-light .video-editor-panel-title {{
            color: alpha(#111827,0.92);
        }}
        .video-editor-panel-detail {{
            color: alpha(white,0.48);
            font-size: {panel_detail}px;
        }}
        .editor-theme-light .video-editor-panel-detail {{
            color: alpha(#111827,0.54);
        }}
        .video-editor-panel-rule {{
            min-height: 1px;
            background: alpha(white,0.08);
        }}
        .editor-theme-light .video-editor-panel-rule {{
            background: alpha(#111827,0.09);
        }}
        .video-editor-panel-placeholder {{
            margin-top: {placeholder_top}px;
            color: alpha(white,0.38);
            font-size: {placeholder}px;
        }}
        .editor-theme-light .video-editor-panel-placeholder {{
            color: alpha(#111827,0.46);
        }}
        .video-editor-add-zoom-bar {{
            padding: {add_zoom_y}px {add_zoom_x}px;
            border-radius: 999px;
            background: alpha(black,0.62);
        }}
        .video-editor-add-zoom-hint {{
            color: alpha(white,0.92);
            font-size: {add_zoom_text}px;
        }}
        button.video-editor-add-zoom-auto {{
            min-height: 0;
            padding: {add_zoom_button_y}px {add_zoom_button_x}px;
            border-radius: 999px;
            color: white;
            background: alpha(white,0.16);
            font-size: {add_zoom_text}px;
        }}
        button.video-editor-add-zoom-auto:hover {{
            background: alpha(white,0.24);
        }}
        .video-editor-root button.recording-editor-timeline-tool.video-editor-tool-armed {{
            background: alpha(white,0.12);
        }}
        .video-editor-root.editor-theme-light button.recording-editor-timeline-tool.video-editor-tool-armed {{
            background: alpha(#111827,0.09);
        }}
        "#,
        titlebar = px(TITLE_BAR_HEIGHT),
        title = px(15.0),
        tools_w = px(super::side_panel::TOOL_COLUMN_WIDTH as f64),
        panel_pad = px(10.0),
        panel_side = px(16.0),
        panel_head_y = px(14.0),
        panel_head_bottom = px(12.0),
        panel_title = px(15.0),
        panel_detail = px(13.0),
        placeholder_top = px(16.0),
        placeholder = px(13.0),
        add_zoom_y = px(6.0),
        add_zoom_x = px(8.0),
        add_zoom_text = px(13.0),
        add_zoom_button_y = px(3.0),
        add_zoom_button_x = px(10.0),
    );

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        canvas_dimensions, card_radius, estimated_size_bytes, format_size, history_key,
        output_dimensions, padding_px, screen_transform, split_stage, stage_source,
        video_rect_dimensions, FrameAspect, HistoryKey, QualityTier,
    };

    #[test]
    fn the_auto_stage_widens_for_the_split() {
        assert_eq!(
            stage_source(FrameAspect::Original, (320, 200), true, 0.0),
            (432, 200)
        );
        assert_eq!(
            stage_source(FrameAspect::Original, (320, 200), false, 0.0),
            (320, 200)
        );
        assert_eq!(
            stage_source(FrameAspect::Wide, (320, 200), true, 0.0),
            (320, 200)
        );
        assert_eq!(
            stage_source(FrameAspect::Original, (200, 320), true, 0.0),
            (200, 320)
        );
    }

    #[test]
    fn the_padded_split_settles_its_stage_and_gap() {
        assert_eq!(
            split_stage(FrameAspect::Original, (320, 200), true, false, 0.5, true),
            ((432, 200), 0.0)
        );
        assert_eq!(
            split_stage(FrameAspect::Original, (320, 200), true, true, 0.5, false),
            ((432, 200), 0.0)
        );
        let (stage, gap) = split_stage(FrameAspect::Original, (320, 200), true, true, 0.5, true);
        assert!(gap > 0.0, "the padded split has no gap: {gap}");
        assert!(
            stage.0 > 432,
            "the padded split did not widen the stage: {stage:?}"
        );
    }

    #[test]
    fn the_export_shell_contains_the_entire_editor_and_clips_its_corners() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            use gtk4::{Box as GtkBox, Orientation, Overflow};

            let root = GtkBox::new(Orientation::Vertical, 0);
            let title = GtkBox::new(Orientation::Horizontal, 0);
            let body = GtkBox::new(Orientation::Vertical, 0);
            let shell = super::build_editor_shell(&root, &title, &body);
            assert_eq!(root.first_child(), Some(shell.clone().upcast()));
            assert_eq!(root.last_child(), Some(shell.clone().upcast()));
            let editor = shell.child().unwrap();
            assert_eq!(editor.first_child(), Some(title.clone().upcast()));
            assert_eq!(editor.last_child(), Some(body.upcast()));
            assert_eq!(root.overflow(), Overflow::Hidden);
            assert_eq!(shell.overflow(), Overflow::Hidden);
            editor.set_sensitive(false);
            assert!(!title.is_sensitive());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn a_split_screen_transform_covers_the_video_pane() {
        let video = super::super::cursor::VideoRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        };
        let screen = super::super::webcam::Rect {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let view = screen_transform(video, screen, 0.0);
        assert!((view.clip.x - 10.0).abs() < 1e-9);
        assert!((view.clip.y - 20.0).abs() < 1e-9);
        assert!((view.clip.width - 100.0).abs() < 1e-9);
        assert!((view.clip.height - 100.0).abs() < 1e-9);
        assert!((view.draw.x - -40.0).abs() < 1e-9);
        assert!((view.draw.y - 20.0).abs() < 1e-9);
        assert!((view.draw.width - 200.0).abs() < 1e-9);
        assert!((view.draw.height - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_preview_paints_inside_the_split_screens_pane() {
        use gtk4::cairo::{Context, Format, ImageSurface};

        let mut surface = ImageSurface::create(Format::ARgb32, 320, 200).unwrap();
        let video = super::super::cursor::VideoRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        };
        {
            let context = Context::new(&surface).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().unwrap();
            for (x, colour) in [(0.0, (1.0, 0.0, 0.0)), (0.5, (0.0, 0.0, 1.0))] {
                let screen = super::super::webcam::Rect {
                    x,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                };
                let view = screen_transform(video, screen, 0.0);
                super::draw_in_video(
                    &context,
                    Some(video),
                    Some(view),
                    (320, 200),
                    |cr, width, height| {
                        assert_eq!((width, height), (200, 100));
                        cr.set_source_rgb(colour.0, colour.1, colour.2);
                        cr.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
                        cr.fill().unwrap();
                    },
                );
            }
        }
        let left = read_pixel(&mut surface, 13, 23);
        let right = read_pixel(&mut surface, 207, 70);
        let left_edge = read_pixel(&mut surface, 5, 70);
        let right_edge = read_pixel(&mut surface, 215, 70);
        assert!(
            left[2] > 200 && left[0] < 40,
            "the cover rect paints inside the left pane: {left:?}"
        );
        assert!(
            right[0] > 200 && right[2] < 40,
            "the cover rect paints inside the flipped pane: {right:?}"
        );
        assert!(
            left_edge[2] < 40 && right_edge[0] < 40,
            "each pane clips the cover rect: {left_edge:?} {right_edge:?}"
        );
    }

    fn read_pixel(surface: &mut gtk4::cairo::ImageSurface, x: u32, y: u32) -> [u8; 4] {
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let offset = y as usize * stride + x as usize * 4;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn ctrl_z_undoes_and_ctrl_shift_z_or_ctrl_y_redoes() {
        use gtk4::gdk::{Key, ModifierType};
        let ctrl = ModifierType::CONTROL_MASK;
        let shift = ModifierType::SHIFT_MASK;

        assert_eq!(history_key(Key::z, ctrl), Some(HistoryKey::Undo));
        // Backends differ on whether Shift folds into the keyval, so both
        // spellings of redo have to land on redo.
        assert_eq!(history_key(Key::z, ctrl | shift), Some(HistoryKey::Redo));
        assert_eq!(history_key(Key::Z, ctrl | shift), Some(HistoryKey::Redo));
        assert_eq!(history_key(Key::y, ctrl), Some(HistoryKey::Redo));
        // Without Ctrl the key belongs to whatever has focus, and an unrelated
        // Ctrl shortcut is not ours either.
        assert_eq!(history_key(Key::z, ModifierType::empty()), None);
        assert_eq!(history_key(Key::Escape, ctrl), None);

        // Nothing here can press a key, so pin the installation itself.
        let source = include_str!("window.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        assert!(
            production.contains("wire_history_keys(&window, &timeline, &shell)"),
            "the editor window must install the undo shortcut"
        );
    }

    #[test]
    fn quality_tiers_match_the_recorder_scale() {
        assert_eq!(QualityTier::Balanced.crf(), 23);
        assert_eq!(QualityTier::High.crf(), 20);
        assert_eq!(QualityTier::Ultra.crf(), 16);
        assert_eq!(QualityTier::default(), QualityTier::High);
    }

    #[test]
    fn a_fixed_frame_exports_at_1080p() {
        assert_eq!(
            output_dimensions(FrameAspect::Original, (1280, 720), 24.0, false),
            (1280, 720)
        );
        assert_eq!(
            output_dimensions(FrameAspect::Wide, (1280, 720), 24.0, true),
            (1920, 1080)
        );
        assert_eq!(
            output_dimensions(FrameAspect::Portrait, (1280, 720), 24.0, true),
            (1080, 1920)
        );
        assert_eq!(
            canvas_dimensions(FrameAspect::Original, (1280, 720)),
            (1280, 720)
        );
        assert_eq!(
            canvas_dimensions(FrameAspect::Wide, (1280, 720)),
            (1920, 1080)
        );
    }

    #[test]
    fn a_fill_grows_the_original_canvas_instead_of_insetting_it() {
        // Original holds no ratio, so the padding becomes a border around the
        // source: the video keeps its own pixel count and the canvas grows.
        let grown = output_dimensions(FrameAspect::Original, (1280, 720), 24.0, true);
        let pad = padding_px((1280, 720), 24.0, true).round() as u32;
        assert!(pad > 0);
        assert_eq!(grown, (1280 + pad * 2, 720 + pad * 2));
        assert_eq!(
            video_rect_dimensions(FrameAspect::Original, (1280, 720), 24.0, true),
            (1280, 720),
            "the video rect is the footage, not the padded canvas"
        );
    }

    #[test]
    fn a_fixed_frame_insets_the_video_rect_inside_its_own_ratio() {
        let out = output_dimensions(FrameAspect::Wide, (1280, 720), 24.0, true);
        let (video_w, video_h) = video_rect_dimensions(FrameAspect::Wide, (1280, 720), 24.0, true);
        assert!(video_w < out.0 && video_h < out.1);
        let source_ratio = 1280.0 / 720.0;
        let video_ratio = f64::from(video_w) / f64::from(video_h);
        assert!(
            (source_ratio - video_ratio).abs() < 0.01,
            "the video rect keeps the footage's aspect"
        );
        assert!(
            (f64::from(video_h) / f64::from(video_w) - 720.0 / 1280.0).abs() < 0.01,
            "a 16:9 source fills the width of a 16:9 frame"
        );
    }

    #[test]
    fn a_fill_never_lets_the_video_touch_the_background() {
        // The slider keeps the user's value; rendering floors it so the video
        // never sits edge to edge against the fill.
        let floor = padding_px((1280, 720), 0.0, true);
        assert_eq!(floor, 8.0 / 400.0 * 1280.0);
        assert_eq!(padding_px((1280, 720), 40.0, true), 40.0 / 400.0 * 1280.0);
        assert_eq!(padding_px((1280, 720), 40.0, false), 0.0);
    }

    #[test]
    fn the_estimate_scales_with_the_trim() {
        let full = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1920, 1080),
            (1920, 1080),
            QualityTier::Balanced,
        );
        let half = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            10.0,
            (1920, 1080),
            (1920, 1080),
            QualityTier::Balanced,
        );
        assert_eq!(full, 100 * 1024 * 1024);
        assert_eq!(half, 50 * 1024 * 1024);
    }

    #[test]
    fn the_estimate_follows_the_quality_tier() {
        let balanced = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1920, 1080),
            (1920, 1080),
            QualityTier::Balanced,
        );
        let high = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1920, 1080),
            (1920, 1080),
            QualityTier::High,
        );
        let ultra = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1920, 1080),
            (1920, 1080),
            QualityTier::Ultra,
        );
        assert!(balanced < high && high < ultra);
    }

    #[test]
    fn the_estimate_follows_the_export_pixels() {
        // A 720p recording grown to the 16:9 frame exports more pixels than
        // it holds, and a 4K one shrunk to that frame exports fewer.
        let small_source = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1280, 720),
            canvas_dimensions(FrameAspect::Original, (1280, 720)),
            QualityTier::High,
        );
        let small_framed = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (1280, 720),
            canvas_dimensions(FrameAspect::Wide, (1280, 720)),
            QualityTier::High,
        );
        assert!(small_framed > small_source);

        let large_source = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (3840, 2160),
            canvas_dimensions(FrameAspect::Original, (3840, 2160)),
            QualityTier::High,
        );
        let large_framed = estimated_size_bytes(
            100 * 1024 * 1024,
            20.0,
            20.0,
            (3840, 2160),
            canvas_dimensions(FrameAspect::Wide, (3840, 2160)),
            QualityTier::High,
        );
        assert!(large_framed < large_source);
    }

    #[test]
    fn a_missing_recording_estimates_nothing() {
        assert_eq!(
            estimated_size_bytes(0, 0.0, 0.0, (0, 0), (0, 0), QualityTier::High),
            0
        );
        assert_eq!(format_size(0), "0.0 MB");
    }

    #[test]
    fn small_exports_keep_one_decimal() {
        assert_eq!(format_size(1_500_000), "1.4 MB");
        assert_eq!(format_size(20 * 1024 * 1024), "20 MB");
    }

    #[test]
    fn the_corner_radius_scales_with_the_canvas_long_edge() {
        // The slider is in 400px-long-edge units: 40 on a 1080p canvas is a
        // tenth of the width, which is the largest the panel allows.
        assert_eq!(
            card_radius(FrameAspect::Original, (1920, 1080), 24.0, false, 40.0),
            (192.0, 1920.0)
        );
        // A fixed frame measures against its own canvas, not the source.
        assert_eq!(
            card_radius(FrameAspect::Wide, (1280, 720), 24.0, false, 40.0),
            (192.0, 1920.0)
        );
        assert_eq!(
            card_radius(FrameAspect::Portrait, (1920, 1080), 24.0, false, 40.0),
            (192.0, 1080.0)
        );
    }

    #[test]
    fn the_radius_is_measured_against_the_video_rect_not_the_padded_canvas() {
        // The clip is the video rect, so the radius must rescale to that width;
        // otherwise a padded canvas would draw corners that are either too
        // tight or too round for the footage they sit on.
        let (video_w, _) = video_rect_dimensions(FrameAspect::Wide, (1280, 720), 24.0, true);
        assert_eq!(
            card_radius(FrameAspect::Wide, (1280, 720), 24.0, true, 40.0),
            (192.0, f64::from(video_w))
        );
        // `Original` keeps the footage at its own size and grows the canvas, so
        // padding leaves the radius and its measuring width alone.
        assert_eq!(
            card_radius(FrameAspect::Original, (1920, 1080), 24.0, true, 40.0),
            (192.0, 1920.0)
        );
    }

    #[test]
    fn a_zero_radius_leaves_the_card_square() {
        assert_eq!(
            card_radius(FrameAspect::Original, (1920, 1080), 24.0, false, 0.0),
            (0.0, 1920.0)
        );
        assert_eq!(
            card_radius(FrameAspect::Original, (1920, 1080), 24.0, false, -4.0),
            (0.0, 1920.0)
        );
    }
}

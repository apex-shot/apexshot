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
use std::path::Path;
use std::rc::Rc;
use std::sync::Once;

type UpdateDocument = Rc<dyn Fn(Option<&Path>)>;

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
    let title_bar = build_title_bar(&window, document_title.clone(), scale);
    root.append(&title_bar);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let background = super::background::Background::default();
    let stage = build_stage(scale, background.clone());
    workspace.append(&stage.root);

    let tools_column = super::side_panel::build_tools_column(scale, &window, background);
    workspace.append(&tools_column);
    root.append(&workspace);

    let timeline = super::timeline::build(&window, &stage.canvas, scale, prefers_dark);
    root.append(&timeline.root);
    timeline.set_frame_sink({
        let preview = stage.preview.clone();
        let estimate = stage.estimate.clone();
        Rc::new(move |seconds, composition_seconds, playing| {
            preview.show_time(seconds, playing);
            estimate.set_composition_seconds(composition_seconds);
        })
    });

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

    window.present();
}

fn build_title_bar(
    window: &ApplicationWindow,
    document_title: Rc<RefCell<String>>,
    scale: f64,
) -> GtkBox {
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

    let actions = GtkBox::new(Orientation::Horizontal, 0);
    actions.set_halign(Align::End);
    actions.set_valign(Align::Fill);
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
    bar
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

    /// x264 CRF the exporter encodes with for this tier. Nothing writes an
    /// export yet, so only the tests read it for now.
    #[allow(dead_code)]
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

/// Even dimensions keep the encoder's yuv420p happy.
fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(2);
    if clamped % 2 == 0 {
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
    background: super::background::BackgroundState,
    background_layer: DrawingArea,
    background_pixbuf: Rc<RefCell<Option<Pixbuf>>>,
    background_fill: Rc<RefCell<Option<super::background::FillPainter>>>,
    background_selection: Rc<RefCell<Option<String>>>,
    source_size: Rc<Cell<Option<(u32, u32)>>>,
    clip: super::rounded_clip::RoundedClip,
    clip_margins: Rc<RefCell<(i32, i32, i32, i32)>>,
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
            return;
        };
        let aspect = self.aspect.get();
        let padding = self.background.padding();
        let has_fill = self.background.has_fill();
        let (out_w, out_h) = output_dimensions(aspect, source, padding, has_fill);
        let (video_w, video_h) = video_rect_dimensions(aspect, source, padding, has_fill);
        self.set_stage_ratio(out_w as f32 / out_h.max(1) as f32);
        self.set_video_margins((video_w, video_h), (out_w, out_h));
        let (radius, measured_width) =
            card_radius(aspect, source, padding, has_fill, self.background.radius());
        self.clip.set_radius(radius, measured_width);
    }

    /// Set the stage aspect only when it really moves: `AspectFrame` re-runs
    /// layout on a ratio change, and the resize that follows calls back here.
    fn set_stage_ratio(&self, ratio: f32) {
        if (self.canvas.ratio() - ratio).abs() > 0.001 {
            self.canvas.set_ratio(ratio.into());
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
        let Some((texture, width, height)) = path.and_then(poster_texture) else {
            self.picture.set_visible(false);
            self.hint.set_visible(true);
            self.source_size.set(None);
            self.preview.set_document(None, 0, 0);
            self.apply_geometry();
            self.estimate.set_source(None, 0.0, 0.0);
            return;
        };
        self.picture.set_paintable(Some(&texture));
        self.picture.set_visible(true);
        self.hint.set_visible(false);
        self.source_size.set(Some((width, height)));
        self.apply_geometry();
        if let Some(path) = path {
            self.preview.set_document(Some(path), width, height);
            self.estimate.set_pixels(width, height);
            self.estimate
                .set_source(Some(path), source_seconds, composition_seconds);
        }
    }
}

fn build_stage(scale: f64, background: super::background::Background) -> Stage {
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
    background_layer.set_draw_func({
        let background_pixbuf = background_pixbuf.clone();
        let background_fill = background_fill.clone();
        move |_, context, width, height| {
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

    let hint = Label::new(Some(&t("Click to open a video, or drop one here")));
    hint.add_css_class("recording-editor-empty-preview-hint");
    hint.set_halign(Align::Center);
    hint.set_valign(Align::Center);
    hint.set_wrap(true);
    hint.set_can_target(false);
    overlay.add_overlay(&hint);

    canvas.set_child(Some(&overlay));
    workspace.append(&canvas);
    frame.append(&workspace);

    let aspect = Rc::new(Cell::new(FrameAspect::Original));
    let source_size = Rc::new(Cell::new(None::<(u32, u32)>));
    let preview = super::preview::Preview::new(picture.clone());
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
        background: background.state(),
        background_layer: background_layer.clone(),
        background_pixbuf,
        background_fill,
        background_selection: Rc::new(RefCell::new(None)),
        source_size,
        clip,
        clip_margins: Rc::new(RefCell::new((i32::MIN, 0, 0, 0))),
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
            max-width: {tools_w}px;
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
        "#,
        titlebar = px(TITLE_BAR_HEIGHT),
        title = px(12.0),
        tools_w = px(super::side_panel::TOOL_COLUMN_WIDTH as f64),
        panel_pad = px(10.0),
        panel_side = px(16.0),
        panel_head_y = px(14.0),
        panel_head_bottom = px(12.0),
        panel_title = px(15.0),
        panel_detail = px(12.0),
        placeholder_top = px(16.0),
        placeholder = px(12.0),
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
        canvas_dimensions, card_radius, estimated_size_bytes, format_size, output_dimensions,
        padding_px, video_rect_dimensions, FrameAspect, QualityTier,
    };

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
